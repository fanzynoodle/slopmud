use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(dead_code)] // Protocol fields are matched by serde; test flow doesn't read every field.
enum JsonOut {
    Hello { mode: String },
    Attached { session: String },
    Output { text: String },
    Err { text: String },
    Pong {},
}

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum JsonIn<'a> {
    Attach { name: &'a str, is_bot: bool },
    Input { line: &'a str },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Dedicated ports.
    let shard_bind = "127.0.0.1:55041";
    let ws_bind = "127.0.0.1:41041";
    let ws_url = format!("ws://{ws_bind}/v1/json");

    let mut shard = Command::new("target/debug/shard_01")
        .env("SHARD_BIND", shard_bind)
        .env("WORLD_TICK_MS", "200")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::sleep(Duration::from_millis(800)).await;
    wait_tcp(shard_bind, Duration::from_secs(10)).await?;

    let mut gw = Command::new("target/debug/ws_gateway")
        .env("WS_BIND", ws_bind)
        .env("SHARD_ADDR", shard_bind)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::sleep(Duration::from_millis(800)).await;
    wait_tcp(ws_bind, Duration::from_secs(10)).await?;

    let mut bots = Command::new("target/debug/bot_party")
        .env("WS_URL", &ws_url)
        .env("BOTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::sleep(Duration::from_millis(800)).await;

    let res = run_client(&ws_url).await;

    let _ = bots.kill().await;
    let _ = gw.kill().await;
    let _ = shard.kill().await;

    res
}

async fn wait_tcp(bind: &str, timeout: Duration) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect(bind).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::bail!("timeout waiting for tcp {bind}");
}

async fn run_client(ws_url: &str) -> anyhow::Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(ws_url).await?;
    let (mut sink, mut stream) = ws.split();

    let attach = serde_json::to_string(&JsonIn::Attach {
        name: "Alice",
        is_bot: false,
    })?;
    sink.send(Message::Text(attach)).await?;

    let mut seen_invite = false;
    let mut seen_joined = false;
    let mut seen_lead = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let step = remaining.min(Duration::from_millis(300));
        let m = match tokio::time::timeout(step, stream.next()).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(m) = m else {
            break;
        };
        let m = m?;
        let Message::Text(s) = m else { continue };
        let Ok(msg) = serde_json::from_str::<JsonOut>(&s) else {
            continue;
        };
        match msg {
            JsonOut::Hello { .. } => {}
            JsonOut::Attached { .. } => {}
            JsonOut::Err { text } => {
                anyhow::bail!("ws err: {text}");
            }
            JsonOut::Output { text } => {
                if text.contains("party invite from") {
                    seen_invite = true;
                    let cmd = serde_json::to_string(&JsonIn::Input {
                        line: "party accept",
                    })?;
                    sink.send(Message::Text(cmd)).await?;
                }
                if text.contains("party: joined") {
                    seen_joined = true;
                }
                if text.contains("party: leader is now Alice") {
                    seen_lead = true;
                }
                if seen_invite && seen_joined && seen_lead {
                    println!("ws/bot_party e2e ok");
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    anyhow::bail!(
        "timed out (invite={}, joined={}, lead={})",
        seen_invite,
        seen_joined,
        seen_lead
    );
}
