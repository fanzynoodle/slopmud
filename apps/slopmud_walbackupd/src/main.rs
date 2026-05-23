use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};
use slopmud_walbackup::daemon;
use slopmud_walbackup::{
    EventLogUploadConfig, WalBackupConfig, WalRestoreConfig, restore_wal_from_backup,
    run_backup_loop, scan_eventlog_backlog, upload_eventlog_relpaths,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

#[derive(Default)]
struct ServerState {
    wal_backup_started: AtomicBool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,slopmud_walbackupd=info".into()),
        )
        .with_target(false)
        .init();

    let bind = parse_args()?;
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind walbackupd {bind}"))?;
    let addr = listener.local_addr()?;
    print!("{}", daemon::ready_line(addr));
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    info!(%addr, "walbackupd listening");

    let state = Arc::new(ServerState::default());
    loop {
        let (stream, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_conn(stream, state).await {
                warn!(%peer, err=%err, "walbackupd request failed");
            }
        });
    }
}

fn parse_args() -> anyhow::Result<String> {
    let mut serve = false;
    let mut bind = daemon::DEFAULT_BIND.to_string();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--serve" => serve = true,
            "--bind" => {
                bind = it
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--bind requires an address"))?;
            }
            "-h" | "--help" => {
                eprintln!("slopmud_walbackupd --serve [--bind 127.0.0.1:0]");
                std::process::exit(0);
            }
            _ => bail!("unknown argument {arg:?}"),
        }
    }
    if !serve {
        bail!("slopmud_walbackupd must be started with --serve");
    }
    Ok(bind)
}

async fn handle_conn(stream: TcpStream, state: Arc<ServerState>) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(());
    }

    let response = match handle_command(&line, state).await {
        Ok(CommandOutcome::Reply(payload)) => daemon::format_ok(&payload),
        Ok(CommandOutcome::Shutdown) => daemon::format_ok("bye"),
        Err(err) => daemon::format_err(&err),
    };
    write_half.write_all(response.as_bytes()).await?;
    write_half.shutdown().await?;

    if response == daemon::format_ok("bye") {
        std::process::exit(0);
    }
    Ok(())
}

enum CommandOutcome {
    Reply(String),
    Shutdown,
}

async fn handle_command(line: &str, state: Arc<ServerState>) -> anyhow::Result<CommandOutcome> {
    let fields = daemon::decode_command_fields(line)?;
    let Some(cmd) = fields.first().map(String::as_str) else {
        bail!("empty walbackupd command");
    };
    match cmd {
        "PING" => Ok(CommandOutcome::Reply("pong".to_string())),
        "RESTORE_WAL" => restore_wal().await,
        "START_WAL_BACKUP" => start_wal_backup(state).await,
        "EVENTLOG_UPLOAD" => {
            let relpaths = fields.into_iter().skip(1).collect::<Vec<_>>();
            queue_eventlog_upload(relpaths).await
        }
        "EVENTLOG_SCAN" => queue_eventlog_scan().await,
        "SHUTDOWN" => Ok(CommandOutcome::Shutdown),
        _ => bail!("unknown walbackupd command {cmd:?}"),
    }
}

async fn restore_wal() -> anyhow::Result<CommandOutcome> {
    let source = wal_source_path()?;
    let Some(cfg) = WalRestoreConfig::from_env(source)? else {
        bail!("WAL restore requested but SLOPMUD_WAL_RESTORE_ENABLED is not configured");
    };
    let outcome = restore_wal_from_backup(&cfg).await?;
    Ok(CommandOutcome::Reply(daemon::format_restore_outcome(
        &outcome,
    )))
}

async fn start_wal_backup(state: Arc<ServerState>) -> anyhow::Result<CommandOutcome> {
    if state.wal_backup_started.swap(true, Ordering::SeqCst) {
        return Ok(CommandOutcome::Reply("already_started".to_string()));
    }
    let source = wal_source_path()?;
    let node_id = std::env::var("SLOPMUD_WALD_NODE_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("SHARD_RAFT_NODE_ID").ok())
        .or_else(|| std::env::var("NODE_ID").ok())
        .unwrap_or_else(|| "node".to_string());
    let Some(cfg) = WalBackupConfig::from_env(source, node_id)? else {
        state.wal_backup_started.store(false, Ordering::SeqCst);
        return Ok(CommandOutcome::Reply("disabled".to_string()));
    };
    tokio::spawn(run_backup_loop(cfg));
    Ok(CommandOutcome::Reply("started".to_string()))
}

async fn queue_eventlog_upload(relpaths: Vec<String>) -> anyhow::Result<CommandOutcome> {
    if relpaths.is_empty() {
        return Ok(CommandOutcome::Reply("queued".to_string()));
    }
    let Some(cfg) = EventLogUploadConfig::from_env()? else {
        return Ok(CommandOutcome::Reply("disabled".to_string()));
    };
    tokio::spawn(async move {
        if let Err(err) = upload_eventlog_relpaths(&cfg, &relpaths).await {
            warn!(err=%err, "eventlog upload task failed");
        }
    });
    Ok(CommandOutcome::Reply("queued".to_string()))
}

async fn queue_eventlog_scan() -> anyhow::Result<CommandOutcome> {
    let Some(cfg) = EventLogUploadConfig::from_env()? else {
        return Ok(CommandOutcome::Reply("disabled".to_string()));
    };
    tokio::spawn(async move {
        match scan_eventlog_backlog(&cfg) {
            Ok(relpaths) if relpaths.is_empty() => {}
            Ok(relpaths) => {
                if let Err(err) = upload_eventlog_relpaths(&cfg, &relpaths).await {
                    warn!(err=%err, "eventlog scan upload task failed");
                }
            }
            Err(err) => warn!(err=%err, "eventlog scan failed"),
        }
    });
    Ok(CommandOutcome::Reply("queued".to_string()))
}

fn wal_source_path() -> anyhow::Result<std::path::PathBuf> {
    std::env::var("SLOPMUD_WALD_SOURCE_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("SHARD_RAFT_LOG").ok())
        .map(Into::into)
        .ok_or_else(|| anyhow::anyhow!("missing SLOPMUD_WALD_SOURCE_PATH or SHARD_RAFT_LOG"))
}
