use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use getrandom::getrandom;
use mudproto::session::SessionId;
use mudproto::shard::{ShardResp, REQ_ATTACH, REQ_DETACH, REQ_INPUT};
use serde::{Deserialize, Serialize};
use slopio::frame::{FrameReader, FrameWriter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn, Level};

mod ws_fb;

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    shard_addr: SocketAddr,
}

fn usage_and_exit() -> ! {
    eprintln!(
        "ws_gateway\n\n\
USAGE:\n  ws_gateway [--bind HOST:PORT] [--shard-addr HOST:PORT]\n\n\
ENV:\n  WS_BIND     default 127.0.0.1:4100\n  SHARD_ADDR  default 127.0.0.1:5000\n"
    );
    std::process::exit(2);
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("WS_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4100".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut shard_addr: SocketAddr = std::env::var("SHARD_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--shard-addr" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                shard_addr = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config { bind, shard_addr }
}

fn new_session_id() -> SessionId {
    let mut b = [0u8; 16];
    getrandom(&mut b).expect("getrandom");
    SessionId::from_be_bytes(b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Json,
    Fbs,
}

#[derive(Clone)]
struct ClientInfo {
    mode: Mode,
    tx: mpsc::Sender<Outbound>,
    attach_body: Bytes,
}

#[derive(Clone, Debug)]
enum Outbound {
    JsonText(String),
    FbsBin(Vec<u8>),
}

#[derive(Debug, Clone)]
struct ShardMsg {
    t: u8,
    session: SessionId,
    body: Bytes,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum JsonIn {
    Attach { name: String, is_bot: bool },
    Input { line: String },
    Detach {},
    Ping {},
}

#[derive(Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum JsonOut<'a> {
    Hello { mode: &'a str },
    Attached { session: String },
    Output { text: String },
    Err { text: String },
    Pong {},
}

fn sid_hex(s: SessionId) -> String {
    let b = s.to_be_bytes();
    let mut out = String::with_capacity(32);
    for x in b {
        out.push_str(&format!("{:02x}", x));
    }
    out
}

fn shard_attach_body(is_bot: bool, name: &str) -> Bytes {
    let mut b = Vec::with_capacity(1 + name.len());
    let mut flags = 0u8;
    if is_bot {
        flags |= 0x01;
    }
    b.push(flags);
    b.extend_from_slice(name.as_bytes());
    Bytes::from(b)
}

async fn shard_manager_task(
    shard_addr: SocketAddr,
    clients: Arc<tokio::sync::Mutex<HashMap<SessionId, ClientInfo>>>,
    mut rx: mpsc::Receiver<ShardMsg>,
) {
    let mut announced_down = false;

    loop {
        match TcpStream::connect(shard_addr).await {
            Ok(stream) => {
                announced_down = false;
                info!(shard_addr = %shard_addr, "connected to shard");

                let (rd, wr) = stream.into_split();
                let mut fr = FrameReader::new(rd);
                let mut fw = FrameWriter::new(wr);

                // Re-attach all live clients (best-effort).
                let snapshot = {
                    let m = clients.lock().await;
                    m.iter()
                        .map(|(sid, ci)| (*sid, ci.attach_body.clone()))
                        .collect::<Vec<_>>()
                };
                for (sid, body) in snapshot {
                    let _ = write_req(&mut fw, REQ_ATTACH, sid, &body).await;
                }
                let _ = fw.flush().await;

                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            let Some(msg) = msg else { return; };
                            let _ = write_req(&mut fw, msg.t, msg.session, &msg.body).await;
                        }
                        res = fr.read_frame() => {
                            let frame = match res {
                                Ok(Some(f)) => f,
                                Ok(None) => break,
                                Err(_) => break,
                            };
                            match mudproto::shard::parse_resp(frame) {
                                Ok(resp) => route_resp(resp, &clients).await,
                                Err(e) => {
                                    warn!(err=%e, "bad shard response");
                                }
                            }
                        }
                    }
                }

                warn!(shard_addr = %shard_addr, "shard disconnected; reconnecting");
            }
            Err(e) => {
                if !announced_down {
                    warn!(shard_addr = %shard_addr, err = %e, "shard down; retrying");
                    announced_down = true;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

async fn route_resp(
    resp: ShardResp,
    clients: &Arc<tokio::sync::Mutex<HashMap<SessionId, ClientInfo>>>,
) {
    match resp {
        ShardResp::Output { session, line } => {
            let ci = { clients.lock().await.get(&session).cloned() };
            if let Some(ci) = ci {
                match ci.mode {
                    Mode::Json => {
                        let text = String::from_utf8_lossy(&line).to_string();
                        let msg = JsonOut::Output { text };
                        if let Ok(s) = serde_json::to_string(&msg) {
                            let _ = ci.tx.send(Outbound::JsonText(s)).await;
                        }
                    }
                    Mode::Fbs => {
                        let body = line.to_vec();
                        let b = ws_fb::finish_frame_buf(
                            mudproto::shard::RESP_OUTPUT,
                            session.to_be_bytes(),
                            &body,
                        );
                        let _ = ci.tx.send(Outbound::FbsBin(b)).await;
                    }
                }
            }
        }
        ShardResp::Err { session, msg } => {
            let ci = { clients.lock().await.get(&session).cloned() };
            if let Some(ci) = ci {
                match ci.mode {
                    Mode::Json => {
                        let text = String::from_utf8_lossy(&msg).to_string();
                        let msg = JsonOut::Err { text };
                        if let Ok(s) = serde_json::to_string(&msg) {
                            let _ = ci.tx.send(Outbound::JsonText(s)).await;
                        }
                    }
                    Mode::Fbs => {
                        let body = msg.to_vec();
                        let b = ws_fb::finish_frame_buf(
                            mudproto::shard::RESP_ERR,
                            session.to_be_bytes(),
                            &body,
                        );
                        let _ = ci.tx.send(Outbound::FbsBin(b)).await;
                    }
                }
            }
        }
    }
}

async fn write_req(
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    t: u8,
    session: SessionId,
    body: &[u8],
) -> std::io::Result<()> {
    let mut hdr = [0u8; 1 + SessionId::LEN];
    hdr[0] = t;
    hdr[1..].copy_from_slice(&session.to_be_bytes());
    fw.write_frame_parts(&[&hdr, body]).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,ws_gateway=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();
    let listener = TcpListener::bind(cfg.bind).await?;

    let clients: Arc<tokio::sync::Mutex<HashMap<SessionId, ClientInfo>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    let (shard_tx, shard_rx) = mpsc::channel::<ShardMsg>(4096);
    tokio::spawn(shard_manager_task(
        cfg.shard_addr,
        clients.clone(),
        shard_rx,
    ));

    info!(bind=%cfg.bind, shard_addr=%cfg.shard_addr, "ws gateway listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let clients = clients.clone();
        let shard_tx = shard_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_ws_conn(stream, peer, clients, shard_tx).await {
                warn!(peer=%peer, err=%e, "ws connection ended with error");
            }
        });
    }
}

async fn handle_ws_conn(
    stream: TcpStream,
    peer: SocketAddr,
    clients: Arc<tokio::sync::Mutex<HashMap<SessionId, ClientInfo>>>,
    shard_tx: mpsc::Sender<ShardMsg>,
) -> anyhow::Result<()> {
    let mut mode = Mode::Json;
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .context("accept ws")?;

    // Determine mode by peer request path is not directly exposed post-accept.
    // Clients indicate mode via their first message; JSON defaults.
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::channel::<Outbound>(128);

    // Writer task.
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let res = match msg {
                Outbound::JsonText(s) => sink.send(Message::Text(s)).await,
                Outbound::FbsBin(b) => sink.send(Message::Binary(b)).await,
            };
            if res.is_err() {
                break;
            }
        }
    });

    // Send hello (JSON only; FBS clients should ignore text frames).
    let _ = tx
        .send(Outbound::JsonText(
            serde_json::to_string(&JsonOut::Hello { mode: "json" }).unwrap_or_default(),
        ))
        .await;

    let mut session: Option<SessionId> = None;
    while let Some(m) = stream.next().await {
        let m = match m {
            Ok(m) => m,
            Err(_) => break,
        };
        match m {
            Message::Text(s) => {
                if mode != Mode::Json {
                    // Ignore unexpected.
                    continue;
                }
                let Ok(j) = serde_json::from_str::<JsonIn>(&s) else {
                    let _ = tx
                        .send(Outbound::JsonText(
                            serde_json::to_string(&JsonOut::Err {
                                text: "bad json".to_string(),
                            })
                            .unwrap_or_default(),
                        ))
                        .await;
                    continue;
                };
                match j {
                    JsonIn::Attach { name, is_bot } => {
                        if session.is_some() {
                            continue;
                        }
                        let sid = new_session_id();
                        session = Some(sid);
                        let body = shard_attach_body(is_bot, name.trim());
                        clients.lock().await.insert(
                            sid,
                            ClientInfo {
                                mode: Mode::Json,
                                tx: tx.clone(),
                                attach_body: body.clone(),
                            },
                        );
                        let _ = shard_tx
                            .send(ShardMsg {
                                t: REQ_ATTACH,
                                session: sid,
                                body,
                            })
                            .await;
                        let _ = tx
                            .send(Outbound::JsonText(
                                serde_json::to_string(&JsonOut::Attached {
                                    session: sid_hex(sid),
                                })
                                .unwrap_or_default(),
                            ))
                            .await;
                    }
                    JsonIn::Input { line } => {
                        let Some(sid) = session else {
                            continue;
                        };
                        let _ = shard_tx
                            .send(ShardMsg {
                                t: REQ_INPUT,
                                session: sid,
                                body: Bytes::from(line.into_bytes()),
                            })
                            .await;
                    }
                    JsonIn::Detach {} => {
                        let Some(sid) = session.take() else {
                            continue;
                        };
                        clients.lock().await.remove(&sid);
                        let _ = shard_tx
                            .send(ShardMsg {
                                t: REQ_DETACH,
                                session: sid,
                                body: Bytes::new(),
                            })
                            .await;
                    }
                    JsonIn::Ping {} => {
                        let _ = tx
                            .send(Outbound::JsonText(
                                serde_json::to_string(&JsonOut::Pong {}).unwrap_or_default(),
                            ))
                            .await;
                    }
                }
            }
            Message::Binary(b) => {
                // First binary message flips the connection to fbs mode.
                mode = Mode::Fbs;
                let buf = b;
                let frame = unsafe { flatbuffers::root_unchecked::<ws_fb::Frame>(&buf) };
                let t = frame.t();
                let Some(sess_v) = frame.session() else {
                    continue;
                };
                if sess_v.len() != 16 {
                    continue;
                }
                let mut sid = [0u8; 16];
                for i in 0..16 {
                    sid[i] = sess_v.get(i);
                }
                let sid = SessionId::from_be_bytes(sid);

                let mut body_vec = Vec::new();
                if let Some(bv) = frame.body() {
                    body_vec.reserve(bv.len());
                    for i in 0..bv.len() {
                        body_vec.push(bv.get(i));
                    }
                }
                let body = Bytes::from(body_vec);

                if session.is_none() {
                    // First message must be attach so we can persist reattach metadata.
                    if t != REQ_ATTACH {
                        continue;
                    }
                    session = Some(sid);
                    clients.lock().await.insert(
                        sid,
                        ClientInfo {
                            mode: Mode::Fbs,
                            tx: tx.clone(),
                            attach_body: body.clone(),
                        },
                    );
                }
                let _ = shard_tx
                    .send(ShardMsg {
                        t,
                        session: sid,
                        body,
                    })
                    .await;
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
            _ => {}
        }
    }

    // Cleanup.
    if let Some(sid) = session.take() {
        clients.lock().await.remove(&sid);
        let _ = shard_tx
            .send(ShardMsg {
                t: REQ_DETACH,
                session: sid,
                body: Bytes::new(),
            })
            .await;
    }
    drop(tx);
    let _ = writer.await;
    info!(peer=%peer, "ws client disconnected");
    Ok(())
}
