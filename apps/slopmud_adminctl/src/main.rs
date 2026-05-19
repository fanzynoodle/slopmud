use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use base64::Engine;
use serde::Serialize;
use slopmud_walbackup::{
    RecoveryTarget, S3WalBackupClient, WalBackupManifest, extent_summary_lines,
    list_local_manifests, manifest_summary_lines, select_manifest_from_list, store_for_manifest,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud_adminctl\n\n\
USAGE:\n\
  slopmud_adminctl [--admin-addr HOST:PORT] <command> [args...]\n\n\
ENV:\n\
  SLOPMUD_ADMIN_ADDR  default 127.0.0.1:4011\n\n\
COMMANDS:\n\
  reset-password <name> [--password PW]\n\
  create-admin <name>   [--password PW]\n\
  promote-admin <name>\n\
  get-account <name>\n\
  list-accounts\n\
  wal-backup list (--dir DIR | --s3 s3://BUCKET/PREFIX) [--extents] [--json]\n\
  wal-backup recover (--dir DIR | --s3 s3://BUCKET/PREFIX --cache-dir DIR) --out PATH [--until-offset N|--until-index N|--until-ms N] [--manifest-unix-at-or-before S]\n\
  wal-backup verify --dir DIR\n"
    );
    std::process::exit(2);
}

fn gen_password() -> anyhow::Result<String> {
    // URL-safe, copy/paste friendly.
    let mut b = [0u8; 18];
    getrandom::getrandom(&mut b).map_err(|e| anyhow::anyhow!("getrandom failed: {e:?}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b))
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdminReq {
    CreateAccountPassword {
        name: String,
        password: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caps: Option<Vec<String>>,
    },
    SetAccountPassword {
        name: String,
        password: String,
    },
    GrantAccountCaps {
        name: String,
        caps: Vec<String>,
    },
    GetAccount {
        name: String,
    },
    ListAccounts {},
}

async fn send_admin_req(addr: SocketAddr, req: &AdminReq) -> anyhow::Result<serde_json::Value> {
    let mut stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect {addr}"))?;
    let line = serde_json::to_string(req)?;
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let mut rd = BufReader::new(stream);
    let mut out = Vec::new();
    rd.read_until(b'\n', &mut out).await?;
    if out.is_empty() {
        anyhow::bail!("empty admin response");
    }
    let s = String::from_utf8_lossy(&out);
    let v: serde_json::Value = serde_json::from_str(s.trim())
        .with_context(|| format!("bad json response: {}", s.trim()))?;
    Ok(v)
}

fn take_flag_value(rest: &[String], flag: &str) -> Option<String> {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == flag {
            return rest.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WalBackupSource {
    Local(PathBuf),
    S3 {
        uri: String,
        cache_dir: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WalBackupCliCommand {
    List {
        source: WalBackupSource,
        extents: bool,
        json: bool,
    },
    Recover {
        source: WalBackupSource,
        out: PathBuf,
        target: RecoveryTarget,
        manifest_unix_at_or_before: Option<u64>,
    },
    Verify {
        dir: PathBuf,
    },
}

fn parse_wal_backup_args(rest: &[String]) -> anyhow::Result<WalBackupCliCommand> {
    let Some(subcmd) = rest.first().map(String::as_str) else {
        anyhow::bail!("missing wal-backup subcommand");
    };
    let args = &rest[1..];
    match subcmd {
        "list" => {
            let (source, _) = parse_wal_backup_source(args, false)?;
            Ok(WalBackupCliCommand::List {
                source,
                extents: has_flag(args, "--extents"),
                json: has_flag(args, "--json"),
            })
        }
        "recover" => {
            let (source, _) = parse_wal_backup_source(args, true)?;
            let out = flag_value(args, "--out")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("wal-backup recover requires --out PATH"))?;
            let target = parse_recovery_target(args)?;
            let manifest_unix_at_or_before = flag_value(args, "--manifest-unix-at-or-before")
                .map(|v| v.parse().context("parse --manifest-unix-at-or-before"))
                .transpose()?;
            Ok(WalBackupCliCommand::Recover {
                source,
                out,
                target,
                manifest_unix_at_or_before,
            })
        }
        "verify" => {
            let dir = flag_value(args, "--dir")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("wal-backup verify requires --dir DIR"))?;
            Ok(WalBackupCliCommand::Verify { dir })
        }
        _ => anyhow::bail!("unknown wal-backup subcommand {subcmd:?}"),
    }
}

fn parse_wal_backup_source(
    args: &[String],
    recover: bool,
) -> anyhow::Result<(WalBackupSource, usize)> {
    let dir = flag_value(args, "--dir").map(PathBuf::from);
    let s3 = flag_value(args, "--s3");
    match (dir, s3) {
        (Some(dir), None) => Ok((WalBackupSource::Local(dir), 1)),
        (None, Some(uri)) => {
            let cache_dir = flag_value(args, "--cache-dir").map(PathBuf::from);
            if recover && cache_dir.is_none() {
                anyhow::bail!("wal-backup recover from --s3 requires --cache-dir DIR");
            }
            Ok((WalBackupSource::S3 { uri, cache_dir }, 1))
        }
        (Some(_), Some(_)) => anyhow::bail!("use only one of --dir or --s3"),
        (None, None) => anyhow::bail!("wal-backup requires --dir DIR or --s3 s3://BUCKET/PREFIX"),
    }
}

fn parse_recovery_target(args: &[String]) -> anyhow::Result<RecoveryTarget> {
    let mut targets = Vec::new();
    if let Some(v) = flag_value(args, "--until-offset") {
        targets.push(RecoveryTarget::Offset(
            v.parse().context("parse --until-offset")?,
        ));
    }
    if let Some(v) = flag_value(args, "--until-index") {
        targets.push(RecoveryTarget::Index(
            v.parse().context("parse --until-index")?,
        ));
    }
    if let Some(v) = flag_value(args, "--until-ms") {
        targets.push(RecoveryTarget::Ms(v.parse().context("parse --until-ms")?));
    }
    match targets.len() {
        0 => Ok(RecoveryTarget::Latest),
        1 => Ok(targets.remove(0)),
        _ => anyhow::bail!("use at most one recovery target"),
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

async fn handle_wal_backup(rest: &[String]) -> anyhow::Result<()> {
    match parse_wal_backup_args(rest)? {
        WalBackupCliCommand::List {
            source,
            extents,
            json,
        } => {
            let manifests = load_manifests_for_source(&source).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&manifests)?);
                return Ok(());
            }
            for line in manifest_summary_lines(&manifests) {
                println!("{line}");
            }
            if extents {
                if let Some(manifest) = select_manifest_from_list(&manifests, None) {
                    for line in extent_summary_lines(&manifest.segments) {
                        println!("{line}");
                    }
                }
            }
        }
        WalBackupCliCommand::Recover {
            source,
            out,
            target,
            manifest_unix_at_or_before,
        } => {
            let (root, manifest) =
                prepare_manifest_for_recovery(&source, manifest_unix_at_or_before).await?;
            let store = store_for_manifest(root, &manifest);
            let bytes = store.recover_to_path(&manifest, &out, target)?;
            println!(
                "recovered bytes={} source_len={} manifest={} out={}",
                bytes,
                manifest.source_len,
                manifest.manifest_relpath,
                out.display()
            );
        }
        WalBackupCliCommand::Verify { dir } => {
            let manifests = list_local_manifests(&dir)?;
            if manifests.is_empty() {
                println!("no wal backup manifests found");
                return Ok(());
            }
            for manifest in manifests {
                let store = store_for_manifest(dir.clone(), &manifest);
                let report = store.verify_manifest(&manifest)?;
                println!(
                    "ok manifest={} segments={} bytes={}",
                    manifest.manifest_relpath, report.checked_segments, report.bytes
                );
            }
        }
    }
    Ok(())
}

async fn load_manifests_for_source(
    source: &WalBackupSource,
) -> anyhow::Result<Vec<WalBackupManifest>> {
    match source {
        WalBackupSource::Local(dir) => list_local_manifests(dir),
        WalBackupSource::S3 { uri, .. } => {
            S3WalBackupClient::from_uri(uri)
                .await?
                .list_manifests()
                .await
        }
    }
}

async fn prepare_manifest_for_recovery(
    source: &WalBackupSource,
    manifest_unix_at_or_before: Option<u64>,
) -> anyhow::Result<(PathBuf, WalBackupManifest)> {
    match source {
        WalBackupSource::Local(dir) => {
            let manifests = list_local_manifests(dir)?;
            let manifest = select_manifest_from_list(&manifests, manifest_unix_at_or_before)
                .ok_or_else(|| anyhow::anyhow!("no matching local WAL backup manifest"))?;
            Ok((dir.clone(), manifest))
        }
        WalBackupSource::S3 { uri, cache_dir } => {
            let cache_dir = cache_dir
                .clone()
                .ok_or_else(|| anyhow::anyhow!("recover from S3 requires --cache-dir"))?;
            let client = S3WalBackupClient::from_uri(uri).await?;
            let manifests = client.list_manifests().await?;
            let manifest = select_manifest_from_list(&manifests, manifest_unix_at_or_before)
                .ok_or_else(|| anyhow::anyhow!("no matching S3 WAL backup manifest"))?;
            client
                .download_manifest_to_cache(&manifest, &cache_dir)
                .await?;
            Ok((cache_dir, manifest))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut admin_addr: SocketAddr = std::env::var("SLOPMUD_ADMIN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4011".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut args = std::env::args().skip(1);
    let mut cmd: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();

    while let Some(a) = args.next() {
        if a == "--admin-addr" {
            let v = args.next().unwrap_or_else(|| usage_and_exit());
            admin_addr = v.parse().unwrap_or_else(|_| usage_and_exit());
            continue;
        }
        cmd = Some(a);
        rest.extend(args);
        break;
    }

    let Some(cmd) = cmd else { usage_and_exit() };

    match cmd.as_str() {
        "reset-password" => {
            if rest.is_empty() {
                usage_and_exit();
            }
            let name = rest[0].clone();
            let password = if let Some(pw) = take_flag_value(&rest[1..], "--password") {
                pw
            } else {
                gen_password()?
            };

            println!("password: {password}");
            let resp = send_admin_req(admin_addr, &AdminReq::SetAccountPassword { name, password })
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        "create-admin" => {
            if rest.is_empty() {
                usage_and_exit();
            }
            let name = rest[0].clone();
            let password = if let Some(pw) = take_flag_value(&rest[1..], "--password") {
                pw
            } else {
                gen_password()?
            };

            println!("password: {password}");
            let resp = send_admin_req(
                admin_addr,
                &AdminReq::CreateAccountPassword {
                    name,
                    password,
                    caps: Some(vec!["admin.all".to_string()]),
                },
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        "promote-admin" => {
            if rest.len() != 1 {
                usage_and_exit();
            }
            let name = rest[0].clone();
            let resp = send_admin_req(
                admin_addr,
                &AdminReq::GrantAccountCaps {
                    name,
                    caps: vec!["admin.all".to_string()],
                },
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        "get-account" => {
            if rest.len() != 1 {
                usage_and_exit();
            }
            let name = rest[0].clone();
            let resp = send_admin_req(admin_addr, &AdminReq::GetAccount { name }).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        "list-accounts" => {
            if !rest.is_empty() {
                usage_and_exit();
            }
            let resp = send_admin_req(admin_addr, &AdminReq::ListAccounts {}).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        "wal-backup" => handle_wal_backup(&rest).await?,
        _ => usage_and_exit(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_wal_backup_list_local_extents_json() {
        let cmd = parse_wal_backup_args(&args(&[
            "list",
            "--dir",
            "/tmp/backups",
            "--extents",
            "--json",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            WalBackupCliCommand::List {
                source: WalBackupSource::Local(PathBuf::from("/tmp/backups")),
                extents: true,
                json: true,
            }
        );
    }

    #[test]
    fn parses_wal_backup_recover_s3_with_cache_and_index_target() {
        let cmd = parse_wal_backup_args(&args(&[
            "recover",
            "--s3",
            "s3://bucket/prefix",
            "--cache-dir",
            "/tmp/cache",
            "--out",
            "/tmp/raft.jsonl",
            "--until-index",
            "42",
            "--manifest-unix-at-or-before",
            "1000",
        ]))
        .unwrap();
        assert_eq!(
            cmd,
            WalBackupCliCommand::Recover {
                source: WalBackupSource::S3 {
                    uri: "s3://bucket/prefix".to_string(),
                    cache_dir: Some(PathBuf::from("/tmp/cache")),
                },
                out: PathBuf::from("/tmp/raft.jsonl"),
                target: RecoveryTarget::Index(42),
                manifest_unix_at_or_before: Some(1000),
            }
        );
    }

    #[test]
    fn rejects_recover_with_multiple_point_in_time_targets() {
        let err = parse_wal_backup_args(&args(&[
            "recover",
            "--dir",
            "/tmp/backups",
            "--out",
            "/tmp/raft.jsonl",
            "--until-index",
            "42",
            "--until-ms",
            "9000",
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("at most one recovery target"));
    }

    #[test]
    fn rejects_s3_recover_without_cache_dir() {
        let err = parse_wal_backup_args(&args(&[
            "recover",
            "--s3",
            "s3://bucket/prefix",
            "--out",
            "/tmp/raft.jsonl",
        ]))
        .unwrap_err()
        .to_string();
        assert!(err.contains("requires --cache-dir"));
    }
}
