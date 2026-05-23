use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use axum::extract::{Host, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Json, Router, routing::get};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::info;

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    title: String,
    expected_envs: Vec<String>,
    status_hosts: Vec<String>,
    checks: Vec<Check>,
    timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Check {
    name: String,
    env: String,
    service: String,
    instance: String,
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
    environments: Vec<EnvironmentStatus>,
    checks: Vec<CheckResult>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
struct Summary {
    total: usize,
    ok: usize,
    failed: usize,
    degraded: usize,
    unconfigured_envs: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
struct EnvironmentStatus {
    name: String,
    expected: bool,
    configured: bool,
    summary: Summary,
    services: Vec<ServiceStatus>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct ServiceStatus {
    name: String,
    summary: Summary,
    instances: Vec<CheckResult>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct CheckResult {
    name: String,
    env: String,
    service: String,
    instance: String,
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
  SLOPMUD_STATUS_HOSTS   comma-separated hostnames served as status page; other hosts redirect to HTTPS
  SLOPMUD_STATUS_CHECKS  comma-separated env/service/instance=tcp://host:port checks
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
    let status_hosts = parse_csv(
        &std::env::var("SLOPMUD_STATUS_HOSTS")
            .unwrap_or_else(|_| "status.slopmud.com,localhost,127.0.0.1".to_string()),
    )
    .into_iter()
    .map(|host| host.to_ascii_lowercase())
    .collect::<Vec<_>>();
    let checks_raw = std::env::var("SLOPMUD_STATUS_CHECKS").unwrap_or_else(|_| {
        [
            "dev/broker/gateway=tcp://127.0.0.1:4000",
            "stg/broker/gateway=tcp://127.0.0.1:4023",
            "prd/broker/gateway=tcp://127.0.0.1:4200",
            "prd/websocket/gateway=tcp://127.0.0.1:4242",
            "ops/metrics/gateway=tcp://127.0.0.1:9912",
        ]
        .join(",")
    });
    let checks = parse_checks(&checks_raw, &expected_envs)?;

    Ok(Config {
        bind,
        title,
        expected_envs,
        status_hosts,
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

fn parse_checks(raw: &str, expected_envs: &[String]) -> anyhow::Result<Vec<Check>> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| parse_check(s, expected_envs))
        .collect()
}

fn parse_check(raw: &str, expected_envs: &[String]) -> anyhow::Result<Check> {
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
    let (env, service, instance) = parse_check_identity(name, expected_envs, host, port)?;
    Ok(Check {
        name: format!("{env}/{service}/{instance}"),
        env,
        service,
        instance,
        target: TcpTarget {
            host: host.to_string(),
            port,
        },
    })
}

fn parse_check_identity(
    raw: &str,
    expected_envs: &[String],
    host: &str,
    port: u16,
) -> anyhow::Result<(String, String, String)> {
    let slash_parts = raw
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if slash_parts.len() >= 2 {
        if slash_parts.len() > 3 {
            bail!("status check identity must be env/service[/instance]: {raw}");
        }
        return Ok((
            slash_parts[0].to_string(),
            slash_parts[1].to_string(),
            slash_parts
                .get(2)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| default_instance(host, port)),
        ));
    }

    let mut words = raw.split_whitespace();
    if let Some(first) = words.next() {
        if expected_envs.iter().any(|e| e == first) {
            let service = words.collect::<Vec<_>>().join(" ");
            if !service.trim().is_empty() {
                return Ok((first.to_string(), service, default_instance(host, port)));
            }
        }
    }

    Ok((
        "ops".to_string(),
        raw.trim().to_string(),
        default_instance(host, port),
    ))
}

fn default_instance(host: &str, port: u16) -> String {
    match host {
        "127.0.0.1" | "localhost" => "gateway".to_string(),
        _ => format!("{host}:{port}"),
    }
}

async fn collect_status(cfg: &Config) -> StatusDoc {
    let mut checks = Vec::with_capacity(cfg.checks.len());
    for check in &cfg.checks {
        checks.push(run_check(check, cfg.timeout).await);
    }
    let ok = checks.iter().filter(|c| c.ok).count();
    let total = checks.len();
    let failed = total.saturating_sub(ok);
    let environments = build_environments(&cfg.expected_envs, &checks);
    let unconfigured_envs = environments
        .iter()
        .filter(|e| e.expected && !e.configured)
        .count();
    StatusDoc {
        title: cfg.title.clone(),
        generated_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        summary: Summary {
            total,
            ok,
            failed,
            degraded: failed + unconfigured_envs,
            unconfigured_envs,
        },
        environments,
        checks,
    }
}

async fn run_check(check: &Check, limit: Duration) -> CheckResult {
    let target = format!("{}:{}", check.target.host, check.target.port);
    let started = std::time::Instant::now();
    match timeout(limit, TcpStream::connect(&target)).await {
        Ok(Ok(_stream)) => CheckResult {
            name: check.name.clone(),
            env: check.env.clone(),
            service: check.service.clone(),
            instance: check.instance.clone(),
            target,
            ok: true,
            latency_ms: Some(started.elapsed().as_millis()),
            error: None,
        },
        Ok(Err(err)) => CheckResult {
            name: check.name.clone(),
            env: check.env.clone(),
            service: check.service.clone(),
            instance: check.instance.clone(),
            target,
            ok: false,
            latency_ms: None,
            error: Some(err.to_string()),
        },
        Err(_elapsed) => CheckResult {
            name: check.name.clone(),
            env: check.env.clone(),
            service: check.service.clone(),
            instance: check.instance.clone(),
            target,
            ok: false,
            latency_ms: None,
            error: Some(format!("timeout after {}ms", limit.as_millis())),
        },
    }
}

fn build_environments(expected_envs: &[String], checks: &[CheckResult]) -> Vec<EnvironmentStatus> {
    let mut env_order = expected_envs.to_vec();
    for check in checks {
        if !env_order.iter().any(|env| env == &check.env) {
            env_order.push(check.env.clone());
        }
    }

    env_order
        .into_iter()
        .map(|name| {
            let env_checks = checks
                .iter()
                .filter(|check| check.env == name)
                .cloned()
                .collect::<Vec<_>>();
            let mut by_service: BTreeMap<String, Vec<CheckResult>> = BTreeMap::new();
            for check in &env_checks {
                by_service
                    .entry(check.service.clone())
                    .or_default()
                    .push(check.clone());
            }
            let services = by_service
                .into_iter()
                .map(|(service, instances)| ServiceStatus {
                    name: service,
                    summary: summarize_checks(&instances),
                    instances,
                })
                .collect::<Vec<_>>();
            EnvironmentStatus {
                expected: expected_envs.iter().any(|env| env == &name),
                configured: !env_checks.is_empty(),
                summary: summarize_checks(&env_checks),
                name,
                services,
            }
        })
        .collect()
}

fn summarize_checks(checks: &[CheckResult]) -> Summary {
    let total = checks.len();
    let ok = checks.iter().filter(|check| check.ok).count();
    let failed = total.saturating_sub(ok);
    Summary {
        total,
        ok,
        failed,
        degraded: failed,
        unconfigured_envs: 0,
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn api_status(Host(host): Host, State(st): State<AppState>, uri: Uri) -> Response {
    if let Some(resp) = redirect_non_status_host(&st.cfg, &host, &uri) {
        return resp;
    }
    Json(collect_status(&st.cfg).await).into_response()
}

async fn index(Host(host): Host, State(st): State<AppState>, uri: Uri) -> Response {
    if let Some(resp) = redirect_non_status_host(&st.cfg, &host, &uri) {
        return resp;
    }
    let doc = collect_status(&st.cfg).await;
    Html(render_html(&doc)).into_response()
}

async fn stylesheet() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        CSS,
    )
        .into_response()
}

fn redirect_non_status_host(cfg: &Config, host: &str, uri: &Uri) -> Option<Response> {
    if status_host_allowed(&cfg.status_hosts, host) {
        return None;
    }
    let bare_host = host.split(':').next().unwrap_or(host);
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    Some(Redirect::permanent(&format!("https://{bare_host}{path}")).into_response())
}

fn status_host_allowed(status_hosts: &[String], host: &str) -> bool {
    if status_hosts.is_empty() {
        return true;
    }
    let bare_host = host
        .split(':')
        .next()
        .unwrap_or(host)
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    status_hosts.iter().any(|allowed| allowed == &bare_host)
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
        .map(render_env_chip)
        .collect::<Vec<_>>()
        .join("");
    let environments = doc
        .environments
        .iter()
        .map(render_environment)
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
    {environments}
  </main>
</body>
</html>"#,
        title = escape_html(&doc.title),
        ok = doc.summary.ok,
        total = doc.summary.total,
        state_class = state_class,
        state_label = state_label,
        envs = envs,
        environments = environments
    )
}

fn render_env_chip(env: &EnvironmentStatus) -> String {
    let class = env_class(env);
    let label = if !env.configured {
        "unconfigured".to_string()
    } else {
        format!("{}/{}", env.summary.ok, env.summary.total)
    };
    format!(
        r#"<span class="{class}"><strong>{name}</strong>{label}</span>"#,
        class = class,
        name = escape_html(&env.name),
        label = escape_html(&label)
    )
}

fn render_environment(env: &EnvironmentStatus) -> String {
    let class = env_class(env);
    let status = if !env.configured {
        "unconfigured".to_string()
    } else {
        format!("{}/{} up", env.summary.ok, env.summary.total)
    };
    let body = if env.services.is_empty() {
        r#"<p class="empty">No checks configured</p>"#.to_string()
    } else {
        let rows = env
            .services
            .iter()
            .flat_map(|service| {
                service
                    .instances
                    .iter()
                    .map(|check| render_check_row(service, check))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .join("");
        format!(
            r#"<table>
        <thead><tr><th>service</th><th>instance</th><th>target</th><th>state</th><th>latency</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>"#,
            rows = rows
        )
    };
    format!(
        r#"<section class="env-block {class}">
      <div class="env-head">
        <h2>{name}</h2>
        <span class="badge {class}">{status}</span>
      </div>
      {body}
    </section>"#,
        class = class,
        name = escape_html(&env.name),
        status = escape_html(&status),
        body = body
    )
}

fn render_check_row(service: &ServiceStatus, check: &CheckResult) -> String {
    let class = if check.ok { "ok" } else { "down" };
    let label = if check.ok { "up" } else { "down" };
    let detail = if let Some(ms) = check.latency_ms {
        format!("{ms} ms")
    } else {
        check
            .error
            .clone()
            .unwrap_or_else(|| "unreachable".to_string())
    };
    format!(
        r#"<tr><td class="service-name">{service}</td><td>{instance}</td><td>{target}</td><td><span class="pill {class}">{label}</span></td><td>{detail}</td></tr>"#,
        service = escape_html(&service.name),
        instance = escape_html(&check.instance),
        target = escape_html(&check.target),
        class = class,
        label = label,
        detail = escape_html(&detail)
    )
}

fn env_class(env: &EnvironmentStatus) -> &'static str {
    if !env.configured {
        "missing"
    } else if env.summary.failed == 0 {
        "ok"
    } else {
        "down"
    }
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
.badge.degraded,
.badge.missing { color: var(--warn); }
.badge.down { color: var(--bad); }
.pill.down { color: var(--bad); }
.envs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 18px 0;
}
.envs span {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--line);
  background: var(--panel);
  border-radius: 999px;
  padding: 4px 10px;
  color: var(--muted);
}
.envs span.ok strong { color: var(--ok); }
.envs span.down strong { color: var(--bad); }
.envs span.missing strong { color: var(--warn); }
section {
  margin-top: 18px;
}
.env-block {
  border-top: 1px solid var(--line);
  padding-top: 18px;
}
.env-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 700;
}
.empty {
  border: 1px solid var(--line);
  background: var(--panel);
  margin: 0;
  padding: 12px;
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
td:nth-child(3),
td:nth-child(5) {
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.service-name {
  font-weight: 650;
}
@media (max-width: 620px) {
  main { width: min(100vw - 20px, 920px); margin: 20px auto; }
  header { align-items: flex-start; flex-direction: column; }
  .env-head { align-items: flex-start; flex-direction: column; }
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
        let expected_envs = vec!["dev".to_string(), "stg".to_string(), "prd".to_string()];
        let checks = parse_checks(
            "dev/broker/gateway=tcp://127.0.0.1:4000, prd/websocket/gateway=tcp://example.com:4200",
            &expected_envs,
        )
        .expect("checks");
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "dev/broker/gateway");
        assert_eq!(checks[0].env, "dev");
        assert_eq!(checks[0].service, "broker");
        assert_eq!(checks[0].instance, "gateway");
        assert_eq!(checks[0].target.host, "127.0.0.1");
        assert_eq!(checks[0].target.port, 4000);
        assert_eq!(checks[1].target.host, "example.com");
    }

    #[test]
    fn parses_legacy_check_names_with_env_prefix() {
        let expected_envs = vec!["dev".to_string(), "stg".to_string(), "prd".to_string()];
        let checks = parse_checks(
            "dev telnet=tcp://127.0.0.1:4000, metrics=tcp://10.0.0.12:9912",
            &expected_envs,
        )
        .expect("checks");
        assert_eq!(checks[0].env, "dev");
        assert_eq!(checks[0].service, "telnet");
        assert_eq!(checks[0].instance, "gateway");
        assert_eq!(checks[1].env, "ops");
        assert_eq!(checks[1].service, "metrics");
        assert_eq!(checks[1].instance, "10.0.0.12:9912");
    }

    #[test]
    fn groups_checks_by_environment_and_service() {
        let expected_envs = vec!["dev".to_string(), "stg".to_string(), "prd".to_string()];
        let checks = vec![
            CheckResult {
                name: "dev/broker/gateway".to_string(),
                env: "dev".to_string(),
                service: "broker".to_string(),
                instance: "gateway".to_string(),
                target: "127.0.0.1:4000".to_string(),
                ok: true,
                latency_ms: Some(1),
                error: None,
            },
            CheckResult {
                name: "sandbox/broker/gateway".to_string(),
                env: "sandbox".to_string(),
                service: "broker".to_string(),
                instance: "gateway".to_string(),
                target: "127.0.0.1:4500".to_string(),
                ok: true,
                latency_ms: Some(1),
                error: None,
            },
            CheckResult {
                name: "prd/websocket/gateway".to_string(),
                env: "prd".to_string(),
                service: "websocket".to_string(),
                instance: "gateway".to_string(),
                target: "127.0.0.1:4242".to_string(),
                ok: false,
                latency_ms: None,
                error: Some("refused".to_string()),
            },
        ];
        let envs = build_environments(&expected_envs, &checks);
        assert_eq!(
            envs.iter().map(|env| env.name.as_str()).collect::<Vec<_>>(),
            vec!["dev", "stg", "prd", "sandbox"]
        );
        assert!(envs[1].expected);
        assert!(!envs[1].configured);
        assert_eq!(envs[2].summary.failed, 1);
        assert_eq!(envs[3].services[0].instances[0].instance, "gateway");
    }

    #[test]
    fn renders_environment_service_and_instance_labels() {
        let doc = StatusDoc {
            title: "test".to_string(),
            generated_unix_ms: 0,
            summary: Summary {
                total: 1,
                ok: 1,
                failed: 0,
                degraded: 0,
                unconfigured_envs: 0,
            },
            environments: vec![EnvironmentStatus {
                name: "dev".to_string(),
                expected: true,
                configured: true,
                summary: Summary {
                    total: 1,
                    ok: 1,
                    failed: 0,
                    degraded: 0,
                    unconfigured_envs: 0,
                },
                services: vec![ServiceStatus {
                    name: "broker".to_string(),
                    summary: Summary {
                        total: 1,
                        ok: 1,
                        failed: 0,
                        degraded: 0,
                        unconfigured_envs: 0,
                    },
                    instances: vec![CheckResult {
                        name: "dev/broker/gateway".to_string(),
                        env: "dev".to_string(),
                        service: "broker".to_string(),
                        instance: "gateway".to_string(),
                        target: "127.0.0.1:4000".to_string(),
                        ok: true,
                        latency_ms: Some(0),
                        error: None,
                    }],
                }],
            }],
            checks: Vec::new(),
        };
        let html = render_html(&doc);
        assert!(html.contains("<h2>dev</h2>"));
        assert!(html.contains("broker"));
        assert!(html.contains("gateway"));
    }

    #[test]
    fn only_configured_status_hosts_are_served_as_status_page() {
        let hosts = vec![
            "status.slopmud.com".to_string(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];
        assert!(status_host_allowed(&hosts, "status.slopmud.com"));
        assert!(status_host_allowed(&hosts, "status.slopmud.com:80"));
        assert!(status_host_allowed(&hosts, "LOCALHOST:8080"));
        assert!(!status_host_allowed(&hosts, "slopmud.com"));
        assert!(!status_host_allowed(&hosts, "www.slopmud.com"));
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html("<x&y>"), "&lt;x&amp;y&gt;");
    }
}
