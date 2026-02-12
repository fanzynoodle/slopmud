use std::net::SocketAddr;

use anyhow::Context;
use base64::Engine;
use serde::Serialize;
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
  list-accounts\n"
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
        _ => usage_and_exit(),
    }

    Ok(())
}
