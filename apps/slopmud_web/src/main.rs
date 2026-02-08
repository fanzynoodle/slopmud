use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::{
    Router,
    extract::{Host, Query, State, ws},
    http::Uri,
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use slopio::telnet::IacParser;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{Level, info, warn};

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud_web

USAGE:
  slopmud_web [--bind HOST:PORT] [--dir PATH] [--https-bind HOST:PORT --tls-cert PATH --tls-key PATH] [--session-tcp-addr HOST:PORT]

ENV:
  BIND                     default 0.0.0.0:8080
  STATIC_DIR               default web_homepage
  HTTPS_BIND               optional
  TLS_CERT                 required if HTTPS_BIND set
  TLS_KEY                  required if HTTPS_BIND set
  SESSION_TCP_ADDR         default 127.0.0.1:23 (used by /ws)
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

    let mut https_bind: Option<SocketAddr> = std::env::var("HTTPS_BIND").ok().and_then(|v| v.parse().ok());

    let mut dir: PathBuf = std::env::var("STATIC_DIR")
        .unwrap_or_else(|_| "web_homepage".to_string())
        .into();

    let mut tls_cert: Option<PathBuf> = std::env::var("TLS_CERT").ok().map(Into::into);
    let mut tls_key: Option<PathBuf> = std::env::var("TLS_KEY").ok().map(Into::into);

    let mut session_tcp_addr: SocketAddr = std::env::var("SESSION_TCP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:23".to_string())
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
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| async move { ws_session_task(socket, state).await })
}

#[derive(Clone, Debug)]
struct AppState {
    session_tcp_addr: SocketAddr,
    oauth: OAuthState,
}

#[derive(Clone, Debug)]
struct OAuthState {
    dir: PathBuf,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
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
        return (axum::http::StatusCode::BAD_REQUEST, "missing code
").into_response();
    };

    let (Some(client_id), Some(redirect_uri)) = (st.oauth.client_id.as_deref(), st.oauth.redirect_uri.as_deref()) else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "google sso not configured
",
        )
            .into_response();
    };

    let path = pending_path(&st.oauth.dir, code);
    let s = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "unknown code
").into_response(),
    };
    let pending: GoogleOAuthPending = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "bad oauth record
").into_response(),
    };
    if pending.status != "pending" {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "oauth already completed
",
        )
            .into_response();
    }

    let challenge = base64url_sha256(&pending.verifier);

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

async fn google_auth_callback(State(st): State<AppState>, Query(q): Query<GoogleCallbackQuery>) -> impl IntoResponse {
    let Some(state_code) = q.state.as_deref() else {
        return (axum::http::StatusCode::BAD_REQUEST, "missing state
").into_response();
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
        return (axum::http::StatusCode::BAD_REQUEST, "missing code
").into_response();
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
        Err(_) => return (axum::http::StatusCode::NOT_FOUND, "unknown state
").into_response(),
    };
    let pending: GoogleOAuthPending = match serde_json::from_str(&pending_s) {
        Ok(v) => v,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "bad oauth record
").into_response(),
    };
    if pending.status != "pending" {
        return Html(
            "<h1>Already completed</h1><p>Return to the game and type <code>check</code>.</p>".to_string(),
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

    Html(
        "<h1>Signed in</h1><p>Return to the game and type <code>check</code>.</p>".to_string(),
    )
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
        .replace(''', "&#39;")
}

async fn ws_session_task(mut socket: ws::WebSocket, state: AppState) {
    let stream = match tokio::net::TcpStream::connect(state.session_tcp_addr).await {
        Ok(s) => s,
        Err(e) => {
            let _ = socket
                .send(ws::Message::Text(format!(
                    "ERROR: failed to connect to session tcp {}: {e}
",
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
            ws_w.send(m)
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
            match msg.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))? {
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

    let https_enabled = cfg.https_bind.is_some();
    if https_enabled && (cfg.tls_cert.is_none() || cfg.tls_key.is_none()) {
        eprintln!("ERROR: HTTPS_BIND set but TLS_CERT/TLS_KEY not set");
        std::process::exit(2);
    }

    let state = AppState {
        session_tcp_addr: cfg.session_tcp_addr,
        oauth: OAuthState {
            dir: cfg.google_oauth_dir.clone(),
            client_id: cfg.google_client_id.clone(),
            client_secret: cfg.google_client_secret.clone(),
            redirect_uri: cfg.google_redirect_uri.clone(),
        },
    };

    let service = ServeDir::new(&cfg.static_dir);
    let app_https = Router::new()
        .route("/healthz", get(|| async { "ok
" }))
        .route("/ws", get(ws_session))
        .route("/auth/google", get(google_auth_start))
        .route("/auth/google/callback", get(google_auth_callback))
        .with_state(state.clone())
        .fallback_service(service)
        .layer(TraceLayer::new_for_http());

    let app_http = if https_enabled {
        Router::new()
            .route("/healthz", get(|| async { "ok
" }))
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
            axum::serve(listener, app_http)
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
                .serve(app_https.into_make_service())
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
