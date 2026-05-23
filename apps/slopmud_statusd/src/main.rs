use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Json, Router, extract::State, routing::get};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    title: String,
    expected_envs: Vec<String>,
    checks: Vec<Check>,
    timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Check {
    name: String,
    target: TcpTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TcpTarget {
    host: String,
    port: u16,
}

#[derive(Clone, Debug, serde::Serialize)]
struct StatusDoc {
    title: String,
    generated_unix_ms: u128,
    summary: Summary,
    environments: Vec<String>,
    checks: Vec<CheckResult>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct Summary {
    total: usize,
    ok: usize,
    degraded: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
struct CheckResult {
    name: String,
    target: String,
    ok: bool,
    latency_ms: Option<u128>,
    error: Option<String>,
}

#[derive(Clone)]
struct AppState {
    cfg: Config,
}

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud_statusd

USAGE:
  slopmud_statusd [--bind HOST:PORT]

ENV:
  SLOPMUD_STATUS_BIND    default 0.0.0.0:8080
  SLOPMUD_STATUS_TITLE   default Slopmud Status
  SLOPMUD_STATUS_ENVS    default dev,stg,prd
  SLOPMUD_STATUS_CHECKS  comma-separated name=tcp://host:port checks
"
    );
    std::process::exit(2);
}

fn parse_args() -> anyhow::Result<Config> {
    let mut bind: SocketAddr = std::env::var("SLOPMUD_STATUS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .context("parse SLOPMUD_STATUS_BIND")?;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let Some(v) = it.next() else {
                    usage_and_exit();
                };
                bind = v.parse().context("parse --bind")?;
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    let title =
        std::env::var("SLOPMUD_STATUS_TITLE").unwrap_or_else(|_| "Slopmud Status".to_string());
    let expected_envs = parse_csv(
        &std::env::var("SLOPMUD_STATUS_ENVS").unwrap_or_else(|_| "dev,stg,prd".to_string()),
    );
    let checks_raw = std::env::var("SLOPMUD_STATUS_CHECKS").unwrap_or_else(|_| {
        [
            "dev telnet=tcp://127.0.0.1:4000",
            "prod telnet=tcp://127.0.0.1:4200",
            "websocket=tcp://127.0.0.1:4242",
            "metrics=tcp://127.0.0.1:9912",
        ]
        .join(",")
    });
    let checks = parse_checks(&checks_raw)?;

    Ok(Config {
        bind,
        title,
        expected_envs,
        checks,
        timeout: Duration::from_millis(650),
    })
}

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_checks(raw: &str) -> anyhow::Result<Vec<Check>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_check)
        .collect()
}

fn parse_check(raw: &str) -> anyhow::Result<Check> {
    let Some((name, target)) = raw.split_once('=') else {
        bail!("status check must be name=tcp://host:port: {raw}");
    };
    let Some(host_port) = target.strip_prefix("tcp://") else {
        bail!("status check only supports tcp:// targets: {raw}");
    };
    let Some((host, port)) = host_port.rsplit_once(':') else {
        bail!("tcp target must include a port: {raw}");
    };
    let port: u16 = port
        .parse()
        .with_context(|| format!("parse port in {raw}"))?;
    let name = name.trim();
    let host = host.trim();
    if name.is_empty() || host.is_empty() {
        bail!("status check name and host cannot be empty: {raw}");
    }
    Ok(Check {
        name: name.to_string(),
        target: TcpTarget {
            host: host.to_string(),
            port,
        },
    })
}

async fn collect_status(cfg: &Config) -> StatusDoc {
    let mut checks = Vec::with_capacity(cfg.checks.len());
    for check in &cfg.checks {
        checks.push(run_check(check, cfg.timeout).await);
    }
    let ok = checks.iter().filter(|c| c.ok).count();
    let total = checks.len();
    StatusDoc {
        title: cfg.title.clone(),
        generated_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        summary: Summary {
            total,
            ok,
            degraded: total.saturating_sub(ok),
        },
        environments: cfg.expected_envs.clone(),
        checks,
    }
}

async fn run_check(check: &Check, limit: Duration) -> CheckResult {
    let target = format!("{}:{}", check.target.host, check.target.port);
    let started = std::time::Instant::now();
    match timeout(limit, TcpStream::connect(&target)).await {
        Ok(Ok(_stream)) => CheckResult {
            name: check.name.clone(),
            target,
            ok: true,
            latency_ms: Some(started.elapsed().as_millis()),
            error: None,
        },
        Ok(Err(err)) => CheckResult {
            name: check.name.clone(),
            target,
            ok: false,
            latency_ms: None,
            error: Some(err.to_string()),
        },
        Err(_elapsed) => CheckResult {
            name: check.name.clone(),
            target,
            ok: false,
            latency_ms: None,
            error: Some(format!("timeout after {}ms", limit.as_millis())),
        },
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn api_status(State(st): State<AppState>) -> Json<StatusDoc> {
    Json(collect_status(&st.cfg).await)
}

async fn index(State(st): State<AppState>) -> Html<String> {
    let doc = collect_status(&st.cfg).await;
    Html(render_html(&doc))
}

async fn stylesheet() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        CSS,
    )
        .into_response()
}

fn render_html(doc: &StatusDoc) -> String {
    let state_class = if doc.summary.degraded == 0 {
        "ok"
    } else {
        "degraded"
    };
    let state_label = if doc.summary.degraded == 0 {
        "Operational"
    } else {
        "Degraded"
    };
    let envs = doc
        .environments
        .iter()
        .map(|e| format!(r#"<span>{}</span>"#, escape_html(e)))
        .collect::<Vec<_>>()
        .join("");
    let checks = doc
        .checks
        .iter()
        .map(|c| {
            let class = if c.ok { "ok" } else { "down" };
            let label = if c.ok { "up" } else { "down" };
            let detail = if let Some(ms) = c.latency_ms {
                format!("{ms} ms")
            } else {
                c.error.clone().unwrap_or_else(|| "unreachable".to_string())
            };
            format!(
                r#"<tr><td>{}</td><td>{}</td><td><span class="pill {}">{}</span></td><td>{}</td></tr>"#,
                escape_html(&c.name),
                escape_html(&c.target),
                class,
                label,
                escape_html(&detail)
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="refresh" content="20">
  <title>{title}</title>
  <link rel="stylesheet" href="/style.css">
</head>
<body>
  <main>
    <header>
      <div>
        <h1>{title}</h1>
        <p>{ok}/{total} checks passing</p>
      </div>
      <span class="badge {state_class}">{state_label}</span>
    </header>
    <section class="envs" aria-label="environments">{envs}</section>
    <section>
      <table>
        <thead><tr><th>check</th><th>target</th><th>state</th><th>latency</th></tr></thead>
        <tbody>{checks}</tbody>
      </table>
    </section>
  </main>
</body>
</html>"#,
        title = escape_html(&doc.title),
        ok = doc.summary.ok,
        total = doc.summary.total,
        state_class = state_class,
        state_label = state_label,
        envs = envs,
        checks = checks
    )
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

const CSS: &str = r#"
:root {
  color-scheme: light dark;
  --bg: #f7f8fa;
  --fg: #171a1f;
  --muted: #5f6876;
  --line: #d9dee7;
  --panel: #ffffff;
  --ok: #16794a;
  --bad: #b42318;
  --warn: #9a5b00;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #111316;
    --fg: #eef1f5;
    --muted: #a2aab7;
    --line: #303640;
    --panel: #181c21;
    --ok: #51c285;
    --bad: #ff7b72;
    --warn: #f0b24b;
  }
}

* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font: 15px/1.5 system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
main {
  width: min(920px, calc(100vw - 32px));
  margin: 40px auto;
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  border-bottom: 1px solid var(--line);
  padding-bottom: 18px;
}
h1 {
  margin: 0;
  font-size: 28px;
  font-weight: 700;
}
p {
  margin: 4px 0 0;
  color: var(--muted);
}
.badge,
.pill {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  border-radius: 999px;
  padding: 3px 10px;
  border: 1px solid var(--line);
  font-weight: 650;
}
.badge.ok,
.pill.ok { color: var(--ok); }
.badge.degraded { color: var(--warn); }
.pill.down { color: var(--bad); }
.envs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 18px 0;
}
.envs span {
  border: 1px solid var(--line);
  background: var(--panel);
  border-radius: 999px;
  padding: 4px 10px;
  color: var(--muted);
}
section {
  margin-top: 18px;
}
table {
  width: 100%;
  border-collapse: collapse;
  background: var(--panel);
  border: 1px solid var(--line);
}
th,
td {
  padding: 11px 12px;
  border-bottom: 1px solid var(--line);
  text-align: left;
  vertical-align: middle;
}
th {
  color: var(--muted);
  font-size: 13px;
  font-weight: 650;
}
tr:last-child td { border-bottom: 0; }
td:nth-child(2),
td:nth-child(4) {
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
@media (max-width: 620px) {
  main { width: min(100vw - 20px, 920px); margin: 20px auto; }
  header { align-items: flex-start; flex-direction: column; }
  table { display: block; overflow-x: auto; }
  th, td { white-space: nowrap; }
}
"#;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "slopmud_statusd=info,tower_http=info".into()),
        )
        .init();

    let cfg = parse_args()?;
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/healthz", get(healthz))
        .route("/style.css", get(stylesheet))
        .with_state(AppState { cfg: cfg.clone() });

    info!(bind=%cfg.bind, "status dashboard listening");
    let listener = tokio::net::TcpListener::bind(cfg.bind)
        .await
        .with_context(|| format!("bind {}", cfg.bind))?;
    axum::serve(listener, app).await.context("serve status")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checks() {
        let checks =
            parse_checks("dev=tcp://127.0.0.1:4000, prod=tcp://example.com:4200").expect("checks");
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "dev");
        assert_eq!(checks[0].target.host, "127.0.0.1");
        assert_eq!(checks[0].target.port, 4000);
        assert_eq!(checks[1].target.host, "example.com");
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html("<x&y>"), "&lt;x&amp;y&gt;");
    }
}
