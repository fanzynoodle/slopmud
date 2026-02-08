use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Router,
    extract::{ConnectInfo, Host, Query, State, ws},
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use slopio::telnet::IacParser;
use tokio_util::sync::CancellationToken;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{Level, info, warn};

mod compliance_portal;

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud_web

USAGE:
  slopmud_web [--bind HOST:PORT] [--dir PATH] [--https-bind HOST:PORT --tls-cert PATH --tls-key PATH] [--session-tcp-addr HOST:PORT] [--admin-tcp-addr HOST:PORT]

ENV:
  BIND                     default 0.0.0.0:8080
  STATIC_DIR               default web_homepage
  HTTPS_BIND               optional
  TLS_CERT                 required if HTTPS_BIND set
  TLS_KEY                  required if HTTPS_BIND set
  SESSION_TCP_ADDR         default 127.0.0.1:23 (used by /ws)
  WEB_SESSION_IDLE_TTL_S   optional; default 600 (keep web sessions alive across reload)
  SLOPMUD_ADMIN_ADDR        default 127.0.0.1:4011 (used by /api/online; falls back to SLOPMUD_ADMIN_BIND)
  SLOPMUD_GOOGLE_OAUTH_DIR  default locks/google_oauth (shared with slopmud broker)
  GOOGLE_OAUTH_CLIENT_ID   required to enable Google SSO
  GOOGLE_OAUTH_CLIENT_SECRET required to enable Google SSO
  GOOGLE_OAUTH_REDIRECT_URI required to enable Google SSO (e.g. https://slopmud.com/auth/google/callback)
"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    http_bind: SocketAddr,
    https_bind: Option<SocketAddr>,
    static_dir: PathBuf,
    tls_cert: Option<PathBuf>,
    tls_key: Option<PathBuf>,
    session_tcp_addr: SocketAddr,
    admin_tcp_addr: SocketAddr,
    google_oauth_dir: PathBuf,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    google_redirect_uri: Option<String>,
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut https_bind: Option<SocketAddr> = std::env::var("HTTPS_BIND")
        .ok()
        .and_then(|v| v.parse().ok());

    let mut dir: PathBuf = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| "web_homepage".to_string())
        .into();

    let mut tls_cert: Option<PathBuf> = std::env::var("TLS_CERT").ok().map(Into::into);
    let mut tls_key: Option<PathBuf> = std::env::var("TLS_KEY").ok().map(Into::into);

    let mut session_tcp_addr: SocketAddr = std::env::var("SESSION_TCP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:23".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut admin_tcp_addr: SocketAddr = std::env::var("SLOPMUD_ADMIN_ADDR")
        .or_else(|_| std::env::var("SLOPMUD_ADMIN_BIND"))
        .unwrap_or_else(|_| "127.0.0.1:4011".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let google_oauth_dir: PathBuf = std::env::var("SLOPMUD_GOOGLE_OAUTH_DIR")
        .unwrap_or_else(|_| "locks/google_oauth".to_string())
        .into();

    let google_client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok();
    let google_client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").ok();
    let google_redirect_uri = std::env::var("GOOGLE_OAUTH_REDIRECT_URI").ok();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--https-bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                https_bind = Some(v.parse().unwrap_or_else(|_| usage_and_exit()));
            }
            "--dir" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                dir = v.into();
            }
            "--tls-cert" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                tls_cert = Some(v.into());
            }
            "--tls-key" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                tls_key = Some(v.into());
            }
            "--session-tcp-addr" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                session_tcp_addr = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--admin-tcp-addr" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                admin_tcp_addr = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        http_bind: bind,
        https_bind,
        static_dir: dir,
        tls_cert,
        tls_key,
        session_tcp_addr,
        admin_tcp_addr,
        google_oauth_dir,
        google_client_id,
        google_client_secret,
        google_redirect_uri,
    }
}

async fn redirect_to_https(Host(host): Host, uri: Uri) -> Redirect {
    // Host may include :port; strip it for canonical redirects.
    let host = host.split(':').next().unwrap_or(&host);
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    Redirect::permanent(&format!("https://{host}{path}"))
}

async fn ws_session(
    ws: ws::WebSocketUpgrade,
    Query(q): Query<WsSessionQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| async move { ws_session_task(socket, state, peer, q.resume).await })
}

#[derive(Clone, Debug, Deserialize, Default)]
struct WsSessionQuery {
    resume: Option<String>,
}

#[derive(Clone)]
struct AppState {
    session_tcp_addr: SocketAddr,
    admin_tcp_addr: SocketAddr,
    web_sessions: WebSessionManager,
    oauth: OAuthState,
    compliance: compliance_portal::ComplianceState,
}

async fn api_online(State(state): State<AppState>) -> impl IntoResponse {
    let addr = state.admin_tcp_addr;
    let req = b"{\"type\":\"list_sessions\"}\n";
    let res = tokio::time::timeout(Duration::from_secs(2), async move {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, req).await?;
        let mut rd = tokio::io::BufReader::new(stream);
        let mut line = Vec::new();
        tokio::io::AsyncBufReadExt::read_until(&mut rd, b'\n', &mut line).await?;
        Ok::<Vec<u8>, std::io::Error>(line)
    })
    .await;

    match res {
        Ok(Ok(line)) if !line.is_empty() => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            String::from_utf8_lossy(&line).into_owned(),
        )
            .into_response(),
        Ok(Ok(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "application/json")],
            "{\"type\":\"err\",\"message\":\"empty admin response\"}\n".to_string(),
        )
            .into_response(),
        Ok(Err(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "application/json")],
            "{\"type\":\"err\",\"message\":\"admin unavailable\"}\n".to_string(),
        )
            .into_response(),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            [(header::CONTENT_TYPE, "application/json")],
            "{\"type\":\"err\",\"message\":\"admin timeout\"}\n".to_string(),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct WsLogoutReq {
    resume: String,
}

async fn api_ws_logout(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<WsLogoutReq>,
) -> impl IntoResponse {
    let ok = state.web_sessions.logout(&req.resume).await;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"ok\":{}}}\n", if ok { "true" } else { "false" }),
    )
}

#[derive(Clone, Debug)]
struct OAuthState {
    dir: PathBuf,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
    web_pending_google: Arc<tokio::sync::Mutex<HashMap<String, WebOAuthPending>>>,
    web_identities: Arc<tokio::sync::Mutex<HashMap<String, WebOAuthIdentity>>>,
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

#[derive(Debug, Clone)]
struct WebOAuthPending {
    resume: String,
    verifier: String,
    return_to: String,
    created_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
struct WebOAuthIdentity {
    provider: String, // google | oidc
    sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    created_unix: u64,
}

fn pending_path(dir: &Path, code: &str) -> PathBuf {
    let mut p = dir.to_path_buf();
    p.push(format!("{}.json", code));
    p
}

fn base64url_sha256(s: &str) -> String {
    let mut h = sha2::Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(out)
}

async fn google_auth_start(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(code) = q.get("code").map(|s| s.as_str()) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "missing code
",
        )
            .into_response();
    };

    let (Some(client_id), Some(redirect_uri)) = (
        st.oauth.client_id.as_deref(),
        st.oauth.redirect_uri.as_deref(),
    ) else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "google sso not configured
",
        )
            .into_response();
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let path = pending_path(&st.oauth.dir, code);
    let verifier = match std::fs::read_to_string(&path) {
        Ok(s) => {
            let pending: GoogleOAuthPending = match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        "bad oauth record
",
                    )
                        .into_response();
                }
            };
            if pending.status != "pending" {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    "oauth already completed
",
                )
                    .into_response();
            }
            pending.verifier
        }
        Err(_) => {
            // Web flow: pending state stored in-memory (keyed by a random state code).
            let pending = { st.oauth.web_pending_google.lock().await.get(code).cloned() };
            let Some(p) = pending else {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    "unknown code
",
                )
                    .into_response();
            };
            if now_unix.saturating_sub(p.created_unix) > 15 * 60 {
                let mut m = st.oauth.web_pending_google.lock().await;
                m.remove(code);
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    "oauth expired
",
                )
                    .into_response();
            }
            p.verifier
        }
    };

    let challenge = base64url_sha256(&verifier);

    // Build Google OAuth URL.
    let scope = urlencoding::encode("openid email profile");
    let client_id = urlencoding::encode(client_id);
    let redirect_uri = urlencoding::encode(redirect_uri);
    let state = urlencoding::encode(code);
    let challenge = urlencoding::encode(&challenge);

    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id={client_id}&redirect_uri={redirect_uri}&scope={scope}&state={state}&code_challenge={challenge}&code_challenge_method=S256&prompt=select_account"
    );

    Redirect::temporary(&url).into_response()
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GoogleCallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

async fn google_auth_callback(
    State(st): State<AppState>,
    Query(q): Query<GoogleCallbackQuery>,
) -> impl IntoResponse {
    let Some(state_code) = q.state.as_deref() else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "missing state
",
        )
            .into_response();
    };

    let path = pending_path(&st.oauth.dir, state_code);

    if let Some(err) = q.error.as_deref() {
        let msg = q
            .error_description
            .clone()
            .unwrap_or_else(|| err.to_string());
        let _ = write_pending_err(&path, &msg);
        return Html(format!(
            "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
            html_escape(&msg)
        ))
        .into_response();
    }

    let Some(code) = q.code.as_deref() else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "missing code
",
        )
            .into_response();
    };

    let (Some(client_id), Some(client_secret), Some(redirect_uri)) = (
        st.oauth.client_id.as_deref(),
        st.oauth.client_secret.as_deref(),
        st.oauth.redirect_uri.as_deref(),
    ) else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "google sso not configured
",
        )
            .into_response();
    };

    let pending_s = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "unknown state
",
            )
                .into_response();
        }
    };
    let pending: GoogleOAuthPending = match serde_json::from_str(&pending_s) {
        Ok(v) => v,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "bad oauth record
",
            )
                .into_response();
        }
    };
    if pending.status != "pending" {
        return Html(
            "<h1>Already completed</h1><p>Return to the game and type <code>check</code>.</p>"
                .to_string(),
        )
        .into_response();
    }

    let http = reqwest::Client::new();

    let token = match http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", redirect_uri),
            ("code", code),
            ("code_verifier", pending.verifier.as_str()),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("token exchange failed: {e}");
            let _ = write_pending_err(&path, &msg);
            return Html(format!(
                "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
                html_escape(&msg)
            ))
            .into_response();
        }
    };

    if !token.status().is_success() {
        let msg = format!("token exchange returned {}", token.status());
        let _ = write_pending_err(&path, &msg);
        return Html(format!(
            "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
            html_escape(&msg)
        ))
        .into_response();
    }

    let token: GoogleTokenResponse = match token.json().await {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("token response parse failed: {e}");
            let _ = write_pending_err(&path, &msg);
            return Html(format!(
                "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
                html_escape(&msg)
            ))
            .into_response();
        }
    };

    let userinfo = match http
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(&token.access_token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("userinfo request failed: {e}");
            let _ = write_pending_err(&path, &msg);
            return Html(format!(
                "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
                html_escape(&msg)
            ))
            .into_response();
        }
    };

    if !userinfo.status().is_success() {
        let msg = format!("userinfo returned {}", userinfo.status());
        let _ = write_pending_err(&path, &msg);
        return Html(format!(
            "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
            html_escape(&msg)
        ))
        .into_response();
    }

    let userinfo: GoogleUserInfo = match userinfo.json().await {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("userinfo parse failed: {e}");
            let _ = write_pending_err(&path, &msg);
            return Html(format!(
                "<h1>Google sign-in failed</h1><p>{}</p><p>Return to the game and type <code>check</code>.</p>",
                html_escape(&msg)
            ))
            .into_response();
        }
    };

    let email = userinfo.email.filter(|e| e.len() <= 254);
    if let Err(e) = write_pending_ok(&path, &userinfo.sub, email.as_deref()) {
        warn!(err = %e, "failed writing oauth pending ok");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to write oauth result
",
        )
            .into_response();
    }

    Html("<h1>Signed in</h1><p>Return to the game and type <code>check</code>.</p>".to_string())
        .into_response()
}

fn write_pending_ok(path: &Path, sub: &str, email: Option<&str>) -> anyhow::Result<()> {
    let s = std::fs::read_to_string(path)?;
    let mut pending: GoogleOAuthPending = serde_json::from_str(&s)?;

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    pending.status = "ok".to_string();
    pending.updated_unix = Some(now_unix);
    pending.google_sub = Some(sub.to_string());
    pending.google_email = email.map(|s| s.to_string());
    pending.error = None;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&pending)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn write_pending_err(path: &Path, msg: &str) -> anyhow::Result<()> {
    let s = std::fs::read_to_string(path)?;
    let mut pending: GoogleOAuthPending = serde_json::from_str(&s)?;

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    pending.status = "err".to_string();
    pending.updated_unix = Some(now_unix);
    pending.google_sub = None;
    pending.google_email = None;
    pending.error = Some(msg.to_string());

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&pending)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn html_escape(s: &str) -> String {
    // Enough for our minimal status pages.
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const WEB_SESSION_IDLE_TTL_DEFAULT: Duration = Duration::from_secs(10 * 60);
const WEB_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(60);
const WEB_SESSION_BUFFER_MAX_BYTES: usize = 64 * 1024;

fn web_session_idle_ttl_from_env() -> Duration {
    // Keep sessions alive across page reloads / short disconnects, but don't leave
    // "link-dead" characters around forever.
    std::env::var("WEB_SESSION_IDLE_TTL_S")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(WEB_SESSION_IDLE_TTL_DEFAULT)
}

fn is_valid_resume_token(t: &str) -> bool {
    let s = t.trim();
    if s.len() < 16 || s.len() > 128 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

async fn connect_session_tcp(
    session_tcp_addr: SocketAddr,
    peer: SocketAddr,
) -> anyhow::Result<tokio::net::TcpStream> {
    let mut stream = tokio::net::TcpStream::connect(session_tcp_addr).await?;

    // Pass the real client IP to the broker via PROXY protocol v1.
    // The broker only trusts PROXY headers from loopback peers.
    let src_ip = peer.ip();
    let (family, dst_ip): (&str, IpAddr) = match src_ip {
        IpAddr::V4(_) => (
            "TCP4",
            match session_tcp_addr.ip() {
                IpAddr::V4(v4) => IpAddr::V4(v4),
                IpAddr::V6(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            },
        ),
        IpAddr::V6(_) => (
            "TCP6",
            match session_tcp_addr.ip() {
                IpAddr::V6(v6) => IpAddr::V6(v6),
                IpAddr::V4(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            },
        ),
    };
    let proxy_line = format!(
        "PROXY {family} {src} {dst} {sport} {dport}\r\n",
        src = src_ip,
        dst = dst_ip,
        sport = peer.port(),
        dport = session_tcp_addr.port()
    );
    tokio::io::AsyncWriteExt::write_all(&mut stream, proxy_line.as_bytes()).await?;

    Ok(stream)
}

#[derive(Clone)]
struct WebSessionManager {
    sessions: Arc<tokio::sync::Mutex<HashMap<String, Arc<WebSession>>>>,
    idle_ttl: Duration,
    pending_tcp_inputs: Arc<tokio::sync::Mutex<HashMap<String, Vec<Vec<u8>>>>>,
}

impl WebSessionManager {
    fn new(idle_ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            idle_ttl,
            pending_tcp_inputs: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    async fn get_or_create(
        &self,
        token: String,
        session_tcp_addr: SocketAddr,
        peer: SocketAddr,
    ) -> anyhow::Result<Arc<WebSession>> {
        if let Some(existing) = { self.sessions.lock().await.get(&token).cloned() } {
            if existing.is_alive().await {
                return Ok(existing);
            }

            // Drop dead sessions eagerly so the token can start fresh.
            {
                let mut m = self.sessions.lock().await;
                if m.get(&token).is_some_and(|cur| Arc::ptr_eq(cur, &existing)) {
                    m.remove(&token);
                }
            }
            existing.shutdown();
        }

        let created = WebSession::connect(token.clone(), session_tcp_addr, peer).await?;

        // Insert (unless someone raced us).
        {
            let mut m = self.sessions.lock().await;
            if let Some(raced) = m.get(&token).cloned() {
                created.shutdown();
                return Ok(raced);
            }
            m.insert(token, created.clone());
        }

        // Flush any queued TCP inputs (for example: pre-auth).
        let pending = {
            let mut m = self.pending_tcp_inputs.lock().await;
            m.remove(&token).unwrap_or_default()
        };
        for b in pending {
            let _ = created.tcp_tx.send(b).await;
        }

        Ok(created)
    }

    async fn sweep_once(&self) {
        let items = {
            let m = self.sessions.lock().await;
            m.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        };

        let mut remove = Vec::new();
        for (token, sess) in &items {
            let st = sess.state.lock().await;
            let idle = st.attached.is_none() && st.last_detached.elapsed() >= self.idle_ttl;
            let dead = !st.tcp_alive && st.attached.is_none();
            if idle || dead {
                remove.push((token.clone(), sess.clone()));
            }
        }

        if remove.is_empty() {
            return;
        }

        for (token, sess) in remove {
            let mut m = self.sessions.lock().await;
            if m.get(&token).is_some_and(|cur| Arc::ptr_eq(cur, &sess)) {
                m.remove(&token);
                drop(m);
                sess.shutdown();
            }
        }
    }

    async fn sweep_loop(self) {
        loop {
            tokio::time::sleep(WEB_SESSION_SWEEP_INTERVAL).await;
            self.sweep_once().await;
        }
    }

    async fn logout(&self, token: &str) -> bool {
        let token = token.trim();
        if !is_valid_resume_token(token) {
            return false;
        }

        {
            let mut m = self.pending_tcp_inputs.lock().await;
            m.remove(token);
        }

        let removed = {
            let mut m = self.sessions.lock().await;
            m.remove(token)
        };
        if let Some(sess) = removed {
            sess.shutdown();
            true
        } else {
            false
        }
    }

    async fn send_or_defer(&self, token: &str, b: Vec<u8>) -> bool {
        let token = token.trim();
        if !is_valid_resume_token(token) {
            return false;
        }
        if b.is_empty() {
            return true;
        }

        if let Some(sess) = { self.sessions.lock().await.get(token).cloned() } {
            let _ = sess.tcp_tx.send(b).await;
            return true;
        }

        let mut m = self.pending_tcp_inputs.lock().await;
        m.entry(token.to_string()).or_default().push(b);
        true
    }
}

struct WebSession {
    tcp_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    state: tokio::sync::Mutex<WebSessionState>,
}

struct WebSessionState {
    attached: Option<AttachedWs>,
    attach_seq: u64,
    last_detached: Instant,
    buffer: VecDeque<Vec<u8>>,
    buffer_bytes: usize,
    tcp_alive: bool,
}

struct AttachedWs {
    id: u64,
    ws_tx: tokio::sync::mpsc::Sender<ws::Message>,
    cancel: CancellationToken,
}

impl WebSessionState {
    fn buffer_push(&mut self, mut b: Vec<u8>) {
        if b.is_empty() {
            return;
        }

        // Clamp oversized chunks.
        if b.len() > WEB_SESSION_BUFFER_MAX_BYTES {
            b = b.split_off(b.len() - WEB_SESSION_BUFFER_MAX_BYTES);
        }

        self.buffer_bytes = self.buffer_bytes.saturating_add(b.len());
        self.buffer.push_back(b);

        while self.buffer_bytes > WEB_SESSION_BUFFER_MAX_BYTES {
            let Some(front) = self.buffer.pop_front() else {
                self.buffer_bytes = 0;
                break;
            };
            self.buffer_bytes = self.buffer_bytes.saturating_sub(front.len());
        }
    }

    fn buffer_take_all(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(b) = self.buffer.pop_front() {
            out.push(b);
        }
        self.buffer_bytes = 0;
        out
    }
}

impl WebSession {
    async fn connect(
        _token: String,
        session_tcp_addr: SocketAddr,
        peer: SocketAddr,
    ) -> anyhow::Result<Arc<Self>> {
        let stream = connect_session_tcp(session_tcp_addr, peer).await?;
        let (mut tcp_r, mut tcp_w) = stream.into_split();

        let (tcp_tx, mut tcp_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let sess = Arc::new(Self {
            tcp_tx: tcp_tx.clone(),
            shutdown_tx: shutdown_tx.clone(),
            state: tokio::sync::Mutex::new(WebSessionState {
                attached: None,
                attach_seq: 0,
                last_detached: Instant::now(),
                buffer: VecDeque::new(),
                buffer_bytes: 0,
                tcp_alive: true,
            }),
        });

        // TCP writer.
        {
            let sess = sess.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() { break; }
                        }
                        v = tcp_rx.recv() => {
                            let Some(b) = v else { break; };
                            if tokio::io::AsyncWriteExt::write_all(&mut tcp_w, &b).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                sess.mark_tcp_dead().await;
            });
        }

        // TCP reader.
        {
            let sess = sess.clone();
            let tcp_tx_telnet = tcp_tx.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                let mut telnet = IacParser::new();
                let mut buf = [0u8; 4096];
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() { break; }
                        }
                        res = tokio::io::AsyncReadExt::read(&mut tcp_r, &mut buf) => {
                            let n = match res {
                                Ok(n) => n,
                                Err(_) => 0,
                            };
                            if n == 0 {
                                break;
                            }
                            let (data, reply) = telnet.parse(&buf[..n]);
                            if !reply.is_empty() {
                                let _ = tcp_tx_telnet.send(reply).await;
                            }
                            if !data.is_empty() {
                                sess.deliver_output(data).await;
                            }
                        }
                    }
                }

                sess.mark_tcp_dead().await;
                let _ = shutdown_tx.send(true);
            });
        }

        Ok(sess)
    }

    async fn is_alive(&self) -> bool {
        self.state.lock().await.tcp_alive
    }

    fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        // Also kick any attached websocket.
        // Best-effort: if we can't lock immediately, the sweeper will retry later.
        if let Ok(mut st) = self.state.try_lock() {
            if let Some(att) = st.attached.take() {
                att.cancel.cancel();
            }
        }
    }

    async fn mark_tcp_dead(&self) {
        let (cancel_opt, ws_tx_opt) = {
            let mut st = self.state.lock().await;
            if !st.tcp_alive {
                return;
            }
            st.tcp_alive = false;
            let att = st.attached.take();
            st.last_detached = Instant::now();
            match att {
                Some(att) => (Some(att.cancel), Some(att.ws_tx)),
                None => (None, None),
            }
        };

        if let Some(ws_tx) = ws_tx_opt {
            let _ = ws_tx
                .send(ws::Message::Text("ERROR: session closed\r\n".to_string()))
                .await;
        }
        if let Some(c) = cancel_opt {
            c.cancel();
        }
    }

    async fn deliver_output(&self, data: Vec<u8>) {
        let ws_tx = {
            let mut st = self.state.lock().await;
            if let Some(att) = st.attached.as_ref() {
                att.ws_tx.clone()
            } else {
                st.buffer_push(data);
                return;
            }
        };
        let _ = ws_tx.send(ws::Message::Binary(data)).await;
    }

    async fn detach_ws(&self, attach_id: u64) {
        let mut st = self.state.lock().await;
        if st.attached.as_ref().is_some_and(|a| a.id == attach_id) {
            st.attached = None;
            st.last_detached = Instant::now();
        }
    }

    async fn attach(self: Arc<Self>, socket: ws::WebSocket) {
        if !self.is_alive().await {
            // Should be rare; usually the manager will replace dead sessions on connect.
            let mut socket = socket;
            let _ = socket
                .send(ws::Message::Text("ERROR: session expired\r\n".to_string()))
                .await;
            let _ = socket.close().await;
            return;
        }

        let (mut ws_w, mut ws_r) = socket.split();
        let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<ws::Message>(64);

        let cancel = CancellationToken::new();

        let (attach_id, prev_cancel, buffered) = {
            let mut st = self.state.lock().await;
            let prev_cancel = st.attached.take().map(|a| a.cancel);
            st.attach_seq = st.attach_seq.saturating_add(1);
            let attach_id = st.attach_seq;
            let buffered = st.buffer_take_all();
            st.attached = Some(AttachedWs {
                id: attach_id,
                ws_tx: ws_tx.clone(),
                cancel: cancel.clone(),
            });
            (attach_id, prev_cancel, buffered)
        };

        if let Some(prev) = prev_cancel {
            prev.cancel();
        }

        let writer_cancel = cancel.clone();
        let mut writer = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = writer_cancel.cancelled() => { break; }
                    m = ws_rx.recv() => {
                        let Some(m) = m else { break; };
                        if ws_w.send(m).await.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = ws_w.close().await;
        });

        let reader_cancel = cancel.clone();
        let tcp_tx = self.tcp_tx.clone();
        let ws_tx_pongs = ws_tx.clone();
        let mut reader = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = reader_cancel.cancelled() => { break; }
                    msg = ws_r.next() => {
                        let Some(msg) = msg else { break; };
                        match msg {
                            Ok(ws::Message::Text(s)) => {
                                let _ = tcp_tx.send(s.into_bytes()).await;
                            }
                            Ok(ws::Message::Binary(b)) => {
                                let _ = tcp_tx.send(b).await;
                            }
                            Ok(ws::Message::Close(_)) => break,
                            Ok(ws::Message::Ping(v)) => {
                                let _ = ws_tx_pongs.send(ws::Message::Pong(v)).await;
                            }
                            Ok(ws::Message::Pong(_)) => {}
                            Err(_) => break,
                        }
                    }
                }
            }
        });

        // Flush buffered output (if any).
        for b in buffered {
            if ws_tx.send(ws::Message::Binary(b)).await.is_err() {
                break;
            }
        }

        tokio::select! {
            _ = &mut writer => {
                // `JoinHandle` must not be polled again after completion. `select!` already
                // polled this handle to completion, so we must not `.await` it below.
                cancel.cancel();
                let _ = reader.await;
            },
            _ = &mut reader => {
                cancel.cancel();
                let _ = writer.await;
            },
        }
        self.detach_ws(attach_id).await;
    }
}

async fn ws_session_task(
    mut socket: ws::WebSocket,
    state: AppState,
    peer: SocketAddr,
    resume: Option<String>,
) {
    let token = resume
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && is_valid_resume_token(s))
        .map(|s| s.to_string());

    if let Some(token) = token {
        match state
            .web_sessions
            .get_or_create(token, state.session_tcp_addr, peer)
            .await
        {
            Ok(sess) => {
                sess.attach(socket).await;
            }
            Err(e) => {
                let _ = socket
                    .send(ws::Message::Text(format!("ERROR: {e}\n")))
                    .await;
                let _ = socket.close().await;
            }
        }
        return;
    }

    ws_session_task_ephemeral(socket, state, peer).await;
}

async fn ws_session_task_ephemeral(mut socket: ws::WebSocket, state: AppState, peer: SocketAddr) {
    let stream = match connect_session_tcp(state.session_tcp_addr, peer).await {
        Ok(s) => s,
        Err(e) => {
            let _ = socket
                .send(ws::Message::Text(format!(
                    "ERROR: failed to connect to session tcp {}: {e}\n",
                    state.session_tcp_addr
                )))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    let (mut tcp_r, mut tcp_w) = stream.into_split();
    let (mut ws_w, mut ws_r) = socket.split();
    let telnet = IacParser::new();

    let (tcp_tx, mut tcp_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let (ws_tx, mut ws_rx) = tokio::sync::mpsc::channel::<ws::Message>(32);

    let tcp_writer = tokio::spawn(async move {
        while let Some(b) = tcp_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut tcp_w, &b).await?;
        }
        Ok::<(), std::io::Error>(())
    });

    let ws_writer = tokio::spawn(async move {
        while let Some(m) = ws_rx.recv().await {
            ws_w.send(m).await.map_err(std::io::Error::other)?;
        }
        Ok::<(), std::io::Error>(())
    });

    let tcp_tx_tcp = tcp_tx.clone();
    let ws_tx_tcp = ws_tx.clone();
    let mut tcp_reader = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut telnet = telnet;
        loop {
            let n = tokio::io::AsyncReadExt::read(&mut tcp_r, &mut buf).await?;
            if n == 0 {
                break;
            }
            let (data, reply) = telnet.parse(&buf[..n]);
            if !reply.is_empty() {
                let _ = tcp_tx_tcp.send(reply).await;
            }
            if !data.is_empty() {
                let _ = ws_tx_tcp.send(ws::Message::Binary(data)).await;
            }
        }
        Ok::<(), std::io::Error>(())
    });

    let tcp_tx_ws = tcp_tx.clone();
    let ws_tx_ws = ws_tx.clone();
    let mut ws_reader = tokio::spawn(async move {
        while let Some(msg) = ws_r.next().await {
            match msg.map_err(std::io::Error::other)? {
                ws::Message::Text(s) => {
                    let _ = tcp_tx_ws.send(s.into_bytes()).await;
                }
                ws::Message::Binary(b) => {
                    let _ = tcp_tx_ws.send(b).await;
                }
                ws::Message::Close(_) => break,
                ws::Message::Ping(v) => {
                    let _ = ws_tx_ws.send(ws::Message::Pong(v)).await;
                }
                ws::Message::Pong(_) => {}
            }
        }
        Ok::<(), std::io::Error>(())
    });

    tokio::select! {
        _ = &mut tcp_reader => {},
        _ = &mut ws_reader => {},
    }

    tcp_reader.abort();
    ws_reader.abort();
    drop(tcp_tx);
    drop(ws_tx);

    let _ = tcp_writer.await;
    let _ = ws_writer.await;
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = parse_args();

    // rustls 0.23: if multiple crypto providers are enabled (ring + aws-lc-rs),
    // the application must pick one process-wide provider before any config builders run.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let https_enabled = cfg.https_bind.is_some();
    if https_enabled && (cfg.tls_cert.is_none() || cfg.tls_key.is_none()) {
        eprintln!("ERROR: HTTPS_BIND set but TLS_CERT/TLS_KEY not set");
        std::process::exit(2);
    }

    let compliance = match compliance_portal::ComplianceState::from_env().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: compliance portal init failed: {e:?}");
            std::process::exit(2);
        }
    };

    let state = AppState {
        session_tcp_addr: cfg.session_tcp_addr,
        admin_tcp_addr: cfg.admin_tcp_addr,
        web_sessions: WebSessionManager::new(web_session_idle_ttl_from_env()),
        oauth: OAuthState {
            dir: cfg.google_oauth_dir.clone(),
            client_id: cfg.google_client_id.clone(),
            client_secret: cfg.google_client_secret.clone(),
            redirect_uri: cfg.google_redirect_uri.clone(),
            web_pending_google: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            web_identities: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        },
        compliance,
    };

    // Keep resumable web sessions alive across short disconnects / page reload.
    tokio::spawn(state.web_sessions.clone().sweep_loop());

    let service = ServeDir::new(&cfg.static_dir);
    let app_https = Router::new()
        .route(
            "/healthz",
            get(|| async {
                "ok
"
            }),
        )
        .route("/api/online", get(api_online))
        .route("/api/ws/logout", axum::routing::post(api_ws_logout))
        .route("/ws", get(ws_session))
        .route("/auth/google", get(google_auth_start))
        .route("/auth/google/callback", get(google_auth_callback))
        .merge(compliance_portal::router())
        .with_state(state.clone())
        .fallback_service(service)
        .layer(TraceLayer::new_for_http());

    let app_http = if https_enabled {
        Router::new()
            .route(
                "/healthz",
                get(|| async {
                    "ok
"
                }),
            )
            .fallback(redirect_to_https)
            .layer(TraceLayer::new_for_http())
    } else {
        app_https.clone()
    };

    info!(
        http_bind = ?cfg.http_bind,
        https_bind = ?cfg.https_bind,
        static_dir = %cfg.static_dir.display(),
        google_oauth_dir = %cfg.google_oauth_dir.display(),
        "starting slopmud_web"
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
        info!("shutdown signal received");
    });

    let mut joins = Vec::new();
    {
        let listener = tokio::net::TcpListener::bind(cfg.http_bind)
            .await
            .expect("http bind failed");
        let rx = shutdown_rx.clone();
        joins.push(tokio::spawn(async move {
            axum::serve(
                listener,
                app_http.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(wait_for_shutdown(rx))
            .await
            .expect("http server failed");
        }));
    }

    if let (Some(https_bind), Some(cert), Some(key)) =
        (cfg.https_bind, cfg.tls_cert.as_ref(), cfg.tls_key.as_ref())
    {
        let rustls = RustlsConfig::from_pem_file(cert, key)
            .await
            .expect("invalid TLS_CERT/TLS_KEY");
        let rx = shutdown_rx.clone();
        joins.push(tokio::spawn(async move {
            let handle = axum_server::Handle::new();

            {
                let handle = handle.clone();
                tokio::spawn(async move {
                    wait_for_shutdown(rx).await;
                    handle.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
                });
            }

            axum_server::bind_rustls(https_bind, rustls)
                .handle(handle)
                .serve(app_https.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .expect("https server failed");
        }));
    }

    for j in joins {
        let _ = j.await;
    }
}

async fn wait_for_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}
