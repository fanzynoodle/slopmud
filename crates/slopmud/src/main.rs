use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use memchr::memchr;
use mudproto::session::SessionId;
use mudproto::shard::{REQ_ATTACH, REQ_DETACH, REQ_INPUT, ShardResp};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use slopio::frame::{FrameReader, FrameWriter};
use slopio::telnet::IacParser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{Level, info, warn};
use zeroize::Zeroize;
use argon2::Argon2;

#[derive(Clone)]
struct ServerInfo {
    started_instant: std::time::Instant,
    started_unix: u64,
    shard_addr: SocketAddr,
    bind: SocketAddr,
}

const COC_LINE_ITEMS: [&str; 8] = [
    "1. nothing illegal",
    "2. hard R for violence, hard PG for sex/nudity",
    "3. no soliciting",
    "4. anything you submit - consider it publicly licensed and publicly published",
    "5. don't spam",
    "6. prioritize great experiences for humans",
    "7. don't lie about being a bot",
    "8. zero privacy (except passwords): we will share logs with various folks and train our models on them",
];

const RACE_TOKENS: [&str; 9] = [
    "dragonborn",
    "dwarf",
    "elf",
    "gnome",
    "goliath",
    "halfling",
    "human",
    "orc",
    "tiefling",
];

const CLASS_TOKENS: [&str; 12] = [
    "barbarian",
    "bard",
    "cleric",
    "druid",
    "fighter",
    "monk",
    "paladin",
    "ranger",
    "rogue",
    "sorcerer",
    "warlock",
    "wizard",
];

fn is_allowed_token(s: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|x| *x == s)
}

fn normalize_pronouns(locale: &str, s: &str) -> Option<&'static str> {
    // Locale-specific aliases can go here. For now we support a minimal English set.
    // Return canonical key.
    let lc = s.trim().to_ascii_lowercase();
    if lc.is_empty() {
        return None;
    }
    match locale {
        "en" | "en-us" | "en_us" => match lc.as_str() {
            "he" | "him" => Some("he"),
            "she" | "her" => Some("she"),
            "they" | "them" => Some("they"),
            _ => None,
        },
        _ => match lc.as_str() {
            "he" | "him" => Some("he"),
            "she" | "her" => Some("she"),
            "they" | "them" => Some("they"),
            _ => None,
        },
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud (session broker)\n\n\
USAGE:\n  slopmud [--bind HOST:PORT] [--shard-addr HOST:PORT]\n\n\
ENV:\n  SLOPMUD_BIND               default 0.0.0.0:4000\n  SHARD_ADDR                 default 127.0.0.1:5000\n  NODE_ID                    optional (for logs only)\n  SLOPMUD_ACCOUNTS_PATH       optional; default accounts.json (in WorkingDirectory)\n  SLOPMUD_LOCALE              optional; default en\n  SLOPMUD_GOOGLE_OAUTH_DIR    optional; default locks/google_oauth (shared with static_web)\n  SLOPMUD_GOOGLE_AUTH_BASE_URL optional; default http://127.0.0.1:8080 (where to open OAuth in browser)\n  SLOPMUD_OIDC_TOKEN_URL      optional; if set, mint a session token at login\n  SLOPMUD_OIDC_CLIENT_ID      required if token url set\n  SLOPMUD_OIDC_CLIENT_SECRET  required if token url set\n  SLOPMUD_OIDC_SCOPE          optional; default slopmud:session\n"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    shard_addr: SocketAddr,
    node_id: Option<String>,
    // Accounts DB (stores only password hashes, never raw passwords).
    accounts_path: String,
    // Directory used for cross-process OAuth handoffs (static_web writes results here).
    google_oauth_dir: String,
    // Base URL for the user to open in a browser for OAuth (points at static_web).
    google_auth_base_url: String,
    // If set, mint a session-scoped access token from an internal OIDC token endpoint.
    // The password is never sent to this service.
    oidc_token_url: Option<String>,
    oidc_client_id: Option<String>,
    oidc_client_secret: Option<String>,
    oidc_scope: Option<String>,
    locale: String,
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("SLOPMUD_BIND")
        .unwrap_or_else(|_| "0.0.0.0:4000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut shard_addr: SocketAddr = std::env::var("SHARD_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let node_id = std::env::var("NODE_ID").ok();
    let accounts_path =
        std::env::var("SLOPMUD_ACCOUNTS_PATH").unwrap_or_else(|_| "accounts.json".to_string());
    let google_oauth_dir =
        std::env::var("SLOPMUD_GOOGLE_OAUTH_DIR").unwrap_or_else(|_| "locks/google_oauth".to_string());
    let google_auth_base_url =
        std::env::var("SLOPMUD_GOOGLE_AUTH_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let oidc_token_url = std::env::var("SLOPMUD_OIDC_TOKEN_URL").ok();
    let oidc_client_id = std::env::var("SLOPMUD_OIDC_CLIENT_ID").ok();
    let oidc_client_secret = std::env::var("SLOPMUD_OIDC_CLIENT_SECRET").ok();
    let oidc_scope = std::env::var("SLOPMUD_OIDC_SCOPE").ok();
    let locale = std::env::var("SLOPMUD_LOCALE").unwrap_or_else(|_| "en".to_string());

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

    Config {
        bind,
        shard_addr,
        node_id,
        accounts_path,
        google_oauth_dir,
        google_auth_base_url,
        oidc_token_url,
        oidc_client_id,
        oidc_client_secret,
        oidc_scope,
        locale,
    }
}

#[derive(Debug, Clone)]
struct SessionInfo {
    name: String,
    is_bot: bool,
    auth: Option<Bytes>,
    race: String,
    class: String,
    sex: String,
    pronouns: String,
    write_tx: tokio::sync::mpsc::Sender<Bytes>,
}

#[derive(Debug, Clone)]
struct ShardMsg {
    t: u8,
    session: SessionId,
    body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnState {
    NeedName,
    NeedPasswordCreate,
    NeedPasswordLogin,
    NeedBotDisclosure,
    NeedPublicAck,
    NeedCocAck,
    NeedRace,
    NeedClass,
    NeedSex,
    NeedPronouns,
    InWorld,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct AccountRec {
    name: String,
    #[serde(default)]
    pw_hash: Option<String>,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    created_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoogleOAuthPending {
    code: String,
    verifier: String,
    status: String, // pending | ok | err
    created_unix: u64,
    #[serde(default)]
    updated_unix: Option<u64>,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug)]
struct Accounts {
    path: String,
    by_name: HashMap<String, AccountRec>,
}

impl Accounts {
    fn load(path: String) -> Self {
        let mut by_name = HashMap::new();
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Vec<AccountRec>>(&s) {
                for a in v {
                    by_name.insert(a.name.clone(), a);
                }
            }
        }
        Self { path, by_name }
    }

    fn save(&self) -> anyhow::Result<()> {
        let mut v = self.by_name.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        let s = serde_json::to_string_pretty(&v)?;
        let tmp = format!("{}.tmp", self.path);
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn new_session_id() -> SessionId {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("getrandom");
    SessionId::from_be_bytes(b)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,slopmud=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = Arc::new(parse_args());
    let listener = TcpListener::bind(cfg.bind).await?;

    let server_info = Arc::new(ServerInfo {
        started_instant: std::time::Instant::now(),
        started_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        shard_addr: cfg.shard_addr,
        bind: cfg.bind,
    });

    let sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let accounts: Arc<tokio::sync::Mutex<Accounts>> =
        Arc::new(tokio::sync::Mutex::new(Accounts::load(cfg.accounts_path.clone())));

    let (shard_tx, shard_rx) = tokio::sync::mpsc::channel::<ShardMsg>(4096);
    tokio::spawn(shard_manager_task(
        cfg.shard_addr,
        sessions.clone(),
        shard_rx,
    ));

    info!(
        bind = %cfg.bind,
        shard_addr = %cfg.shard_addr,
        node_id = %cfg.node_id.as_deref().unwrap_or("-"),
        "session broker listening"
    );

    loop {
        let (stream, peer) = listener.accept().await?;
        let sessions = sessions.clone();
        let shard_tx = shard_tx.clone();
        let server_info = server_info.clone();
        let cfg = cfg.clone();
        let accounts = accounts.clone();
        tokio::spawn(async move {
            if let Err(e) =
                handle_conn(stream, peer, sessions, shard_tx, server_info, cfg, accounts).await
            {
                warn!(peer = %peer, err = %e, "connection ended with error");
            }
        });
    }
}

async fn shard_manager_task(
    shard_addr: SocketAddr,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    mut rx: tokio::sync::mpsc::Receiver<ShardMsg>,
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

                // Re-attach all live sessions.
                let snapshot = {
                    let m = sessions.lock().await;
                    m.iter()
                        .map(|(sid, s)| {
                            (
                                *sid,
                                s.is_bot,
                                s.auth.clone(),
                                s.race.clone(),
                                s.class.clone(),
                                s.sex.clone(),
                                s.pronouns.clone(),
                                s.name.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                for (sid, is_bot, auth, race, class, sex, pronouns, name) in snapshot {
                    let body = attach_body(
                        is_bot,
                        auth.as_deref(),
                        &race,
                        &class,
                        &sex,
                        &pronouns,
                        name.as_bytes(),
                    );
                    let _ = write_req(&mut fw, REQ_ATTACH, sid, &body).await;
                }
                let _ = fw.flush().await;

                // Connection loop.
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            let Some(msg) = msg else {
                                return;
                            };
                            let _ = write_req(&mut fw, msg.t, msg.session, &msg.body).await;
                        }
                        res = fr.read_frame() => {
                            let frame = match res {
                                Ok(Some(f)) => f,
                                Ok(None) => break,
                                Err(_) => break,
                            };
                            match mudproto::shard::parse_resp(frame) {
                                Ok(resp) => route_resp(resp, &sessions).await,
                                Err(e) => {
                                    warn!(err=%e, "bad shard response");
                                }
                            }
                        }
                    }
                }

                // Shard connection dropped.
                warn!(shard_addr = %shard_addr, "shard disconnected; reconnecting");
                notify_all(&sessions, b"# shard disconnected; reconnecting...\r\n").await;
            }
            Err(e) => {
                if !announced_down {
                    announced_down = true;
                    warn!(shard_addr = %shard_addr, err=%e, "shard offline; retrying");
                    notify_all(&sessions, b"# shard offline; retrying...\r\n").await;
                }

                // Don't let the outbound queue grow unbounded while offline.
                while let Ok(msg) = rx.try_recv() {
                    if msg.t == REQ_INPUT {
                        notify_one(
                            &sessions,
                            msg.session,
                            b"# shard offline; input dropped\r\n",
                        )
                        .await;
                    }
                }

                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn route_resp(
    resp: ShardResp,
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
) {
    match resp {
        ShardResp::Output { session, line } => {
            let tx = {
                let m = sessions.lock().await;
                m.get(&session).map(|s| s.write_tx.clone())
            };
            if let Some(tx) = tx {
                let _ = tx.send(line).await;
            }
        }
        ShardResp::Err { session, msg } => {
            let tx = {
                let m = sessions.lock().await;
                m.get(&session).map(|s| s.write_tx.clone())
            };
            if let Some(tx) = tx {
                let _ = tx.send(msg).await;
            }
        }
    }
}

async fn notify_all(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    msg: &'static [u8],
) {
    let txs = {
        let m = sessions.lock().await;
        m.values().map(|s| s.write_tx.clone()).collect::<Vec<_>>()
    };
    for tx in txs {
        let _ = tx.send(Bytes::from_static(msg)).await;
    }
}

async fn notify_one(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    session: SessionId,
    msg: &'static [u8],
) {
    let tx = {
        let m = sessions.lock().await;
        m.get(&session).map(|s| s.write_tx.clone())
    };
    if let Some(tx) = tx {
        let _ = tx.send(Bytes::from_static(msg)).await;
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

fn attach_body(
    is_bot: bool,
    auth: Option<&[u8]>,
    race: &str,
    class: &str,
    sex: &str,
    pronouns: &str,
    name: &[u8],
) -> Bytes {
    let mut b = Vec::with_capacity(
        1 + 2
            + auth.map(|a| a.len()).unwrap_or(0)
            + 1
            + race.len()
            + 1
            + class.len()
            + 1
            + sex.len()
            + 1
            + pronouns.len()
            + name.len(),
    );
    let mut flags = 0u8;
    if is_bot {
        flags |= 0x01;
    }
    if auth.is_some() {
        flags |= 0x02;
    }
    flags |= 0x04; // build info always included by broker
    b.push(flags);
    if let Some(a) = auth {
        if a.len() <= u16::MAX as usize {
            let len = a.len() as u16;
            b.extend_from_slice(&len.to_be_bytes());
            b.extend_from_slice(a);
        } else {
            // Too long to encode; drop auth rather than truncating.
            warn!(len = a.len(), "auth token too long; dropping");
            b[0] &= !0x02;
        }
    }
    // race/class tokens (u8 length + bytes)
    let r = race.as_bytes();
    let c = class.as_bytes();
    let sx = sex.as_bytes();
    let pr = pronouns.as_bytes();
    b.push(r.len().min(255) as u8);
    b.extend_from_slice(&r[..r.len().min(255)]);
    b.push(c.len().min(255) as u8);
    b.extend_from_slice(&c[..c.len().min(255)]);
    b.push(sx.len().min(255) as u8);
    b.extend_from_slice(&sx[..sx.len().min(255)]);
    b.push(pr.len().min(255) as u8);
    b.extend_from_slice(&pr[..pr.len().min(255)]);
    b.extend_from_slice(name);
    Bytes::from(b)
}

#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    access_token: String,
    token_type: Option<String>,
    expires_in: Option<u64>,
}

fn hex_lower(b: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(LUT[(x >> 4) as usize] as char);
        s.push(LUT[(x & 0x0f) as usize] as char);
    }
    s
}

async fn mint_internal_oidc_token(cfg: &Config, session: SessionId, sub: &str) -> anyhow::Result<Option<Bytes>> {
    let Some(url) = cfg.oidc_token_url.as_deref() else {
        return Ok(None);
    };
    let client_id = cfg
        .oidc_client_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("SLOPMUD_OIDC_TOKEN_URL set but missing SLOPMUD_OIDC_CLIENT_ID"))?;
    let client_secret = cfg
        .oidc_client_secret
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("SLOPMUD_OIDC_TOKEN_URL set but missing SLOPMUD_OIDC_CLIENT_SECRET"))?;
    let scope = cfg.oidc_scope.as_deref().unwrap_or("slopmud:session");

    let sid_hex = hex_lower(&session.to_be_bytes());
    let http = reqwest::Client::new();
    let resp = http
        .post(url)
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "client_credentials"),
            ("sub", sub),
            ("sid", sid_hex.as_str()),
            ("scope", scope),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("oidc token endpoint returned {}", resp.status()));
    }
    let t: OidcTokenResponse = resp.json().await?;
    Ok(Some(Bytes::from(t.access_token)))
}

async fn handle_conn(
    stream: TcpStream,
    peer: SocketAddr,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    shard_tx: tokio::sync::mpsc::Sender<ShardMsg>,
    server_info: Arc<ServerInfo>,
    cfg: Arc<Config>,
    accounts: Arc<tokio::sync::Mutex<Accounts>>,
) -> anyhow::Result<()> {
    let session = new_session_id();
    let (mut rd, mut wr) = stream.into_split();

    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<Bytes>(128);
    let writer = tokio::spawn(async move {
        while let Some(b) = write_rx.recv().await {
            if wr.write_all(&b[..]).await.is_err() {
                break;
            }
        }
    });

    let mut iac = IacParser::new();
    let mut linebuf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut name: Option<String> = None;
    let mut is_bot: Option<bool> = None;
    let mut auth_token: Option<Bytes> = None;
    let mut race: Option<String> = None;
    let mut class: Option<String> = None;
    let mut sex: Option<String> = None;
    let mut pronouns: Option<String> = None;
    let mut password_echo_disabled = false;
    let mut state = ConnState::NeedName;

    write_tx
        .send(Bytes::from_static(
            b"slopmud (alpha)\r\ncharacter creation (step 1/4)\r\nname: ",
        ))
        .await
        .ok();

    let mut buf = [0u8; 4096];
    'read: loop {
        let n = rd.read(&mut buf).await?;
        if n == 0 {
            break;
        }

        let (data, replies) = iac.parse(&buf[..n]);
        if !replies.is_empty() {
            let _ = write_tx.send(Bytes::from(replies)).await;
        }
        if data.is_empty() {
            continue;
        }

        linebuf.extend_from_slice(&data);
        while let Some(mut line_bytes) = try_pop_line(&mut linebuf) {
            if line_bytes.is_empty() {
                continue;
            }

            match state {
                ConnState::NeedName => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let n = sanitize_name(&line);
                    if n.is_empty() {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"bad name (use letters/numbers/_/-, max 20)\r\nname: ",
                            ))
                            .await;
                        continue;
                    }

                    name = Some(n);
                    let exists = {
                        let a = accounts.lock().await;
                        a.by_name.contains_key(name.as_ref().expect("name set"))
                    };
                    // Disable local echo for password entry (best-effort via telnet negotiation).
                    password_echo_disabled = true;
                    let mut b = Vec::new();
                    b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                    if exists {
                        state = ConnState::NeedPasswordLogin;
                        b.extend_from_slice(b"\r\npassword (never logged/echoed): ");
                    } else {
                        state = ConnState::NeedPasswordCreate;
                        b.extend_from_slice(b"\r\nset password (never logged/echoed; min 8 chars): ");
                    }
                    let _ = write_tx.send(Bytes::from(b)).await;
                    continue;
                }
                ConnState::NeedPasswordCreate => {
                    // Never convert to String (avoid extra copies) and never log.
                    // Trim ASCII whitespace at the ends.
                    let pw = trim_ascii_ws(&line_bytes);

                    let ok = pw.len() >= 8;

                    // Wipe the user-provided password bytes ASAP.
                    // Note: we need pw for hashing below, so we wipe later on success/failure branches.

                    // Re-enable echo (best-effort).
                    if password_echo_disabled {
                        let _ = write_tx
                            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                            .await;
                        password_echo_disabled = false;
                    }

                    if !ok {
                        // Re-disable echo for retry.
                        password_echo_disabled = true;
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\npassword too short\r\n");
                        b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        b.extend_from_slice(b"set password (min 8 chars): ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    let _ = write_tx.send(Bytes::from_static(b"\r\n")).await;

                    // Store only a salted hash, never the raw password.
                    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
                    let hash = Argon2::default()
                        .hash_password(pw, &salt)
                        .map_err(|e| anyhow::anyhow!("hash_password failed: {e}"))?
                        .to_string();

                    let now_unix = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    {
                        let mut a = accounts.lock().await;
                        let uname = name.as_ref().expect("name set").clone();
                        a.by_name.insert(
                            uname.clone(),
                            AccountRec {
                                name: uname,
                                pw_hash: Some(hash),
                                google_sub: None,
                                google_email: None,
                                created_unix: now_unix,
                            },
                        );
                        a.save()?;
                    }

                    // If configured, mint a session token now (password never leaves this process).
                    if auth_token.is_none() {
                        match mint_internal_oidc_token(cfg.as_ref(), session, name.as_deref().unwrap_or("")).await {
                            Ok(t) => auth_token = t,
                            Err(_) => {
                                let _ = write_tx
                                    .send(Bytes::from_static(
                                        b"\r\nauth service unavailable\r\nbye\r\n",
                                    ))
                                    .await;
                                line_bytes.zeroize();
                                break 'read;
                            }
                        }
                    }

                    line_bytes.zeroize();
                    state = ConnState::NeedBotDisclosure;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedPasswordLogin => {
                    let pw = trim_ascii_ws(&line_bytes);
                    let uname = name.as_ref().expect("name set").clone();
                    let rec = {
                        let a = accounts.lock().await;
                        a.by_name.get(&uname).cloned()
                    };
                    if rec.as_ref().and_then(|r| r.pw_hash.as_deref()).is_none() {
                        // Re-enable echo (best-effort).
                        if password_echo_disabled {
                            let _ = write_tx
                                .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                                .await;
                            password_echo_disabled = false;
                        }
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"\r\naccount has no password set\r\nbye\r\n",
                            ))
                            .await;
                        line_bytes.zeroize();
                        break 'read;
                    }

                    let hash = rec.and_then(|r| r.pw_hash);
                    let ok = if let Some(hash) = hash {
                        if let Ok(ph) = PasswordHash::new(&hash) {
                            Argon2::default().verify_password(pw, &ph).is_ok()
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    // Re-enable echo (best-effort).
                    if password_echo_disabled {
                        let _ = write_tx
                            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                            .await;
                        password_echo_disabled = false;
                    }

                    if !ok {
                        password_echo_disabled = true;
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\nbad password\r\n");
                        b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        b.extend_from_slice(b"password: ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    let _ = write_tx.send(Bytes::from_static(b"\r\n")).await;

                    // If configured, mint a session token now (password never leaves this process).
                    if auth_token.is_none() {
                        match mint_internal_oidc_token(cfg.as_ref(), session, name.as_deref().unwrap_or("")).await {
                            Ok(t) => auth_token = t,
                            Err(_) => {
                                let _ = write_tx
                                    .send(Bytes::from_static(
                                        b"\r\nauth service unavailable\r\nbye\r\n",
                                    ))
                                    .await;
                                line_bytes.zeroize();
                                break 'read;
                            }
                        }
                    }

                    line_bytes.zeroize();
                    state = ConnState::NeedBotDisclosure;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedBotDisclosure => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    let b = match v.as_str() {
                        "human" => false,
                        "bot" => true,
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(b"please type: human | bot\r\n> "))
                                .await;
                            continue;
                        }
                    };
                    is_bot = Some(b);
                    state = ConnState::NeedPublicAck;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 3/4)\r\ncontent + licensing:\r\n- anything you submit - consider it publicly licensed and publicly published\r\n- zero privacy: logs may be shared and used for training\r\n- exception: passwords are never logged/echoed; only password hashes are stored\r\ntype: agree\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedPublicAck => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    if v != "agree" {
                        let _ = write_tx
                            .send(Bytes::from_static(b"type: agree\r\n> "))
                            .await;
                        continue;
                    }
                    state = ConnState::NeedCocAck;
                    let mut b = Vec::new();
                    b.extend_from_slice(b"character creation (step 4/4)\r\ncode of conduct:\r\n");
                    for li in COC_LINE_ITEMS {
                        b.extend_from_slice(li.as_bytes());
                        b.extend_from_slice(b"\r\n");
                    }
                    b.extend_from_slice(b"type: agree\r\n> ");
                    let _ = write_tx.send(Bytes::from(b)).await;
                    continue;
                }
                ConnState::NeedCocAck => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    if v != "agree" {
                        let _ = write_tx
                            .send(Bytes::from_static(b"type: agree\r\n> "))
                            .await;
                        continue;
                    }
                    state = ConnState::NeedRace;
                    let mut s = String::new();
                    s.push_str("character creation (step 5/7)\r\nchoose race:\r\n");
                    s.push_str("type: race list | race <name>\r\n> ");
                    let _ = write_tx.send(Bytes::from(s)).await;
                    continue;
                }
                ConnState::NeedRace => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "race list" || line == "list" {
                        let mut s = String::new();
                        s.push_str("races:\r\n");
                        for r in RACE_TOKENS {
                            s.push_str(" - ");
                            s.push_str(r);
                            s.push_str("\r\n");
                        }
                        s.push_str("> ");
                        let _ = write_tx.send(Bytes::from(s)).await;
                        continue;
                    }
                    let token = line.strip_prefix("race ").unwrap_or(line.as_str()).trim();
                    if !is_allowed_token(token, &RACE_TOKENS) {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (try: race list | race human)\r\n> ",
                            ))
                            .await;
                        continue;
                    }
                    race = Some(token.to_string());
                    state = ConnState::NeedClass;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 6/7)\r\nchoose class:\r\ntype: class list | class <name>\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedClass => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "class list" || line == "list" {
                        let mut s = String::new();
                        s.push_str("classes:\r\n");
                        for c in CLASS_TOKENS {
                            s.push_str(" - ");
                            s.push_str(c);
                            s.push_str("\r\n");
                        }
                        s.push_str("> ");
                        let _ = write_tx.send(Bytes::from(s)).await;
                        continue;
                    }
                    let token = line.strip_prefix("class ").unwrap_or(line.as_str()).trim();
                    if !is_allowed_token(token, &CLASS_TOKENS) {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (try: class list | class fighter)\r\n> ",
                            ))
                            .await;
                        continue;
                    }
                    class = Some(token.to_string());
                    state = ConnState::NeedSex;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 7/7)\r\nsex:\r\ntype: male | female | none | other\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedSex => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    match line.as_str() {
                        "male" => {
                            sex = Some("male".to_string());
                            pronouns = Some("he".to_string());
                        }
                        "female" => {
                            sex = Some("female".to_string());
                            pronouns = Some("she".to_string());
                        }
                        "none" => {
                            sex = Some("none".to_string());
                            pronouns = Some("they".to_string());
                        }
                        "other" => {
                            sex = Some("other".to_string());
                            state = ConnState::NeedPronouns;
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"pronouns (en): he | she | they\r\n(type: he)\r\n> ",
                                ))
                                .await;
                            continue;
                        }
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"please type: male | female | none | other\r\n> ",
                                ))
                                .await;
                            continue;
                        }
                    }
                    // Ready to attach.
                }
                ConnState::NeedPronouns => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    let Some(key) = normalize_pronouns(&cfg.locale, &line) else {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (pronouns: he | she | they)\r\n> ",
                            ))
                            .await;
                        continue;
                    };
                    pronouns = Some(key.to_string());
                    // Ready to attach.
                }
                ConnState::InWorld => {
                    // In-world input. Some commands are handled here (connection-level).
                }
            }

            // If we just finished sex/pronouns, attach now.
            if matches!(state, ConnState::NeedSex | ConnState::NeedPronouns)
                && sex.is_some()
                && pronouns.is_some()
            {
                state = ConnState::InWorld;

                let n = name.as_ref().expect("name set").clone();
                let bot = is_bot.unwrap_or(false);
                let race_s = race.clone().unwrap_or_else(|| "human".to_string());
                let class_s = class.clone().unwrap_or_else(|| "fighter".to_string());
                let sex_s = sex.clone().unwrap_or_else(|| "none".to_string());
                let pro_s = pronouns.clone().unwrap_or_else(|| "they".to_string());

                {
                    let mut m = sessions.lock().await;
                    m.insert(
                        session,
                        SessionInfo {
                            name: n.clone(),
                            is_bot: bot,
                            auth: auth_token.clone(),
                            race: race_s.clone(),
                            class: class_s.clone(),
                            sex: sex_s.clone(),
                            pronouns: pro_s.clone(),
                            write_tx: write_tx.clone(),
                        },
                    );
                }

                let body = attach_body(
                    bot,
                    auth_token.as_deref(),
                    &race_s,
                    &class_s,
                    &sex_s,
                    &pro_s,
                    n.as_bytes(),
                );
                let _ = shard_tx
                    .send(ShardMsg {
                        t: REQ_ATTACH,
                        session,
                        body,
                    })
                    .await;
                continue;
            }

            // In-world command handling at the broker level.
            let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let lc = line.to_ascii_lowercase();
            if lc == "exit" || lc == "quit" {
                let _ = write_tx.send(Bytes::from_static(b"bye\r\n")).await;
                break 'read;
            }

            if lc == "uptime" || lc == "uptime broker" || lc == "uptime session" {
                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let up_s = server_info.started_instant.elapsed().as_secs();

                let mut s = String::new();
                s.push_str("uptime:\r\n");
                s.push_str(&format!(" - broker_wall_unix: {now_unix}\r\n"));
                s.push_str(&format!(" - broker_started_unix: {}\r\n", server_info.started_unix));
                s.push_str(&format!(" - broker_uptime_s: {up_s}\r\n"));
                s.push_str(&format!(" - broker_bind: {}\r\n", server_info.bind));
                s.push_str(&format!(" - shard_addr: {}\r\n", server_info.shard_addr));
                s.push_str(" - note: shard uptime/time via `uptime` (forwarded to shard)\r\n");
                let _ = write_tx.send(Bytes::from(s)).await;

                if lc != "uptime" {
                    continue;
                }
                // `uptime` (no args) also forwards to shard so the user can see shard wall time + world time.
            }

            let _ = shard_tx
                .send(ShardMsg {
                    t: REQ_INPUT,
                    session,
                    body: Bytes::from(line.into_bytes()),
                })
                .await;
        }
    }

    // Best-effort: if we disconnected mid-password, restore echo.
    if password_echo_disabled {
        let _ = write_tx
            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
            .await;
    }

    // Disconnect cleanup.
    let existed = { sessions.lock().await.remove(&session).is_some() };
    if existed {
        let _ = shard_tx
            .send(ShardMsg {
                t: REQ_DETACH,
                session,
                body: Bytes::new(),
            })
            .await;
    } else {
        info!(peer=%peer, "disconnected before entering world");
    }

    drop(write_tx);
    let _ = writer.await;
    Ok(())
}

const TELNET_IAC: u8 = 255;
const TELNET_WILL: u8 = 251;
const TELNET_WONT: u8 = 252;
const TELNET_OPT_ECHO: u8 = 1;

fn telnet_will(opt: u8) -> [u8; 3] {
    [TELNET_IAC, TELNET_WILL, opt]
}

fn telnet_wont(opt: u8) -> [u8; 3] {
    [TELNET_IAC, TELNET_WONT, opt]
}

fn trim_ascii_ws(s: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = s.len();
    while a < b && s[a].is_ascii_whitespace() {
        a += 1;
    }
    while b > a && s[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    &s[a..b]
}

fn sanitize_name(s: &str) -> String {
    let s = s.trim();
    let mut out = String::new();
    for c in s.chars() {
        if out.len() >= 20 {
            break;
        }
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        }
    }
    out
}

fn try_pop_line(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    // Telnet tends to send CRLF, but can also send CRNUL.
    // Treat `\n` and `\r` as EOL; if `\r` is followed by `\n` or `\0`, consume both.
    let i_nl = memchr(b'\n', buf.as_slice());
    let i_cr = memchr(b'\r', buf.as_slice());

    let i = match (i_nl, i_cr) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }?;

    let mut line = buf.drain(0..i).collect::<Vec<u8>>();
    // Drain EOL bytes.
    if !buf.is_empty() {
        let first = buf.remove(0);
        if first == b'\r' && !buf.is_empty() && (buf[0] == b'\n' || buf[0] == 0) {
            buf.remove(0);
        }
    }

    // Trim any stray trailing \r (if we matched \n first and had \r\n).
    while line.last() == Some(&b'\r') {
        line.pop();
    }

    Some(line)
}

#[cfg(test)]
mod tests {
    use super::trim_ascii_ws;

    #[test]
    fn trim_ascii_ws_basic() {
        assert_eq!(trim_ascii_ws(b""), b"");
        assert_eq!(trim_ascii_ws(b"  x "), b"x");
        assert_eq!(trim_ascii_ws(b"\r\nx\t"), b"x");
        assert_eq!(trim_ascii_ws(b"   "), b"");
    }
}
