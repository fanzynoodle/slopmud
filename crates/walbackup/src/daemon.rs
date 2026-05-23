use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

use crate::WalRestoreOutcome;

pub const READY_PREFIX: &str = "SLOPMUD_WALBACKUPD_READY\t";
pub const DEFAULT_BIND: &str = "127.0.0.1:0";

#[derive(Clone, Debug)]
pub struct WalBackupDaemonClient {
    addr: SocketAddr,
    request_timeout: Duration,
}

impl WalBackupDaemonClient {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            request_timeout: request_timeout_from_env(),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        let reply = self.request("PING").await?;
        if reply == "pong" {
            Ok(())
        } else {
            bail!("unexpected walbackupd ping reply: {reply}");
        }
    }

    pub async fn restore_wal(&self) -> anyhow::Result<WalRestoreOutcome> {
        parse_restore_reply(&self.request("RESTORE_WAL").await?)
    }

    pub async fn start_wal_backup(&self) -> anyhow::Result<()> {
        let reply = self.request("START_WAL_BACKUP").await?;
        match reply.as_str() {
            "started" | "already_started" | "disabled" => Ok(()),
            _ => bail!("unexpected walbackupd backup reply: {reply}"),
        }
    }

    pub async fn enqueue_eventlog_uploads<I, S>(&self, relpaths: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cmd = String::from("EVENTLOG_UPLOAD");
        let mut count = 0usize;
        for rel in relpaths {
            cmd.push('\t');
            cmd.push_str(&encode_field(rel.as_ref()));
            count += 1;
        }
        if count == 0 {
            return Ok(());
        }
        let reply = self.request(&cmd).await?;
        if reply == "queued" || reply == "uploaded" || reply == "disabled" {
            Ok(())
        } else {
            bail!("unexpected walbackupd eventlog reply: {reply}");
        }
    }

    pub async fn scan_eventlog_uploads(&self) -> anyhow::Result<()> {
        let reply = self.request("EVENTLOG_SCAN").await?;
        if reply == "queued" || reply == "uploaded" || reply == "disabled" {
            Ok(())
        } else {
            bail!("unexpected walbackupd eventlog scan reply: {reply}");
        }
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let reply = self.request("SHUTDOWN").await?;
        if reply == "bye" {
            Ok(())
        } else {
            bail!("unexpected walbackupd shutdown reply: {reply}");
        }
    }

    async fn request(&self, command: &str) -> anyhow::Result<String> {
        if command
            .as_bytes()
            .iter()
            .any(|b| *b == b'\n' || *b == b'\r')
        {
            bail!("walbackupd command contains a newline");
        }

        let fut = async {
            let stream = TcpStream::connect(self.addr).await?;
            let (read_half, mut write_half) = stream.into_split();
            write_half.write_all(command.as_bytes()).await?;
            write_half.write_all(b"\n").await?;
            write_half.shutdown().await?;

            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                bail!("walbackupd closed without a reply");
            }
            parse_reply_line(&line)
        };

        tokio::time::timeout(self.request_timeout, fut)
            .await
            .context("walbackupd request timed out")?
    }
}

pub struct WalBackupDaemonHandle {
    child: Child,
    client: WalBackupDaemonClient,
}

impl WalBackupDaemonHandle {
    pub async fn spawn_for_wal_from_env(
        source_path: impl AsRef<Path>,
        node_id: &str,
    ) -> anyhow::Result<Self> {
        Self::spawn_from_env([
            (
                "SLOPMUD_WALD_SOURCE_PATH".to_string(),
                source_path.as_ref().display().to_string(),
            ),
            ("SLOPMUD_WALD_NODE_ID".to_string(), node_id.to_string()),
        ])
        .await
    }

    pub async fn spawn_from_env<I, K, V>(extra_env: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let bin = daemon_bin_from_env();
        let bind = std::env::var("SLOPMUD_WALBACKUPD_BIND")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BIND.to_string());
        let ready_timeout = ready_timeout_from_env();

        let mut cmd = Command::new(&bin);
        cmd.arg("--serve")
            .arg("--bind")
            .arg(&bind)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (key, value) in extra_env {
            cmd.env(key.as_ref(), value.as_ref());
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawn walbackupd {}", bin.display()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("walbackupd stdout was not piped"))?;
        let mut reader = BufReader::new(stdout);
        let mut ready = String::new();
        let n = tokio::time::timeout(ready_timeout, reader.read_line(&mut ready))
            .await
            .context("walbackupd readiness timed out")??;
        if n == 0 {
            bail!("walbackupd exited before readiness");
        }
        let addr = parse_ready_line(&ready)?;
        let client = WalBackupDaemonClient::new(addr);
        client.ping().await?;
        Ok(Self { child, client })
    }

    pub fn client(&self) -> WalBackupDaemonClient {
        self.client.clone()
    }

    pub fn addr(&self) -> SocketAddr {
        self.client.addr()
    }

    pub async fn restore_wal(&self) -> anyhow::Result<WalRestoreOutcome> {
        self.client.restore_wal().await
    }

    pub async fn start_wal_backup(&self) -> anyhow::Result<()> {
        self.client.start_wal_backup().await
    }

    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        let _ = self.client.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        Ok(())
    }
}

impl Drop for WalBackupDaemonHandle {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

pub fn format_ok(payload: &str) -> String {
    format!("OK\t{payload}\n")
}

pub fn format_err(err: &anyhow::Error) -> String {
    format!("ERR\t{}\n", encode_field(&err.to_string()))
}

pub fn format_restore_outcome(outcome: &WalRestoreOutcome) -> String {
    match outcome {
        WalRestoreOutcome::SkippedExisting { path, bytes } => {
            format!("skipped_existing\t{}\t{bytes}", encode_path(path))
        }
        WalRestoreOutcome::NoManifest => "no_manifest".to_string(),
        WalRestoreOutcome::Restored {
            path,
            bytes,
            source_len,
            manifest_relpath,
        } => format!(
            "restored\t{}\t{bytes}\t{source_len}\t{}",
            encode_path(path),
            encode_field(manifest_relpath)
        ),
    }
}

pub fn parse_restore_reply(raw: &str) -> anyhow::Result<WalRestoreOutcome> {
    let fields = raw.split('\t').collect::<Vec<_>>();
    match fields.as_slice() {
        ["skipped_existing", path, bytes] => Ok(WalRestoreOutcome::SkippedExisting {
            path: PathBuf::from(decode_field(path)?),
            bytes: bytes.parse()?,
        }),
        ["no_manifest"] => Ok(WalRestoreOutcome::NoManifest),
        ["restored", path, bytes, source_len, manifest_relpath] => {
            Ok(WalRestoreOutcome::Restored {
                path: PathBuf::from(decode_field(path)?),
                bytes: bytes.parse()?,
                source_len: source_len.parse()?,
                manifest_relpath: decode_field(manifest_relpath)?,
            })
        }
        _ => bail!("bad walbackupd restore reply: {raw}"),
    }
}

pub fn parse_ready_line(line: &str) -> anyhow::Result<SocketAddr> {
    let rest = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix(READY_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("bad walbackupd ready line: {line:?}"))?;
    Ok(rest.parse()?)
}

pub fn ready_line(addr: SocketAddr) -> String {
    format!("{READY_PREFIX}{addr}\n")
}

pub fn parse_reply_line(line: &str) -> anyhow::Result<String> {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix("OK\t") {
        Ok(rest.to_string())
    } else if let Some(rest) = line.strip_prefix("ERR\t") {
        bail!("{}", decode_field(rest)?);
    } else {
        bail!("bad walbackupd reply line: {line:?}");
    }
}

pub fn decode_command_fields(line: &str) -> anyhow::Result<Vec<String>> {
    line.trim_end_matches(['\r', '\n'])
        .split('\t')
        .map(decode_field)
        .collect()
}

pub fn encode_field(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '=') {
            out.push(c);
        } else {
            out.push('%');
            out.push(nibble_to_hex(b >> 4));
            out.push(nibble_to_hex(b & 0x0f));
        }
    }
    out
}

pub fn decode_field(raw: &str) -> anyhow::Result<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                bail!("bad percent escape in walbackupd field");
            }
            let hi = hex_to_nibble(bytes[i + 1])?;
            let lo = hex_to_nibble(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8(out)?)
}

fn encode_path(path: &Path) -> String {
    encode_field(&path.display().to_string())
}

fn daemon_bin_from_env() -> PathBuf {
    if let Some(path) = std::env::var("SLOPMUD_WALBACKUPD_BIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(mut exe) = std::env::current_exe() {
        exe.set_file_name(daemon_bin_name());
        return exe;
    }
    PathBuf::from(daemon_bin_name())
}

fn daemon_bin_name() -> &'static str {
    if cfg!(windows) {
        "slopmud_walbackupd.exe"
    } else {
        "slopmud_walbackupd"
    }
}

fn ready_timeout_from_env() -> Duration {
    Duration::from_millis(
        std::env::var("SLOPMUD_WALBACKUPD_READY_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000),
    )
}

fn request_timeout_from_env() -> Duration {
    Duration::from_millis(
        std::env::var("SLOPMUD_WALBACKUPD_REQUEST_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000),
    )
}

fn nibble_to_hex(n: u8) -> char {
    match n & 0x0f {
        0..=9 => (b'0' + (n & 0x0f)) as char,
        v => (b'A' + (v - 10)) as char,
    }
}

fn hex_to_nibble(b: u8) -> anyhow::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => bail!("bad hex digit in walbackupd field"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_fields_round_trip_spaces_tabs_and_unicode() {
        let raw = "/tmp/slop mud/char\tname/\u{2603}.log";
        let enc = encode_field(raw);
        assert!(!enc.contains('\t'));
        assert!(!enc.contains(' '));
        assert_eq!(decode_field(&enc).unwrap(), raw);
    }

    #[test]
    fn restore_outcome_round_trips() {
        let outcome = WalRestoreOutcome::Restored {
            path: PathBuf::from("/tmp/raft log.jsonl"),
            bytes: 12,
            source_len: 34,
            manifest_relpath: "v1/nodes/n0/lsmt/manifests/latest.json".to_string(),
        };
        let wire = format_restore_outcome(&outcome);
        assert_eq!(parse_restore_reply(&wire).unwrap(), outcome);
    }

    #[test]
    fn ready_line_round_trips() {
        let addr: SocketAddr = "127.0.0.1:5151".parse().unwrap();
        assert_eq!(parse_ready_line(&ready_line(addr)).unwrap(), addr);
    }
}
