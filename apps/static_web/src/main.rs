use std::net::SocketAddr;
use std::path::PathBuf;

use axum::{
    Router,
    extract::{Host, ws},
    http::Uri,
    response::Redirect,
    routing::get,
};
use axum_server::tls_rustls::RustlsConfig;
use futures_util::{SinkExt, StreamExt};
use slopio::telnet::IacParser;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{Level, info};

fn usage_and_exit() -> ! {
    eprintln!(
        "static_web\n\n\
USAGE:\n  static_web [--bind HOST:PORT] [--dir PATH] [--https-bind HOST:PORT --tls-cert PATH --tls-key PATH] [--session-tcp-addr HOST:PORT]\n\n\
ENV:\n  BIND             default 0.0.0.0:8080\n  STATIC_DIR       default web_homepage\n  HTTPS_BIND       optional\n  TLS_CERT         required if HTTPS_BIND set\n  TLS_KEY          required if HTTPS_BIND set\n  SESSION_TCP_ADDR default 127.0.0.1:23 (used by /ws)\n"
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
}

async fn ws_session_task(mut socket: ws::WebSocket, state: AppState) {
    let stream = match tokio::net::TcpStream::connect(state.session_tcp_addr).await {
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

    // Wait for either side to finish, then let the writer tasks drain/stop.
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
    let state = AppState {
        session_tcp_addr: cfg.session_tcp_addr,
    };

    let https_enabled = cfg.https_bind.is_some();
    if https_enabled && (cfg.tls_cert.is_none() || cfg.tls_key.is_none()) {
        eprintln!("ERROR: HTTPS_BIND set but TLS_CERT/TLS_KEY not set");
        std::process::exit(2);
    }

    let service = ServeDir::new(&cfg.static_dir);
    let app_https = Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route("/ws", get(ws_session))
        .with_state(state.clone())
        .fallback_service(service)
        .layer(TraceLayer::new_for_http());

    let app_http = if https_enabled {
        // If HTTPS is enabled, serve redirects on the HTTP port.
        Router::new()
            .route("/healthz", get(|| async { "ok\n" }))
            .fallback(redirect_to_https)
            .layer(TraceLayer::new_for_http())
    } else {
        app_https.clone()
    };

    info!(
        http_bind = ?cfg.http_bind,
        https_bind = ?cfg.https_bind,
        static_dir = %cfg.static_dir.display(),
        "starting static web server"
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = shutdown_tx.send(true);
        info!("shutdown signal received");
    });

    // HTTP server
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

    // HTTPS server (optional)
    if let (Some(https_bind), Some(cert), Some(key)) =
        (cfg.https_bind, cfg.tls_cert.as_ref(), cfg.tls_key.as_ref())
    {
        let rustls = RustlsConfig::from_pem_file(cert, key)
            .await
            .expect("invalid TLS_CERT/TLS_KEY");
        let rx = shutdown_rx.clone();
        joins.push(tokio::spawn(async move {
            let handle = axum_server::Handle::new();

            // Drive graceful shutdown via the handle (axum-server doesn't expose
            // a with_graceful_shutdown() helper on the returned future).
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
