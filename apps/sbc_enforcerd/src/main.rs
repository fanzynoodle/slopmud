use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::{Json, Router, routing::get};
use sbc_core::{
    AdminReq, AdminResp, BanApplyResult, BanEntry, EnforcementStatus, Event, EventEnvelope,
    EventsReq, ExemptPrefixes, SubscribeMode,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, mpsc};
use tracing::{Level, info, warn};

fn usage_and_exit() -> ! {
    eprintln!(
        "sbc_enforcerd

USAGE:
  sbc_enforcerd [--status-http HOST:PORT]
  sbc_enforcerd --detach-xdp --iface IFACE

ENV:
  SBC_NODE_ID                  default $HOSTNAME or unknown
  SBC_ADMIN_SOCK               default /run/slopmud/sbc-admin.sock
  SBC_EVENTS_SOCK              default /run/slopmud/sbc-events.sock
  SBC_STATUS_HTTP              default 127.0.0.1:9911
  SBC_ENABLE_DNS_NAME          default empty (enforcement disabled)
  SBC_ENABLE_DNS_IP            default empty (presence check); when set, enforcement enabled only if DNS resolves to this IP
  SBC_ENABLE_DNS_INTERVAL_S    default 60
  SBC_APPLY_SNAPSHOT           default 0 (tail-only; non-persistent)
  SBC_EXEMPT_PREFIXES_PATH     default empty (no exemptions)
"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    node_id: String,
    admin_sock: PathBuf,
    events_sock: PathBuf,
    status_http: SocketAddr,
    enable_dns_name: String,
    enable_dns_ip: Option<std::net::IpAddr>,
    enable_dns_interval_s: u64,
    apply_snapshot: bool,
    exempt_prefixes_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Run,
    DetachXdp,
}

fn parse_args() -> anyhow::Result<(Config, Mode)> {
    let node_id = std::env::var("SBC_NODE_ID")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    let admin_sock: PathBuf = std::env::var("SBC_ADMIN_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-admin.sock".to_string())
        .into();
    let events_sock: PathBuf = std::env::var("SBC_EVENTS_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-events.sock".to_string())
        .into();
    let mut status_http: SocketAddr = std::env::var("SBC_STATUS_HTTP")
        .unwrap_or_else(|_| "127.0.0.1:9911".to_string())
        .parse()
        .map_err(|_| anyhow::anyhow!("bad SBC_STATUS_HTTP"))?;

    let enable_dns_name = std::env::var("SBC_ENABLE_DNS_NAME").unwrap_or_default();
    let enable_dns_ip: Option<std::net::IpAddr> = match std::env::var("SBC_ENABLE_DNS_IP").ok() {
        Some(v) if !v.trim().is_empty() => Some(
            v.trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("bad SBC_ENABLE_DNS_IP"))?,
        ),
        _ => None,
    };
    let enable_dns_interval_s: u64 = std::env::var("SBC_ENABLE_DNS_INTERVAL_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let apply_snapshot = std::env::var("SBC_APPLY_SNAPSHOT")
        .ok()
        .is_some_and(|v| v == "1");
    let exempt_prefixes_path = std::env::var("SBC_EXEMPT_PREFIXES_PATH")
        .ok()
        .map(Into::into);

    let mut mode = Mode::Run;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--status-http" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                status_http = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--detach-xdp" => mode = Mode::DetachXdp,
            "--iface" => {
                let _iface = it.next().unwrap_or_else(|| usage_and_exit());
                // currently unused; kept for forward compatibility
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Ok((
        Config {
            node_id,
            admin_sock,
            events_sock,
            status_http,
            enable_dns_name,
            enable_dns_ip,
            enable_dns_interval_s,
            apply_snapshot,
            exempt_prefixes_path,
        },
        mode,
    ))
}

#[derive(Clone, Debug)]
struct SharedState {
    node_id: String,
    dns_name: String,
    dns_expect_ip: Option<String>,
    apply_snapshot: bool,

    dns_enabled: bool,
    dns_last_checked_unix: u64,
    dns_last_error: Option<String>,
    dns_last_ips: Vec<String>,

    enforcement_mode: String,   // enforcing | fail_open
    enforcement_reason: String, // dns_enabled | dns_disabled_or_error | startup

    backend: String,
    backend_attached: bool,

    events_connected: bool,
    events_last_index: u64,
    events_last_error: Option<String>,

    // Desired bans from raft (regardless of enforcement mode).
    desired_bans: HashMap<String, BanEntry>,

    // No-op backend tracks "applied" bans while enforcement is enabled.
    applied_bans: HashMap<String, BanEntry>,

    exempt: ExemptPrefixes,
    exempt_loaded: bool,
    exempt_path: Option<String>,
    exempt_last_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct StatusView {
    node_id: String,
    dns: DnsView,
    enforcement: EnforcementView,
    events: EventsView,
    apply_snapshot: bool,
    exempt_prefixes: ExemptView,
    applied_bans: AppliedBansView,
}

#[derive(Clone, Debug, serde::Serialize)]
struct DnsView {
    name: String,
    expect_ip: Option<String>,
    enabled: bool,
    last_checked_unix: u64,
    last_error: Option<String>,
    last_ips: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct EnforcementView {
    mode: String,
    reason: String,
    backend: String,
    backend_attached: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
struct EventsView {
    connected: bool,
    last_index: u64,
    last_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct ExemptView {
    loaded: bool,
    path: Option<String>,
    count: usize,
    last_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct AppliedBansView {
    count: usize,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn dns_check_value(
    name: &str,
    expect: Option<std::net::IpAddr>,
) -> (bool, Option<String>, Vec<std::net::IpAddr>) {
    if name.trim().is_empty() {
        return (
            false,
            Some("SBC_ENABLE_DNS_NAME not set".to_string()),
            Vec::new(),
        );
    }
    match tokio::net::lookup_host((name, 0)).await {
        Ok(it) => {
            let ips = it.map(|sa| sa.ip()).collect::<Vec<_>>();
            if ips.is_empty() {
                return (false, Some("dns resolved to empty result".to_string()), ips);
            }
            if let Some(expect) = expect {
                if ips.iter().any(|ip| *ip == expect) {
                    (true, None, ips)
                } else {
                    // Explicitly disabled (record present but not in enabled state).
                    (false, None, ips)
                }
            } else {
                // Legacy behavior: any successful resolution enables enforcement.
                (true, None, ips)
            }
        }
        Err(e) => (false, Some(e.to_string()), Vec::new()),
    }
}

async fn send_admin_req(sock: &PathBuf, req: &AdminReq) -> anyhow::Result<AdminResp> {
    let mut stream = UnixStream::connect(sock)
        .await
        .with_context(|| format!("connect admin sock {:?}", sock))?;
    stream
        .write_all(serde_json::to_string(req)?.as_bytes())
        .await?;
    stream.write_all(b"\n").await?;
    let (rd, _) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    Ok(serde_json::from_str(line.trim())?)
}

async fn report_status_task(cfg: Config, st: Arc<Mutex<SharedState>>, mut rx: mpsc::Receiver<()>) {
    let mut last_sent: Option<(bool, Option<String>, bool, String, String, String)> = None;
    while rx.recv().await.is_some() {
        let snapshot = {
            let s = st.lock().await;
            (
                s.dns_enabled,
                s.dns_last_error.clone(),
                s.backend_attached,
                s.enforcement_mode.clone(),
                s.dns_name.clone(),
                s.backend.clone(),
            )
        };

        if last_sent.as_ref().is_some_and(|p| p == &snapshot) {
            continue;
        }

        let status = {
            let s = st.lock().await;
            EnforcementStatus {
                node_id: cfg.node_id.clone(),
                dns_name: s.dns_name.clone(),
                dns_enabled: s.dns_enabled,
                dns_last_error: s.dns_last_error.clone(),
                backend: s.backend.clone(),
                backend_attached: s.backend_attached,
                enforcement_mode: s.enforcement_mode.clone(),
                reported_at_unix: now_unix(),
            }
        };

        if let Err(e) = send_admin_req(
            &cfg.admin_sock,
            &AdminReq::ReportEnforcementStatus { status },
        )
        .await
        {
            warn!(err=%e, "failed to report enforcement status");
            continue;
        }

        last_sent = Some(snapshot);
    }
}

async fn dns_task(cfg: Config, st: Arc<Mutex<SharedState>>, status_tx: mpsc::Sender<()>) {
    let interval = std::time::Duration::from_secs(cfg.enable_dns_interval_s.max(1));
    loop {
        let (enabled, err, ips) = dns_check_value(&cfg.enable_dns_name, cfg.enable_dns_ip).await;
        let mut changed = false;
        let mut sync_reports = Vec::<BanApplyResult>::new();
        {
            let mut s = st.lock().await;
            s.dns_last_checked_unix = now_unix();
            if s.dns_enabled != enabled {
                s.dns_enabled = enabled;
                changed = true;
            }
            if s.dns_last_error != err {
                s.dns_last_error = err;
                changed = true;
            }
            s.dns_last_ips = ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>();

            let want_mode = if s.dns_enabled {
                "enforcing"
            } else {
                "fail_open"
            };
            let want_reason = if s.dns_enabled {
                "dns_enabled"
            } else if s.dns_last_error.is_some() {
                "dns_error"
            } else {
                "dns_disabled"
            };
            if s.enforcement_mode != want_mode {
                s.enforcement_mode = want_mode.to_string();
                changed = true;
            }
            if s.enforcement_reason != want_reason {
                s.enforcement_reason = want_reason.to_string();
                changed = true;
            }

            // No-op backend: treat "attached" as the same as "enforcing".
            let want_attached = s.enforcement_mode == "enforcing";
            if s.backend_attached != want_attached {
                s.backend_attached = want_attached;
                changed = true;

                // Detach: clear all applied state.
                if !want_attached {
                    s.applied_bans.clear();
                } else {
                    // Attach: (re)apply the current desired ban set.
                    let now = now_unix();
                    let desired = s.desired_bans.values().cloned().collect::<Vec<_>>();
                    let mut new_applied = HashMap::new();
                    for b in desired {
                        if b.expires_at_unix != 0 && b.expires_at_unix <= now {
                            continue;
                        }
                        if s.exempt.contains_prefix(&b.key) {
                            sync_reports.push(BanApplyResult {
                                node_id: cfg.node_id.clone(),
                                ban_id: b.ban_id.clone(),
                                op: "sync".to_string(),
                                result: "skipped".to_string(),
                                error: Some("exempt_prefix".to_string()),
                                reported_at_unix: now,
                            });
                            continue;
                        }
                        new_applied.insert(b.ban_id.clone(), b.clone());
                        sync_reports.push(BanApplyResult {
                            node_id: cfg.node_id.clone(),
                            ban_id: b.ban_id.clone(),
                            op: "sync".to_string(),
                            result: "ok".to_string(),
                            error: None,
                            reported_at_unix: now,
                        });
                    }
                    s.applied_bans = new_applied;
                }
            }
        }

        // Report backend sync results best-effort (outside the lock).
        for rep in sync_reports {
            let _ = send_admin_req(
                &cfg.admin_sock,
                &AdminReq::ReportBanApplyResult { report: rep },
            )
            .await;
        }

        if changed {
            let _ = status_tx.send(()).await;
        }

        tokio::time::sleep(interval).await;
    }
}

async fn events_task(cfg: Config, st: Arc<Mutex<SharedState>>, status_tx: mpsc::Sender<()>) {
    let mode = if cfg.apply_snapshot {
        SubscribeMode::Snapshot
    } else {
        SubscribeMode::Tail
    };

    loop {
        {
            let mut s = st.lock().await;
            s.events_connected = false;
            s.events_last_error = None;
        }

        let mut stream = match UnixStream::connect(&cfg.events_sock).await {
            Ok(s) => s,
            Err(e) => {
                {
                    let mut s = st.lock().await;
                    s.events_last_error = Some(e.to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        let sub = EventsReq::Subscribe { mode };
        if let Err(e) = stream
            .write_all(serde_json::to_string(&sub).unwrap().as_bytes())
            .await
        {
            warn!(err=%e, "failed to send subscribe");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
        let _ = stream.write_all(b"\n").await;

        let (rd, _) = stream.into_split();
        let mut rd = BufReader::new(rd);
        {
            let mut s = st.lock().await;
            s.events_connected = true;
        }
        let _ = status_tx.send(()).await;

        let mut line = String::new();
        loop {
            line.clear();
            match rd.read_line(&mut line).await {
                Ok(0) => {
                    let mut s = st.lock().await;
                    s.events_connected = false;
                    s.events_last_error = Some("eof".to_string());
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    let mut s = st.lock().await;
                    s.events_connected = false;
                    s.events_last_error = Some(e.to_string());
                    break;
                }
            }
            let raw = line.trim();
            if raw.is_empty() {
                continue;
            }
            let env: EventEnvelope = match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => {
                    warn!(err=%e, line=%raw, "bad event json");
                    continue;
                }
            };

            let mut report: Option<BanApplyResult> = None;
            {
                let mut s = st.lock().await;
                s.events_last_index = s.events_last_index.max(env.index);
                let enforcing = s.enforcement_mode == "enforcing";

                match env.event {
                    Event::Snapshot { bans, .. } => {
                        s.desired_bans.clear();
                        let now = now_unix();
                        for b in bans {
                            if b.expires_at_unix != 0 && b.expires_at_unix <= now {
                                continue;
                            }
                            s.desired_bans.insert(b.ban_id.clone(), b);
                        }

                        if enforcing {
                            let desired = s.desired_bans.values().cloned().collect::<Vec<_>>();
                            let mut new_applied = HashMap::new();
                            for b in desired {
                                if s.exempt.contains_prefix(&b.key) {
                                    continue;
                                }
                                new_applied.insert(b.ban_id.clone(), b);
                            }
                            s.applied_bans = new_applied;
                        }
                    }
                    Event::BanUpserted { entry } => {
                        let op = "upsert".to_string();
                        s.desired_bans.insert(entry.ban_id.clone(), entry.clone());
                        if enforcing {
                            if s.exempt.contains_prefix(&entry.key) {
                                report = Some(BanApplyResult {
                                    node_id: cfg.node_id.clone(),
                                    ban_id: entry.ban_id.clone(),
                                    op,
                                    result: "skipped".to_string(),
                                    error: Some("exempt_prefix".to_string()),
                                    reported_at_unix: now_unix(),
                                });
                            } else {
                                s.applied_bans.insert(entry.ban_id.clone(), entry.clone());
                                report = Some(BanApplyResult {
                                    node_id: cfg.node_id.clone(),
                                    ban_id: entry.ban_id.clone(),
                                    op,
                                    result: "ok".to_string(),
                                    error: None,
                                    reported_at_unix: now_unix(),
                                });
                            }
                        } else {
                            report = Some(BanApplyResult {
                                node_id: cfg.node_id.clone(),
                                ban_id: entry.ban_id.clone(),
                                op,
                                result: "skipped".to_string(),
                                error: Some("enforcement_disabled".to_string()),
                                reported_at_unix: now_unix(),
                            });
                        }
                    }
                    Event::BanDeleted { ban_id } => {
                        let op = "delete".to_string();
                        s.desired_bans.remove(&ban_id);
                        if enforcing {
                            s.applied_bans.remove(&ban_id);
                            report = Some(BanApplyResult {
                                node_id: cfg.node_id.clone(),
                                ban_id: ban_id.clone(),
                                op,
                                result: "ok".to_string(),
                                error: None,
                                reported_at_unix: now_unix(),
                            });
                        } else {
                            report = Some(BanApplyResult {
                                node_id: cfg.node_id.clone(),
                                ban_id: ban_id.clone(),
                                op,
                                result: "skipped".to_string(),
                                error: Some("enforcement_disabled".to_string()),
                                reported_at_unix: now_unix(),
                            });
                        }
                    }
                    Event::LegalHoldUpserted { .. } | Event::LegalHoldDeleted { .. } => {}
                    Event::EnforcementStatus { .. } | Event::BanApplyResult { .. } => {}
                }
            }

            if let Some(rep) = report {
                let _ = send_admin_req(
                    &cfg.admin_sock,
                    &AdminReq::ReportBanApplyResult { report: rep },
                )
                .await;
            }
        }

        let _ = status_tx.send(()).await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn status_server(cfg: Config, st: Arc<Mutex<SharedState>>) -> anyhow::Result<()> {
    async fn handler(st: axum::extract::State<Arc<Mutex<SharedState>>>) -> Json<StatusView> {
        let s = st.lock().await;
        Json(StatusView {
            node_id: s.node_id.clone(),
            dns: DnsView {
                name: s.dns_name.clone(),
                expect_ip: s.dns_expect_ip.clone(),
                enabled: s.dns_enabled,
                last_checked_unix: s.dns_last_checked_unix,
                last_error: s.dns_last_error.clone(),
                last_ips: s.dns_last_ips.clone(),
            },
            enforcement: EnforcementView {
                mode: s.enforcement_mode.clone(),
                reason: s.enforcement_reason.clone(),
                backend: s.backend.clone(),
                backend_attached: s.backend_attached,
            },
            events: EventsView {
                connected: s.events_connected,
                last_index: s.events_last_index,
                last_error: s.events_last_error.clone(),
            },
            apply_snapshot: s.apply_snapshot,
            exempt_prefixes: ExemptView {
                loaded: s.exempt_loaded,
                path: s.exempt_path.clone(),
                count: s.exempt.prefixes.len(),
                last_error: s.exempt_last_error.clone(),
            },
            applied_bans: AppliedBansView {
                count: s.applied_bans.len(),
            },
        })
    }

    let app = Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route("/status", get(handler))
        .with_state(st);

    info!(bind=%cfg.status_http, "status http listening");
    let listener = tokio::net::TcpListener::bind(cfg.status_http)
        .await
        .context("bind status http")?;
    axum::serve(listener, app)
        .await
        .context("serve status http")?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_max_level(Level::INFO)
        .init();

    let (cfg, mode) = parse_args()?;
    if mode == Mode::DetachXdp {
        // Placeholder for future XDP backend. Must be safe to call from systemd ExecStopPost.
        info!("detach-xdp requested (noop backend)");
        return Ok(());
    }

    let (exempt, exempt_loaded, exempt_path, exempt_err) = match cfg.exempt_prefixes_path.as_ref() {
        Some(p) => match ExemptPrefixes::load(p) {
            Ok(v) => (v, true, Some(p.display().to_string()), None),
            Err(e) => (
                ExemptPrefixes::empty(),
                false,
                Some(p.display().to_string()),
                Some(e.to_string()),
            ),
        },
        None => (ExemptPrefixes::empty(), false, None, None),
    };

    let st = Arc::new(Mutex::new(SharedState {
        node_id: cfg.node_id.clone(),
        dns_name: cfg.enable_dns_name.clone(),
        dns_expect_ip: cfg.enable_dns_ip.map(|v| v.to_string()),
        apply_snapshot: cfg.apply_snapshot,

        dns_enabled: false,
        dns_last_checked_unix: 0,
        dns_last_error: None,
        dns_last_ips: Vec::new(),

        enforcement_mode: "fail_open".to_string(),
        enforcement_reason: "startup".to_string(),

        backend: "noop".to_string(),
        backend_attached: false,

        events_connected: false,
        events_last_index: 0,
        events_last_error: None,

        desired_bans: HashMap::new(),
        applied_bans: HashMap::new(),

        exempt,
        exempt_loaded,
        exempt_path,
        exempt_last_error: exempt_err,
    }));

    let (status_tx, status_rx) = mpsc::channel::<()>(64);

    // Report enforcement status to raft on meaningful changes.
    tokio::spawn(report_status_task(cfg.clone(), st.clone(), status_rx));

    // DNS poller drives enforcement_mode.
    tokio::spawn(dns_task(cfg.clone(), st.clone(), status_tx.clone()));

    // Consume raft events (tail-only by default) and apply to backend when enforcing.
    tokio::spawn(events_task(cfg.clone(), st.clone(), status_tx.clone()));

    // Kick an initial report quickly.
    let _ = status_tx.send(()).await;

    status_server(cfg, st).await?;
    Ok(())
}
