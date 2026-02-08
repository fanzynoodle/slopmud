use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::{Json, Router, extract::Query, routing::get};
use sbc_core::IpPrefix;
use tokio::sync::Mutex;
use tracing::{Level, info, warn};

fn usage_and_exit() -> ! {
    eprintln!(
        "sbc_metricsd

USAGE:
  sbc_metricsd [--statsd-bind HOST:PORT] [--http-bind HOST:PORT]

ENV:
  SBC_STATSD_BIND  default 0.0.0.0:8125
  SBC_METRICS_HTTP default 127.0.0.1:9912
"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    statsd_bind: SocketAddr,
    http_bind: SocketAddr,
}

fn parse_args() -> Config {
    let mut statsd_bind: SocketAddr = std::env::var("SBC_STATSD_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8125".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());
    let mut http_bind: SocketAddr = std::env::var("SBC_METRICS_HTTP")
        .unwrap_or_else(|_| "127.0.0.1:9912".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--statsd-bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                statsd_bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--http-bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                http_bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        statsd_bind,
        http_bind,
    }
}

#[derive(Clone, Default, Debug)]
struct Counters {
    bytes_in: u64,
    bytes_out: u64,
    conns: u64,
}

#[derive(Clone, Default, Debug)]
struct Agg {
    by_src: HashMap<String, Counters>,
    by_v4_18: HashMap<String, Counters>,
    by_v6_48: HashMap<String, Counters>,
}

fn parse_statsd_line(line: &str) -> Option<(String, i64, String, HashMap<String, String>)> {
    // name:value|type|#tag=value,tag2=value
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split('|');
    let name_and_val = parts.next()?;
    let typ = parts.next()?.to_string();
    let tags_part = parts.next(); // may be "#..."

    let (name, val_s) = name_and_val.split_once(':')?;
    let val: i64 = val_s.parse().ok()?;

    let mut tags = HashMap::new();
    if let Some(t) = tags_part {
        let t = t.trim();
        if let Some(rest) = t.strip_prefix("#") {
            for kv in rest.split(',') {
                let kv = kv.trim();
                if kv.is_empty() {
                    continue;
                }
                if let Some((k, v)) = kv.split_once('=') {
                    tags.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
    Some((name.to_string(), val, typ, tags))
}

fn v4_18_key(ip: std::net::Ipv4Addr) -> String {
    let p = IpPrefix::new(std::net::IpAddr::V4(ip), 18).expect("valid /18");
    p.to_cidr_string()
}

fn v6_48_key(ip: std::net::Ipv6Addr) -> String {
    let p = IpPrefix::new(std::net::IpAddr::V6(ip), 48).expect("valid /48");
    p.to_cidr_string()
}

async fn statsd_task(cfg: Config, agg: Arc<Mutex<Agg>>) -> anyhow::Result<()> {
    let sock = tokio::net::UdpSocket::bind(cfg.statsd_bind)
        .await
        .with_context(|| format!("bind statsd {}", cfg.statsd_bind))?;

    info!(bind=%cfg.statsd_bind, "statsd listening");
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let (n, _peer) = sock.recv_from(&mut buf).await?;
        if n == 0 {
            continue;
        }
        let s = String::from_utf8_lossy(&buf[..n]);
        for raw_line in s.lines() {
            let Some((name, val, typ, tags)) = parse_statsd_line(raw_line) else {
                continue;
            };
            if typ != "c" {
                continue;
            }
            let Some(src) = tags.get("src").cloned() else {
                continue;
            };

            let mut a = agg.lock().await;
            let c = a.by_src.entry(src.clone()).or_default();
            match name.as_str() {
                "sbc.bytes_in" => c.bytes_in = c.bytes_in.saturating_add(val.max(0) as u64),
                "sbc.bytes_out" => c.bytes_out = c.bytes_out.saturating_add(val.max(0) as u64),
                "sbc.conns" => c.conns = c.conns.saturating_add(val.max(0) as u64),
                _ => continue,
            }

            // Also aggregate into v4/v6 blocks.
            if let Ok(ip) = src.parse::<std::net::IpAddr>() {
                match ip {
                    std::net::IpAddr::V4(v4) => {
                        let k = v4_18_key(v4);
                        let c = a.by_v4_18.entry(k).or_default();
                        match name.as_str() {
                            "sbc.bytes_in" => {
                                c.bytes_in = c.bytes_in.saturating_add(val.max(0) as u64)
                            }
                            "sbc.bytes_out" => {
                                c.bytes_out = c.bytes_out.saturating_add(val.max(0) as u64)
                            }
                            "sbc.conns" => c.conns = c.conns.saturating_add(val.max(0) as u64),
                            _ => {}
                        }
                    }
                    std::net::IpAddr::V6(v6) => {
                        let k = v6_48_key(v6);
                        let c = a.by_v6_48.entry(k).or_default();
                        match name.as_str() {
                            "sbc.bytes_in" => {
                                c.bytes_in = c.bytes_in.saturating_add(val.max(0) as u64)
                            }
                            "sbc.bytes_out" => {
                                c.bytes_out = c.bytes_out.saturating_add(val.max(0) as u64)
                            }
                            "sbc.conns" => c.conns = c.conns.saturating_add(val.max(0) as u64),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct TopQuery {
    metric: Option<String>, // bytes_in | bytes_out | conns
    group: Option<String>,  // src | v4_18 | v6_48
    n: Option<usize>,
}

#[derive(serde::Serialize)]
struct TopItem {
    key: String,
    value: u64,
}

#[derive(serde::Serialize)]
struct TopResp {
    metric: String,
    group: String,
    top: Vec<TopItem>,
}

async fn top_handler(
    Query(q): Query<TopQuery>,
    axum::extract::State(agg): axum::extract::State<Arc<Mutex<Agg>>>,
) -> Json<TopResp> {
    let metric = q.metric.unwrap_or_else(|| "bytes_in".to_string());
    let group = q.group.unwrap_or_else(|| "src".to_string());
    let n = q.n.unwrap_or(20).clamp(1, 200);

    let a = agg.lock().await;
    let map: &HashMap<String, Counters> = match group.as_str() {
        "src" => &a.by_src,
        "v4_18" => &a.by_v4_18,
        "v6_48" => &a.by_v6_48,
        _ => &a.by_src,
    };

    let mut v = map
        .iter()
        .map(|(k, c)| {
            let val = match metric.as_str() {
                "bytes_out" => c.bytes_out,
                "conns" => c.conns,
                _ => c.bytes_in,
            };
            TopItem {
                key: k.clone(),
                value: val,
            }
        })
        .collect::<Vec<_>>();
    v.sort_by_key(|x| std::cmp::Reverse(x.value));
    v.truncate(n);
    Json(TopResp {
        metric,
        group,
        top: v,
    })
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
    let agg = Arc::new(Mutex::new(Agg::default()));

    // StatsD UDP listener in background.
    {
        let cfg2 = cfg.clone();
        let agg2 = agg.clone();
        tokio::spawn(async move {
            if let Err(e) = statsd_task(cfg2, agg2).await {
                warn!(err=%e, "statsd task ended");
            }
        });
    }

    let app = Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route("/top", get(top_handler))
        .with_state(agg);

    info!(bind=%cfg.http_bind, "metrics http listening");
    let listener = tokio::net::TcpListener::bind(cfg.http_bind)
        .await
        .with_context(|| format!("bind http {}", cfg.http_bind))?;
    axum::serve(listener, app).await.context("serve http")?;
    Ok(())
}
