use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::warn;

#[derive(Clone, Debug)]
pub struct NearlineConfig {
    pub enabled: bool,
    pub dir: PathBuf,
    pub max_segments: usize,
    pub segment_max_bytes: u64,
    pub channel_capacity: usize,
}

impl Default for NearlineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: PathBuf::from("locks/nearline_scrollback"),
            max_segments: 12,
            segment_max_bytes: 2_000_000,
            channel_capacity: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearlineRecord {
    pub v: u8,
    pub id: String,
    pub ts_unix_ms: u64,
    pub kind: String, // "output" | "err"
    pub session: String,
    pub name: String,
    pub ip: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetaFile {
    v: u8,
    cur_seg: usize,
}

impl Default for MetaFile {
    fn default() -> Self {
        Self { v: 1, cur_seg: 0 }
    }
}

#[derive(Clone)]
pub struct NearlineRing {
    cfg: NearlineConfig,
    tx: Option<mpsc::Sender<NearlineRecord>>,
}

impl NearlineRing {
    pub async fn new(cfg: NearlineConfig) -> Self {
        if !cfg.enabled {
            return Self { cfg, tx: None };
        }

        if let Err(e) = tokio::fs::create_dir_all(&cfg.dir).await {
            warn!(err=%e, dir=%cfg.dir.display(), "nearline mkdir failed; disabling");
            return Self { cfg, tx: None };
        }

        let (tx, rx) = mpsc::channel::<NearlineRecord>(cfg.channel_capacity.max(64));
        tokio::spawn(writer_task(cfg.clone(), rx));
        Self { cfg, tx: Some(tx) }
    }

    pub fn try_append(&self, rec: NearlineRecord) {
        let Some(tx) = self.tx.as_ref() else {
            return;
        };
        let _ = tx.try_send(rec);
    }

    pub async fn last_n(&self, name: &str, n: usize) -> Vec<NearlineRecord> {
        if n == 0 {
            return Vec::new();
        }
        let Some(cur_seg) = read_cur_seg(&self.cfg.dir).await else {
            return Vec::new();
        };
        let max = self.cfg.max_segments.max(1);
        let name = name.trim().to_string();
        if name.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::new();
        for seg in newest_to_oldest(cur_seg, max) {
            let path = seg_path(&self.cfg.dir, seg);
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            let s = String::from_utf8_lossy(&bytes);
            for line in s.lines().rev() {
                let Ok(r) = serde_json::from_str::<NearlineRecord>(line) else {
                    continue;
                };
                if r.name != name {
                    continue;
                }
                out.push(r);
                if out.len() >= n {
                    out.reverse();
                    return out;
                }
            }
        }

        out.reverse();
        out
    }

    pub async fn search(&self, name: &str, q: &str, limit: usize) -> Vec<NearlineRecord> {
        let q = q.trim();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let Some(cur_seg) = read_cur_seg(&self.cfg.dir).await else {
            return Vec::new();
        };
        let max = self.cfg.max_segments.max(1);
        let name = name.trim().to_string();
        if name.is_empty() {
            return Vec::new();
        }

        let q_lc = q.to_ascii_lowercase();
        let mut out = Vec::new();
        for seg in newest_to_oldest(cur_seg, max) {
            let path = seg_path(&self.cfg.dir, seg);
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            let s = String::from_utf8_lossy(&bytes);
            for line in s.lines().rev() {
                let Ok(r) = serde_json::from_str::<NearlineRecord>(line) else {
                    continue;
                };
                if r.name != name {
                    continue;
                }
                if r.text.to_ascii_lowercase().contains(&q_lc) {
                    out.push(r);
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    pub async fn find_with_context(
        &self,
        name: &str,
        id: &str,
        ctx: usize,
    ) -> Option<(NearlineRecord, Vec<NearlineRecord>)> {
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        let Some(cur_seg) = read_cur_seg(&self.cfg.dir).await else {
            return None;
        };
        let max = self.cfg.max_segments.max(1);
        let name = name.trim().to_string();
        if name.is_empty() {
            return None;
        }

        let mut prev = VecDeque::<NearlineRecord>::with_capacity(ctx + 1);
        let mut found: Option<NearlineRecord> = None;
        let mut context: Vec<NearlineRecord> = Vec::new();
        let mut after_needed = 0usize;

        for seg in oldest_to_newest(cur_seg, max) {
            let path = seg_path(&self.cfg.dir, seg);
            let Ok(bytes) = tokio::fs::read(&path).await else {
                continue;
            };
            let s = String::from_utf8_lossy(&bytes);
            for line in s.lines() {
                let Ok(r) = serde_json::from_str::<NearlineRecord>(line) else {
                    continue;
                };
                if r.name != name {
                    continue;
                }

                if let Some(_target) = found.as_ref() {
                    if after_needed > 0 {
                        context.push(r);
                        after_needed -= 1;
                        if after_needed == 0 {
                            let target = found.expect("target");
                            return Some((target, context));
                        }
                    }
                    continue;
                }

                if r.id == id {
                    let target = r.clone();
                    found = Some(target.clone());

                    // Build context: prev + target, then capture next ctx records.
                    context.extend(prev.drain(..));
                    context.push(target);
                    after_needed = ctx;
                    if after_needed == 0 {
                        let target = found.expect("target");
                        return Some((target, context));
                    }
                    continue;
                }

                if ctx > 0 {
                    prev.push_back(r);
                    while prev.len() > ctx {
                        let _ = prev.pop_front();
                    }
                }
            }
        }

        None
    }
}

async fn writer_task(cfg: NearlineConfig, mut rx: mpsc::Receiver<NearlineRecord>) {
    let max_segments = cfg.max_segments.max(1);
    let seg_max = cfg.segment_max_bytes.max(256);

    let mut meta = read_meta_file(&cfg.dir).await.unwrap_or_default();
    if meta.cur_seg >= max_segments {
        meta.cur_seg = 0;
    }
    write_meta_file(&cfg.dir, &meta).await;

    let mut cur_seg = meta.cur_seg;
    let mut file = match open_segment_append(&cfg.dir, cur_seg).await {
        Ok(v) => v,
        Err(e) => {
            warn!(err=%e, dir=%cfg.dir.display(), "nearline open failed; writer exiting");
            return;
        }
    };
    let mut cur_bytes = segment_len(&cfg.dir, cur_seg).await.unwrap_or(0);

    while let Some(rec) = rx.recv().await {
        let mut line = match serde_json::to_string(&rec) {
            Ok(s) => s,
            Err(_) => continue,
        };
        line.push('\n');
        let bytes = line.as_bytes();

        if cur_bytes.saturating_add(bytes.len() as u64) > seg_max && cur_bytes > 0 {
            cur_seg = (cur_seg + 1) % max_segments;
            match open_segment_truncate(&cfg.dir, cur_seg).await {
                Ok(f) => {
                    file = f;
                    cur_bytes = 0;
                    meta.cur_seg = cur_seg;
                    write_meta_file(&cfg.dir, &meta).await;
                }
                Err(e) => {
                    warn!(err=%e, seg=cur_seg, "nearline rotate failed; dropping record");
                    continue;
                }
            }
        }

        if let Err(e) = file.write_all(bytes).await {
            warn!(err=%e, seg=cur_seg, "nearline write failed");
            continue;
        }
        cur_bytes = cur_bytes.saturating_add(bytes.len() as u64);
    }
}

fn meta_path(dir: &Path) -> PathBuf {
    let mut p = dir.to_path_buf();
    p.push("meta.json");
    p
}

fn seg_path(dir: &Path, seg: usize) -> PathBuf {
    let mut p = dir.to_path_buf();
    p.push(format!("seg-{seg:04}.jsonl"));
    p
}

fn newest_to_oldest(cur: usize, max: usize) -> impl Iterator<Item = usize> {
    (0..max).map(move |i| (cur + max - (i % max)) % max)
}

fn oldest_to_newest(cur: usize, max: usize) -> impl Iterator<Item = usize> {
    // The segment immediately after `cur` (wrapping) is the oldest.
    (0..max).map(move |i| (cur + 1 + i) % max)
}

async fn read_cur_seg(dir: &Path) -> Option<usize> {
    Some(read_meta_file(dir).await.unwrap_or_default().cur_seg)
}

async fn read_meta_file(dir: &Path) -> Option<MetaFile> {
    let p = meta_path(dir);
    let bytes = tokio::fs::read(&p).await.ok()?;
    serde_json::from_slice::<MetaFile>(&bytes).ok()
}

async fn write_meta_file(dir: &Path, meta: &MetaFile) {
    let p = meta_path(dir);
    let tmp = p.with_extension("json.tmp");
    let bytes = match serde_json::to_vec(meta) {
        Ok(b) => b,
        Err(_) => return,
    };
    if tokio::fs::write(&tmp, bytes).await.is_ok() {
        let _ = tokio::fs::rename(&tmp, &p).await;
    }
}

async fn open_segment_append(dir: &Path, seg: usize) -> std::io::Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(seg_path(dir, seg))
        .await
}

async fn open_segment_truncate(dir: &Path, seg: usize) -> std::io::Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(seg_path(dir, seg))
        .await
}

async fn segment_len(dir: &Path, seg: usize) -> Option<u64> {
    tokio::fs::metadata(seg_path(dir, seg))
        .await
        .ok()
        .map(|m| m.len())
}
