use std::time::Duration;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn, Level};

#[derive(Clone, Debug)]
struct Config {
    ws_url: String,
    bots: u32,
}

fn usage_and_exit() -> ! {
    eprintln!(
        "bot_party\n\n\
USAGE:\n  bot_party [--ws URL] [--bots N]\n\n\
ENV:\n  WS_URL  default ws://127.0.0.1:4100/v1/json\n  BOTS    default 2\n"
    );
    std::process::exit(2);
}

fn parse_args() -> Config {
    let mut ws_url =
        std::env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:4100/v1/json".to_string());
    let mut bots: u32 = std::env::var("BOTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
        .max(1);

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ws" => ws_url = it.next().unwrap_or_else(|| usage_and_exit()),
            "--bots" => {
                bots = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage_and_exit())
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config { ws_url, bots }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(dead_code)] // Protocol fields are matched by serde; the bot doesn't read every field.
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
    Ping {},
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,bot_party=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();
    info!(ws_url=%cfg.ws_url, bots=%cfg.bots, "bot party starting");

    for i in 0..cfg.bots {
        let name = format!("Buddy{}", i + 1);
        let url = cfg.ws_url.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = bot_loop(&url, &name).await {
                    warn!(bot=%name, err=%e, "bot loop error; retrying");
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }

    // Run forever.
    futures_util::future::pending::<()>().await;
    Ok(())
}

async fn bot_loop(ws_url: &str, bot_name: &str) -> anyhow::Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .with_context(|| format!("connect {ws_url}"))?;
    let (mut sink, mut stream) = ws.split();

    // Attach.
    let attach = serde_json::to_string(&JsonIn::Attach {
        name: bot_name,
        is_bot: true,
    })?;
    sink.send(Message::Text(attach)).await?;

    let mut seen_humans: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_ping = tokio::time::Instant::now();

    loop {
        tokio::select! {
            m = stream.next() => {
                let Some(m) = m else { break; };
                let m = m?;
                match m {
                    Message::Text(s) => {
                        let Ok(msg) = serde_json::from_str::<JsonOut>(&s) else { continue; };
                        match msg {
                            JsonOut::Hello { .. } => {}
                            JsonOut::Attached { .. } => {}
                            JsonOut::Pong {} => {}
                            JsonOut::Err { .. } => {}
                            JsonOut::Output { text } => {
                                // Detect joins in our room:
                                // "* Alice joined" or "* Alice joined (bot)"
                                if let Some((name, is_bot)) = parse_join_line(&text) {
                                    if name != bot_name
                                        && !is_bot
                                        && seen_humans.insert(name.to_string())
                                    {
                                        send_cmd(
                                            &mut sink,
                                            &format!(
                                                "say hi {name}. i'm {bot_name} (bot). want a party buddy?"
                                            ),
                                        )
                                        .await?;
                                        send_cmd(&mut sink, &format!("party invite {name}"))
                                            .await?;
                                        send_cmd(
                                            &mut sink,
                                            "say type: party accept (then i'll follow you)",
                                        )
                                        .await?;
                                    }
                                }

                                // If someone joined the party, transfer leadership and follow.
                                if text.contains("party: ") && text.contains(" joined") {
                                    for h in seen_humans.iter() {
                                        let _ = send_cmd(&mut sink, &format!("party lead {h}")).await;
                                    }
                                    let _ = send_cmd(&mut sink, "follow on").await;
                                    let _ = send_cmd(&mut sink, "assist on").await;
                                }
                            }
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if last_ping.elapsed() > Duration::from_secs(10) {
                    let ping = serde_json::to_string(&JsonIn::Ping {})?;
                    let _ = sink.send(Message::Text(ping)).await;
                    last_ping = tokio::time::Instant::now();
                }
            }
        }
    }

    Ok(())
}

fn parse_join_line(text: &str) -> Option<(&str, bool)> {
    // text may contain multiple lines. Look for a line starting "* " and containing " joined".
    for line in text.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("* ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(' ') else {
            continue;
        };
        if tail.starts_with("joined") {
            let is_bot = tail.contains("(bot)");
            return Some((name, is_bot));
        }
    }
    None
}

async fn send_cmd<S>(sink: &mut S, line: &str) -> anyhow::Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let msg = serde_json::to_string(&JsonIn::Input { line })?;
    sink.send(Message::Text(msg)).await?;
    Ok(())
}
