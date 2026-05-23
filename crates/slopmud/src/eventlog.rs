use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, Utc};
use compliance::{LogStream, object_relpath};
use slopmud_walbackup::daemon::{WalBackupDaemonClient, WalBackupDaemonHandle};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct EventLogConfig {
    pub enabled: bool,
    pub spool_dir: PathBuf,
    pub flush_interval_s: u64,

    pub s3_bucket: Option<String>,
    pub s3_prefix: String,
    pub upload_enabled: bool,
    pub upload_delete_local: bool,
    pub upload_scan_interval_s: u64,
}

impl Default for EventLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            spool_dir: PathBuf::from("locks/eventlog"),
            flush_interval_s: 60,

            s3_bucket: None,
            s3_prefix: "slopmud/eventlog".to_string(),
            upload_enabled: false,
            upload_delete_local: true,
            upload_scan_interval_s: 600,
        }
    }
}

pub struct EventLog {
    cfg: EventLogConfig,
    inner: Arc<Mutex<LocalWriter>>,
    upload_tx: Option<mpsc::Sender<Vec<String>>>,
    _upload_daemon: Option<WalBackupDaemonHandle>,
}

impl EventLog {
    pub async fn new(cfg: EventLogConfig) -> Self {
        let today = Utc::now().date_naive();
        let inner = Arc::new(Mutex::new(LocalWriter::new(
            cfg.spool_dir.clone(),
            today,
            cfg.flush_interval_s,
        )));

        let mut upload_tx = None;
        let mut upload_daemon = None;
        if cfg.upload_enabled {
            if cfg.s3_bucket.is_none() {
                warn!("eventlog upload enabled but missing s3 bucket; disabling upload");
            } else {
                match WalBackupDaemonHandle::spawn_from_env(std::iter::empty::<(&str, &str)>())
                    .await
                {
                    Ok(daemon) => {
                        let (tx, rx) = mpsc::channel::<Vec<String>>(64);
                        upload_tx = Some(tx.clone());

                        // Best-effort periodic scan for backlog uploads.
                        tokio::spawn(upload_scan_task(cfg.clone(), tx.clone()));

                        // Upload worker: the broker sends only path descriptors; walbackupd
                        // streams file bodies from disk to S3 in its own process.
                        tokio::spawn(upload_worker_task(daemon.client(), rx));
                        upload_daemon = Some(daemon);
                    }
                    Err(err) => {
                        warn!(err=%err, "eventlog upload child failed to start; disabling upload");
                    }
                }
            }
        }

        // Background flush.
        if cfg.enabled {
            let flush_cfg = cfg.clone();
            let flush_inner = inner.clone();
            tokio::spawn(async move {
                let d = Duration::from_secs(flush_cfg.flush_interval_s.max(1));
                loop {
                    tokio::time::sleep(d).await;
                    let mut w = flush_inner.lock().await;
                    if let Err(e) = w.flush_all() {
                        warn!(err=%e, "eventlog flush failed");
                    }
                }
            });
        }

        Self {
            cfg,
            inner,
            upload_tx,
            _upload_daemon: upload_daemon,
        }
    }

    pub async fn log_line(&self, stream: LogStream<'_>, line: &str) {
        if !self.cfg.enabled {
            return;
        }

        let now = Utc::now();
        let rel = object_relpath(stream, now);
        let date = now.date_naive();

        let rotated = {
            let mut w = self.inner.lock().await;
            w.write_line(date, &rel, line)
        };

        if let Some(rotated_paths) = rotated {
            if let Some(tx) = self.upload_tx.as_ref() {
                let _ = tx.send(rotated_paths).await;
            }
        }
    }

    pub fn public_s3_key(&self, relpath: &str) -> Option<(String, String)> {
        let bucket = self.cfg.s3_bucket.as_ref()?.clone();
        let key = join_prefix(&self.cfg.s3_prefix, relpath);
        Some((bucket, key))
    }

    pub fn spool_path(&self, relpath: &str) -> PathBuf {
        let mut p = self.cfg.spool_dir.clone();
        p.push(relpath);
        p
    }
}

struct LocalWriter {
    root: PathBuf,
    date: NaiveDate,
    open: HashMap<String, OpenFile>,
    touched: HashSet<String>,
    flush_interval: Duration,
    last_flush: Instant,
}

struct OpenFile {
    w: BufWriter<std::fs::File>,
    last_write: Instant,
}

impl LocalWriter {
    fn new(root: PathBuf, date: NaiveDate, flush_interval_s: u64) -> Self {
        Self {
            root,
            date,
            open: HashMap::new(),
            touched: HashSet::new(),
            flush_interval: Duration::from_secs(flush_interval_s.max(1)),
            last_flush: Instant::now(),
        }
    }

    fn rotate_if_needed(&mut self, date: NaiveDate) -> Option<Vec<String>> {
        if date == self.date {
            return None;
        }

        // Flush and close all file handles before rotating.
        let _ = self.flush_all();
        self.open.clear();

        let rotated = self.touched.drain().collect::<Vec<_>>();
        self.date = date;
        Some(rotated)
    }

    fn write_line(&mut self, date: NaiveDate, rel: &str, line: &str) -> Option<Vec<String>> {
        let rotated = self.rotate_if_needed(date);

        let now = Instant::now();
        if now.duration_since(self.last_flush) >= self.flush_interval {
            let _ = self.flush_all();
            self.last_flush = now;
        }

        let path = {
            let mut p = self.root.clone();
            p.push(rel);
            p
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(err=%e, dir=%parent.display(), "eventlog mkdir failed");
            }
        }

        let of = self.open.entry(rel.to_string()).or_insert_with(|| {
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap_or_else(|e| panic!("failed to open eventlog {:?}: {e}", path));
            OpenFile {
                w: BufWriter::new(f),
                last_write: now,
            }
        });

        if !line.ends_with('\n') {
            let _ = of.w.write_all(line.as_bytes());
            let _ = of.w.write_all(b"\n");
        } else {
            let _ = of.w.write_all(line.as_bytes());
        }
        of.last_write = now;
        self.touched.insert(rel.to_string());
        rotated
    }

    fn flush_all(&mut self) -> anyhow::Result<()> {
        for (_, of) in self.open.iter_mut() {
            of.w.flush()?;
        }
        Ok(())
    }
}

async fn upload_scan_task(cfg: EventLogConfig, tx: mpsc::Sender<Vec<String>>) {
    let interval = Duration::from_secs(cfg.upload_scan_interval_s.max(30));
    loop {
        if let Err(e) = scan_and_enqueue_backlog(&cfg, &tx).await {
            warn!(err=%e, "eventlog upload scan failed");
        }
        tokio::time::sleep(interval).await;
    }
}

async fn scan_and_enqueue_backlog(
    cfg: &EventLogConfig,
    tx: &mpsc::Sender<Vec<String>>,
) -> anyhow::Result<()> {
    let today = Utc::now().date_naive();
    let mut rels = Vec::new();
    scan_dir(
        cfg.spool_dir.as_path(),
        cfg.spool_dir.as_path(),
        today,
        &mut rels,
    )?;
    if !rels.is_empty() {
        let _ = tx.send(rels).await;
    }
    Ok(())
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    today: NaiveDate,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    for ent in std::fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        let ft = ent.file_type()?;
        if ft.is_dir() {
            scan_dir(root, &path, today, out)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let Some(rel) = path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string())
        else {
            continue;
        };
        if relpath_is_today(&rel, today) {
            continue;
        }
        out.push(rel);
    }
    Ok(())
}

fn relpath_is_today(rel: &str, today: NaiveDate) -> bool {
    // Layout: v1/<stream>/YYYY/MM/DD.log or v1/char/<name>/YYYY/MM/DD.log
    let parts = rel.split('/').collect::<Vec<_>>();
    if parts.len() < 5 {
        return false;
    }

    let (y_s, m_s, d_file) = (
        parts[parts.len() - 3],
        parts[parts.len() - 2],
        parts[parts.len() - 1],
    );
    let Some(d_stem) = d_file.strip_suffix(".log") else {
        return false;
    };

    let Ok(y) = y_s.parse::<i32>() else {
        return false;
    };
    let Ok(m) = m_s.parse::<u32>() else {
        return false;
    };
    let Ok(d) = d_stem.parse::<u32>() else {
        return false;
    };

    y == today.year() && m == today.month() && d == today.day()
}

async fn upload_worker_task(client: WalBackupDaemonClient, mut rx: mpsc::Receiver<Vec<String>>) {
    while let Some(batch) = rx.recv().await {
        if let Err(e) = client.enqueue_eventlog_uploads(batch).await {
            warn!(err=%e, "eventlog upload enqueue failed");
        }
    }
}

fn join_prefix(prefix: &str, rel: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        rel.trim_start_matches('/').to_string()
    } else {
        format!("{}/{}", prefix, rel.trim_start_matches('/'))
    }
}
