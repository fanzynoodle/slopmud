use std::path::PathBuf;

use anyhow::Context;
use sbc_core::{AdminReq, AdminResp, ExemptPrefixes, IpPrefix};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{Level, info, warn};

fn usage_and_exit() -> ! {
    eprintln!(
        "sbc_deciderd

USAGE:
  sbc_deciderd

ENV:
  SBC_DECIDER_ENABLED             default 0
  SBC_DECIDER_POLL_S              default 5
  SBC_DECIDER_THRESHOLD_BYTES_IN  default 0 (disabled)
  SBC_DECIDER_TTL_S               default 3600
  SBC_METRICS_URL                 default http://127.0.0.1:9912
  SBC_ADMIN_SOCK                  default /run/slopmud/sbc-admin.sock
  SBC_DECIDER_CREATED_BY          default sbc-deciderd
  SBC_EXEMPT_PREFIXES_PATH        default empty
"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    enabled: bool,
    poll_s: u64,
    threshold_bytes_in: u64,
    ttl_s: u64,
    metrics_url: String,
    admin_sock: PathBuf,
    created_by: String,
    exempt: ExemptPrefixes,
}

fn parse_args() -> Config {
    let enabled = std::env::var("SBC_DECIDER_ENABLED")
        .ok()
        .is_some_and(|v| v == "1");
    let poll_s = std::env::var("SBC_DECIDER_POLL_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let threshold_bytes_in = std::env::var("SBC_DECIDER_THRESHOLD_BYTES_IN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let ttl_s = std::env::var("SBC_DECIDER_TTL_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let metrics_url =
        std::env::var("SBC_METRICS_URL").unwrap_or_else(|_| "http://127.0.0.1:9912".to_string());
    let admin_sock: PathBuf = std::env::var("SBC_ADMIN_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-admin.sock".to_string())
        .into();
    let created_by =
        std::env::var("SBC_DECIDER_CREATED_BY").unwrap_or_else(|_| "sbc-deciderd".to_string());
    let exempt = match std::env::var("SBC_EXEMPT_PREFIXES_PATH").ok() {
        Some(p) if !p.trim().is_empty() => match ExemptPrefixes::load(std::path::Path::new(&p)) {
            Ok(v) => v,
            Err(e) => {
                warn!(err=%e, path=%p, "failed to load exempt prefixes; continuing with empty set");
                ExemptPrefixes::empty()
            }
        },
        _ => ExemptPrefixes::empty(),
    };

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        enabled,
        poll_s,
        threshold_bytes_in,
        ttl_s,
        metrics_url,
        admin_sock,
        created_by,
        exempt,
    }
}

#[derive(Debug, serde::Deserialize)]
struct TopResp {
    metric: String,
    group: String,
    top: Vec<TopItem>,
}

#[derive(Debug, serde::Deserialize)]
struct TopItem {
    key: String,
    value: u64,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();
    info!(
        enabled = cfg.enabled,
        poll_s = cfg.poll_s,
        threshold_bytes_in = cfg.threshold_bytes_in,
        ttl_s = cfg.ttl_s,
        metrics_url = %cfg.metrics_url,
        admin_sock = %cfg.admin_sock.display(),
        "sbc_deciderd starting"
    );

    if !cfg.enabled || cfg.threshold_bytes_in == 0 {
        info!("decider disabled (set SBC_DECIDER_ENABLED=1 and threshold envs)");
    }

    let http = reqwest::Client::new();
    let mut banned = std::collections::HashSet::<String>::new();

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(cfg.poll_s.max(1))).await;
        if !cfg.enabled || cfg.threshold_bytes_in == 0 {
            continue;
        }

        let url = format!(
            "{}/top?metric=bytes_in&group=src&n=20",
            cfg.metrics_url.trim_end_matches('/')
        );
        let resp = match http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(err=%e, "metrics fetch failed");
                continue;
            }
        };
        if !resp.status().is_success() {
            warn!(status=%resp.status(), "metrics fetch non-200");
            continue;
        }
        let top: TopResp = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!(err=%e, "metrics json parse failed");
                continue;
            }
        };

        for item in top.top {
            if item.value < cfg.threshold_bytes_in {
                continue;
            }
            if banned.contains(&item.key) {
                continue;
            }

            // Ban exact IP.
            let ip: std::net::IpAddr = match item.key.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let plen = if ip.is_ipv4() { 32 } else { 128 };
            let pfx = IpPrefix::new(ip, plen).context("build prefix")?;
            if cfg.exempt.contains_prefix(&pfx) {
                continue;
            }
            let req = AdminReq::UpsertBan {
                key: pfx.to_cidr_string(),
                ttl_s: cfg.ttl_s,
                created_by: cfg.created_by.clone(),
                reason: format!("auto bytes_in {}", item.value),
            };
            match send_admin_req(&cfg.admin_sock, &req).await {
                Ok(AdminResp::OkBan { .. } | AdminResp::Ok { .. }) => {
                    banned.insert(item.key);
                }
                Ok(AdminResp::Err { message }) => {
                    warn!(message=%message, "ban rejected");
                }
                Err(e) => {
                    warn!(err=%e, "ban proposal failed");
                }
                _ => {}
            }
        }
    }
}
