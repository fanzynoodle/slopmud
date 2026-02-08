use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use sbc_core::{
    AdminReq, AdminResp, BanApplyResult, BanEntry, EnforcementStatus, Event, EventEnvelope,
    EventsReq, IpPrefix, LegalHoldEntry, SubscribeMode,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, broadcast};
use tracing::{Level, info, warn};

fn usage_and_exit() -> ! {
    eprintln!(
        "sbc_raftd

USAGE:
  sbc_raftd [--admin-sock PATH] [--events-sock PATH] [--state-path PATH]

ENV:
  SBC_ADMIN_SOCK   default /run/slopmud/sbc-admin.sock
  SBC_EVENTS_SOCK  default /run/slopmud/sbc-events.sock
  SBC_STATE_PATH   default sbc-state.json (in cwd)
"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    admin_sock: PathBuf,
    events_sock: PathBuf,
    state_path: PathBuf,
}

fn parse_args() -> Config {
    let mut admin_sock: PathBuf = std::env::var("SBC_ADMIN_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-admin.sock".to_string())
        .into();
    let mut events_sock: PathBuf = std::env::var("SBC_EVENTS_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-events.sock".to_string())
        .into();
    let mut state_path: PathBuf = std::env::var("SBC_STATE_PATH")
        .unwrap_or_else(|_| "sbc-state.json".to_string())
        .into();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--admin-sock" => {
                admin_sock = it.next().unwrap_or_else(|| usage_and_exit()).into();
            }
            "--events-sock" => {
                events_sock = it.next().unwrap_or_else(|| usage_and_exit()).into();
            }
            "--state-path" => {
                state_path = it.next().unwrap_or_else(|| usage_and_exit()).into();
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        admin_sock,
        events_sock,
        state_path,
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PersistedState {
    next_index: u64,
    bans: HashMap<String, BanEntry>,     // ban_id -> entry
    ban_by_key: HashMap<String, String>, // cidr -> ban_id
    #[serde(default)]
    holds: HashMap<String, LegalHoldEntry>, // name_lc -> entry
    #[serde(default)]
    last_status_by_node: HashMap<String, EnforcementStatus>,
    #[serde(default)]
    last_apply_by_node_ban: HashMap<String, BanApplyResult>,
}

impl PersistedState {
    fn empty() -> Self {
        Self {
            next_index: 1,
            bans: HashMap::new(),
            ban_by_key: HashMap::new(),
            holds: HashMap::new(),
            last_status_by_node: HashMap::new(),
            last_apply_by_node_ban: HashMap::new(),
        }
    }

    fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(e.into()),
        }
    }
}

fn atomic_write_json(path: &Path, value: &PersistedState) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, s)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn rand_hex_16() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("getrandom");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 32];
    for (i, x) in b.iter().enumerate() {
        out[i * 2] = HEX[(x >> 4) as usize];
        out[i * 2 + 1] = HEX[(x & 0x0f) as usize];
    }
    String::from_utf8_lossy(&out).to_string()
}

fn ensure_unix_socket_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn handle_admin_conn(
    st: Arc<Mutex<PersistedState>>,
    tx: broadcast::Sender<EventEnvelope>,
    mut conn: UnixStream,
) -> anyhow::Result<()> {
    let (rd, mut wr) = conn.split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let req: AdminReq = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            let resp = AdminResp::Err {
                message: format!("bad json: {e}"),
            };
            wr.write_all(serde_json::to_string(&resp)?.as_bytes())
                .await?;
            wr.write_all(b"\n").await?;
            return Ok(());
        }
    };

    let (resp, event_opt) = {
        let mut s = st.lock().await;
        match req {
            AdminReq::UpsertBan {
                key,
                ttl_s,
                created_by,
                reason,
            } => {
                let key_pfx = IpPrefix::parse_cidr(&key)?;
                let key_cidr = key_pfx.to_cidr_string();
                let now = now_unix();
                // ttl_s=0 means "never expires" (until explicitly deleted).
                let expires_at_unix = if ttl_s == 0 {
                    0
                } else {
                    now.saturating_add(ttl_s)
                };

                let ban_id = if let Some(existing) = s.ban_by_key.get(&key_cidr) {
                    existing.clone()
                } else {
                    let id = rand_hex_16();
                    s.ban_by_key.insert(key_cidr.clone(), id.clone());
                    id
                };

                let entry = BanEntry {
                    ban_id: ban_id.clone(),
                    key: key_pfx,
                    created_at_unix: now,
                    created_by,
                    reason,
                    expires_at_unix,
                };
                s.bans.insert(ban_id.clone(), entry.clone());

                let idx = s.next_index;
                s.next_index = s.next_index.saturating_add(1);
                let ev = EventEnvelope {
                    index: idx,
                    event: Event::BanUpserted {
                        entry: entry.clone(),
                    },
                };
                let resp = AdminResp::OkBan { index: idx, entry };
                (resp, Some(ev))
            }
            AdminReq::DeleteBan { ban_id } => {
                if let Some(entry) = s.bans.remove(&ban_id) {
                    s.ban_by_key.remove(&entry.key.to_cidr_string());
                }
                let idx = s.next_index;
                s.next_index = s.next_index.saturating_add(1);
                let ev = EventEnvelope {
                    index: idx,
                    event: Event::BanDeleted {
                        ban_id: ban_id.clone(),
                    },
                };
                let resp = AdminResp::Ok { index: idx };
                (resp, Some(ev))
            }
            AdminReq::UpsertLegalHold {
                name,
                created_by,
                reason,
            } => {
                let name_lc = name.trim().to_ascii_lowercase();
                if name_lc.is_empty() {
                    let resp = AdminResp::Err {
                        message: "missing name".to_string(),
                    };
                    (resp, None)
                } else {
                    let entry = LegalHoldEntry {
                        name_lc: name_lc.clone(),
                        created_at_unix: now_unix(),
                        created_by,
                        reason,
                    };
                    s.holds.insert(name_lc.clone(), entry.clone());

                    let idx = s.next_index;
                    s.next_index = s.next_index.saturating_add(1);
                    let ev = EventEnvelope {
                        index: idx,
                        event: Event::LegalHoldUpserted {
                            entry: entry.clone(),
                        },
                    };
                    let resp = AdminResp::OkLegalHold { index: idx, entry };
                    (resp, Some(ev))
                }
            }
            AdminReq::DeleteLegalHold { name } => {
                let name_lc = name.trim().to_ascii_lowercase();
                if name_lc.is_empty() {
                    let resp = AdminResp::Err {
                        message: "missing name".to_string(),
                    };
                    (resp, None)
                } else {
                    s.holds.remove(&name_lc);
                    let idx = s.next_index;
                    s.next_index = s.next_index.saturating_add(1);
                    let ev = EventEnvelope {
                        index: idx,
                        event: Event::LegalHoldDeleted { name_lc },
                    };
                    let resp = AdminResp::Ok { index: idx };
                    (resp, Some(ev))
                }
            }
            AdminReq::ReportEnforcementStatus { status } => {
                s.last_status_by_node
                    .insert(status.node_id.clone(), status.clone());
                let idx = s.next_index;
                s.next_index = s.next_index.saturating_add(1);
                let ev = EventEnvelope {
                    index: idx,
                    event: Event::EnforcementStatus { status },
                };
                let resp = AdminResp::Ok { index: idx };
                (resp, Some(ev))
            }
            AdminReq::ReportBanApplyResult { report } => {
                let k = format!("{}:{}:{}", report.node_id, report.ban_id, report.op);
                s.last_apply_by_node_ban.insert(k, report.clone());
                let idx = s.next_index;
                s.next_index = s.next_index.saturating_add(1);
                let ev = EventEnvelope {
                    index: idx,
                    event: Event::BanApplyResult { report },
                };
                let resp = AdminResp::Ok { index: idx };
                (resp, Some(ev))
            }
            AdminReq::GetState => {
                let idx = s.next_index.saturating_sub(1);
                let bans = s.bans.values().cloned().collect::<Vec<_>>();
                let holds = s.holds.values().cloned().collect::<Vec<_>>();
                let resp = AdminResp::OkState { index: idx, bans, holds };
                (resp, None)
            }
        }
    };

    // Persist + broadcast outside the lock.
    if let Some(ev) = event_opt.clone() {
        {
            let s = st.lock().await;
            let path = {
                // We only store this in-memory; the caller holds state_path.
                // The actual write happens in the main task to keep this handler generic.
                // (We still persist from here by re-reading env if needed.)
                // For now, do nothing.
                drop(s);
                None::<PathBuf>
            };
            drop(path);
        }

        // Broadcast best-effort.
        let _ = tx.send(ev);
    }

    // Persist state after the mutation (best-effort).
    if matches!(
        &resp,
        AdminResp::Ok { .. }
            | AdminResp::OkBan { .. }
            | AdminResp::OkState { .. }
            | AdminResp::Err { .. }
    ) {
        // no-op: state persistence handled by main loop via a background task.
    }

    wr.write_all(serde_json::to_string(&resp)?.as_bytes())
        .await?;
    wr.write_all(b"\n").await?;
    Ok(())
}

async fn handle_events_conn(
    st: Arc<Mutex<PersistedState>>,
    tx: broadcast::Sender<EventEnvelope>,
    mut conn: UnixStream,
) -> anyhow::Result<()> {
    let (rd, mut wr) = conn.split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let req: EventsReq = serde_json::from_str(line)?;
    let EventsReq::Subscribe { mode } = req;

    if mode == SubscribeMode::Snapshot {
        let (idx, bans, holds) = {
            let s = st.lock().await;
            (
                s.next_index.saturating_sub(1),
                s.bans.values().cloned().collect::<Vec<_>>(),
                s.holds.values().cloned().collect::<Vec<_>>(),
            )
        };
        let snap = EventEnvelope {
            index: idx,
            event: Event::Snapshot { bans, holds },
        };
        wr.write_all(serde_json::to_string(&snap)?.as_bytes())
            .await?;
        wr.write_all(b"\n").await?;
    }

    let mut rx = tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(ev) => {
                wr.write_all(serde_json::to_string(&ev)?.as_bytes()).await?;
                wr.write_all(b"\n").await?;
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(dropped=%n, "events subscriber lagged");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();

    // Load persisted state.
    let state_path = cfg.state_path.clone();
    let st0 = PersistedState::load(&state_path).with_context(|| {
        format!(
            "failed to load state from {}",
            state_path.as_os_str().to_string_lossy()
        )
    })?;

    let st = Arc::new(Mutex::new(st0));

    ensure_unix_socket_dir(&cfg.admin_sock)?;
    ensure_unix_socket_dir(&cfg.events_sock)?;
    remove_if_exists(&cfg.admin_sock)?;
    remove_if_exists(&cfg.events_sock)?;

    let admin_listener = UnixListener::bind(&cfg.admin_sock)
        .with_context(|| format!("bind admin sock {:?}", cfg.admin_sock))?;
    let events_listener = UnixListener::bind(&cfg.events_sock)
        .with_context(|| format!("bind events sock {:?}", cfg.events_sock))?;

    // Event bus.
    let (ev_tx, _ev_rx) = broadcast::channel::<EventEnvelope>(1024);

    info!(
        admin_sock = %cfg.admin_sock.display(),
        events_sock = %cfg.events_sock.display(),
        state_path = %cfg.state_path.display(),
        "sbc_raftd listening"
    );

    // Background persister: periodically flush state to disk when index advances.
    {
        let st = st.clone();
        let state_path = state_path.clone();
        tokio::spawn(async move {
            let mut last_persisted = 0u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let snapshot = { st.lock().await.clone() };
                let idx = snapshot.next_index;
                if idx == last_persisted {
                    continue;
                }
                if let Err(e) = tokio::task::spawn_blocking({
                    let state_path = state_path.clone();
                    move || atomic_write_json(&state_path, &snapshot)
                })
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("persist task join error: {e}")))
                {
                    warn!(err=%e, "failed to persist state");
                    continue;
                }
                last_persisted = idx;
            }
        });
    }

    // TTL reaper: remove expired bans and emit BanDeleted events.
    {
        let st = st.clone();
        let tx = ev_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let now = now_unix();
                let expired_events = {
                    let mut s = st.lock().await;
                    let mut expired_ids = Vec::new();
                    for (ban_id, entry) in s.bans.iter() {
                        if entry.expires_at_unix != 0 && entry.expires_at_unix <= now {
                            expired_ids.push(ban_id.clone());
                        }
                    }

                    if expired_ids.is_empty() {
                        Vec::new()
                    } else {
                        let mut evs = Vec::new();
                        for ban_id in expired_ids {
                            if let Some(entry) = s.bans.remove(&ban_id) {
                                s.ban_by_key.remove(&entry.key.to_cidr_string());
                            }
                            let idx = s.next_index;
                            s.next_index = s.next_index.saturating_add(1);
                            evs.push(EventEnvelope {
                                index: idx,
                                event: Event::BanDeleted { ban_id },
                            });
                        }
                        evs
                    }
                };

                for ev in expired_events {
                    let _ = tx.send(ev);
                }
            }
        });
    }

    // Serve admin + events in parallel.
    let st_admin = st.clone();
    let tx_admin = ev_tx.clone();
    let admin_task = tokio::spawn(async move {
        loop {
            match admin_listener.accept().await {
                Ok((conn, _addr)) => {
                    let st = st_admin.clone();
                    let tx = tx_admin.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_admin_conn(st, tx, conn).await {
                            warn!(err=%e, "admin conn error");
                        }
                    });
                }
                Err(e) => {
                    warn!(err=%e, "admin accept error");
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
        }
    });

    let st_events = st.clone();
    let tx_events = ev_tx.clone();
    let events_task = tokio::spawn(async move {
        loop {
            match events_listener.accept().await {
                Ok((conn, _addr)) => {
                    let st = st_events.clone();
                    let tx = tx_events.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_events_conn(st, tx, conn).await {
                            warn!(err=%e, "events conn error");
                        }
                    });
                }
                Err(e) => {
                    warn!(err=%e, "events accept error");
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
        }
    });

    let _ = tokio::join!(admin_task, events_task);
    Ok(())
}
