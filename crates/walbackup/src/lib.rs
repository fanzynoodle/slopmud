use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
#[cfg(feature = "s3")]
use aws_sdk_s3::Client as S3Client;
#[cfg(feature = "s3")]
use aws_sdk_s3::primitives::ByteStream;
#[cfg(feature = "s3")]
use aws_sdk_s3::types::ServerSideEncryption;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(feature = "s3")]
use tokio::io::{AsyncWriteExt, BufWriter};
#[cfg(feature = "s3")]
use tracing::info;
use tracing::warn;

pub mod daemon;

pub const MANIFEST_FORMAT: u32 = 1;
pub const DEFAULT_BACKUP_INTERVAL_S: u64 = 60;
pub const DEFAULT_MAX_SEGMENT_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_LOCAL_MANIFESTS: usize = 24 * 60;
pub const DEFAULT_S3_PREFIX: &str = "slopmud/wal-backups";
pub const DEFAULT_RESTORE_CACHE_SUFFIX: &str = ".walrestore-cache";
const COPY_BUF_BYTES: usize = 1024 * 1024;
const MAX_META_PREFIX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct WalBackupConfig {
    pub source_path: PathBuf,
    pub local_dir: PathBuf,
    pub node_id: String,
    pub interval: Duration,
    pub max_segment_bytes: u64,
    pub max_local_manifests: usize,
    pub s3_bucket: Option<String>,
    pub s3_prefix: String,
    pub upload_enabled: bool,
}

impl WalBackupConfig {
    pub fn from_env(source_path: PathBuf, default_node_id: String) -> anyhow::Result<Option<Self>> {
        let explicit_enabled = std::env::var("SLOPMUD_WAL_BACKUP_ENABLED")
            .ok()
            .map(|v| parse_bool(&v))
            .transpose()?;
        let local_dir_env = std::env::var("SLOPMUD_WAL_BACKUP_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let s3_bucket = std::env::var("SLOPMUD_WAL_BACKUP_S3_BUCKET")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let enabled = explicit_enabled.unwrap_or(local_dir_env.is_some() || s3_bucket.is_some());
        if !enabled {
            return Ok(None);
        }

        let local_dir = local_dir_env
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("{}.walbackup", source_path.display())));
        let node_id = std::env::var("SLOPMUD_WAL_BACKUP_NODE_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(default_node_id);
        let interval = Duration::from_secs(
            parse_u64_env("SLOPMUD_WAL_BACKUP_INTERVAL_S", DEFAULT_BACKUP_INTERVAL_S)?.max(1),
        );
        let max_segment_bytes = parse_u64_env(
            "SLOPMUD_WAL_BACKUP_MAX_SEGMENT_BYTES",
            DEFAULT_MAX_SEGMENT_BYTES,
        )?
        .max(1);
        let max_local_manifests = parse_usize_env(
            "SLOPMUD_WAL_BACKUP_MAX_LOCAL_MANIFESTS",
            DEFAULT_MAX_LOCAL_MANIFESTS,
        )?
        .max(1);
        let s3_prefix = std::env::var("SLOPMUD_WAL_BACKUP_S3_PREFIX")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_S3_PREFIX.to_string());
        let upload_enabled = std::env::var("SLOPMUD_WAL_BACKUP_UPLOAD_ENABLED")
            .ok()
            .map(|v| parse_bool(&v))
            .transpose()?
            .unwrap_or(s3_bucket.is_some());

        Ok(Some(Self {
            source_path,
            local_dir,
            node_id,
            interval,
            max_segment_bytes,
            max_local_manifests,
            s3_bucket,
            s3_prefix,
            upload_enabled,
        }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalBackupSegment {
    pub relpath: String,
    pub generation: u64,
    pub start_offset: u64,
    pub end_offset: u64,
    pub bytes: u64,
    pub starts_at_line_boundary: bool,
    pub ends_at_line_boundary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalBackupManifest {
    pub format: u32,
    pub created_unix_s: u64,
    pub node_id: String,
    pub source_log: String,
    pub generation: u64,
    pub source_len: u64,
    #[serde(default)]
    pub tail_probe_offset: u64,
    #[serde(default)]
    pub tail_probe_hex: String,
    pub manifest_relpath: String,
    pub latest_relpath: String,
    pub segments: Vec<WalBackupSegment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalBackupSyncResult {
    pub manifest: Option<WalBackupManifest>,
    pub new_segments: Vec<WalBackupSegment>,
    pub manifest_relpaths: Vec<String>,
    pub source_len: u64,
    pub changed: bool,
    pub generation_reset: bool,
}

impl WalBackupSyncResult {
    fn unchanged(source_len: u64, manifest: Option<WalBackupManifest>) -> Self {
        Self {
            manifest,
            new_segments: Vec::new(),
            manifest_relpaths: Vec::new(),
            source_len,
            changed: false,
            generation_reset: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryTarget {
    Latest,
    Offset(u64),
    Index(u64),
    Ms(u64),
}

impl RecoveryTarget {
    fn includes(self, meta: RaftLineMeta) -> bool {
        match self {
            RecoveryTarget::Latest => true,
            RecoveryTarget::Offset(_) => true,
            RecoveryTarget::Index(max) => meta.index <= max,
            RecoveryTarget::Ms(max) => meta.ms <= max,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked_segments: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalRestoreSource {
    Local(PathBuf),
    S3 { uri: String, cache_dir: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalRestoreConfig {
    pub target_path: PathBuf,
    pub source: WalRestoreSource,
    pub node_id: Option<String>,
    pub target: RecoveryTarget,
    pub manifest_unix_at_or_before: Option<u64>,
    pub overwrite_existing: bool,
    pub missing_manifest_ok: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalRestoreOutcome {
    SkippedExisting {
        path: PathBuf,
        bytes: u64,
    },
    NoManifest,
    Restored {
        path: PathBuf,
        bytes: u64,
        source_len: u64,
        manifest_relpath: String,
    },
}

impl WalRestoreConfig {
    pub fn from_env(target_path: PathBuf) -> anyhow::Result<Option<Self>> {
        let restore_enabled_raw = std::env::var("SLOPMUD_WAL_RESTORE_ENABLED")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let restore_auto = restore_enabled_raw
            .as_deref()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("auto"));
        let explicit_enabled = match restore_enabled_raw.as_deref() {
            Some(v) if v.trim().eq_ignore_ascii_case("auto") => None,
            Some(v) => Some(parse_bool(v)?),
            None => None,
        };
        let restore_dir = std::env::var("SLOPMUD_WAL_RESTORE_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from);
        let restore_s3_uri = restore_s3_uri_from_env()?;
        let fallback_s3_uri = backup_s3_uri_from_env()?;
        let fallback_dir = std::env::var("SLOPMUD_WAL_BACKUP_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from);
        let restore_source_configured = restore_dir.is_some() || restore_s3_uri.is_some();
        let any_source_configured =
            restore_source_configured || fallback_s3_uri.is_some() || fallback_dir.is_some();
        let enabled = if restore_auto {
            any_source_configured
        } else {
            explicit_enabled.unwrap_or(restore_source_configured)
        };
        if !enabled {
            return Ok(None);
        }

        let source = match (restore_dir, restore_s3_uri) {
            (Some(_), Some(_)) => {
                bail!("use only one of SLOPMUD_WAL_RESTORE_DIR or SLOPMUD_WAL_RESTORE_S3_*")
            }
            (Some(dir), None) => WalRestoreSource::Local(dir),
            (None, Some(uri)) => WalRestoreSource::S3 {
                cache_dir: restore_cache_dir_from_env(&target_path)?,
                uri,
            },
            (None, None) => {
                if let Some(uri) = fallback_s3_uri {
                    WalRestoreSource::S3 {
                        cache_dir: restore_cache_dir_from_env(&target_path)?,
                        uri,
                    }
                } else if let Some(dir) = fallback_dir {
                    WalRestoreSource::Local(dir)
                } else {
                    bail!("WAL restore enabled but no restore source is configured");
                }
            }
        };
        let overwrite_existing = parse_optional_bool_env("SLOPMUD_WAL_RESTORE_OVERWRITE")?
            .or(parse_optional_bool_env("SLOPMUD_WAL_RESTORE_FORCE")?)
            .unwrap_or(false);
        let missing_manifest_ok = restore_auto
            || parse_optional_bool_env("SLOPMUD_WAL_RESTORE_MISSING_OK")?.unwrap_or(false);
        let manifest_unix_at_or_before =
            parse_optional_u64_env("SLOPMUD_WAL_RESTORE_MANIFEST_UNIX_AT_OR_BEFORE")?;
        let node_id = restore_node_id_from_env();
        let target = parse_recovery_target_env(
            "SLOPMUD_WAL_RESTORE_UNTIL_OFFSET",
            "SLOPMUD_WAL_RESTORE_UNTIL_INDEX",
            "SLOPMUD_WAL_RESTORE_UNTIL_MS",
        )?;

        Ok(Some(Self {
            target_path,
            source,
            node_id,
            target,
            manifest_unix_at_or_before,
            overwrite_existing,
            missing_manifest_ok,
        }))
    }
}

pub struct WalBackupStore {
    source_path: PathBuf,
    root: PathBuf,
    node_id: String,
    source_log: String,
    max_segment_bytes: u64,
    max_local_manifests: usize,
}

impl WalBackupStore {
    pub fn from_config(cfg: &WalBackupConfig) -> Self {
        Self::new(
            cfg.source_path.clone(),
            cfg.local_dir.clone(),
            cfg.node_id.clone(),
            cfg.max_segment_bytes,
            cfg.max_local_manifests,
        )
    }

    pub fn new(
        source_path: PathBuf,
        root: PathBuf,
        node_id: String,
        max_segment_bytes: u64,
        max_local_manifests: usize,
    ) -> Self {
        let source_log = source_path.display().to_string();
        Self {
            source_path,
            root,
            node_id: sanitize_component(&node_id),
            source_log,
            max_segment_bytes: max_segment_bytes.max(1),
            max_local_manifests: max_local_manifests.max(1),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sync_once(&mut self, now_unix_s: u64) -> anyhow::Result<WalBackupSyncResult> {
        let source_len = match std::fs::metadata(&self.source_path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WalBackupSyncResult::unchanged(
                    0,
                    self.load_latest_manifest()?,
                ));
            }
            Err(e) => return Err(e.into()),
        };

        let prev = self.load_latest_manifest()?;
        let mut generation = prev.as_ref().map(|m| m.generation).unwrap_or(1);
        let mut start_offset = prev.as_ref().map(|m| m.source_len).unwrap_or(0);
        let mut segments = prev
            .as_ref()
            .filter(|m| m.source_len <= source_len)
            .map(|m| m.segments.clone())
            .unwrap_or_default();
        let mut generation_reset = false;

        let previous_tail_matches = prev
            .as_ref()
            .map(|m| source_tail_probe_matches(&self.source_path, m))
            .transpose()?
            .unwrap_or(true);

        if start_offset > source_len || !previous_tail_matches {
            generation = generation.saturating_add(1);
            start_offset = 0;
            segments.clear();
            generation_reset = true;
        }

        if start_offset == source_len && prev.is_some() && !generation_reset {
            return Ok(WalBackupSyncResult::unchanged(source_len, prev));
        }

        let mut new_segments = Vec::new();
        let mut offset = start_offset;
        let mut starts_at_line_boundary = segments
            .last()
            .map(|s| s.ends_at_line_boundary)
            .unwrap_or(true);

        while offset < source_len {
            let end = offset
                .saturating_add(self.max_segment_bytes)
                .min(source_len)
                .max(offset.saturating_add(1));
            let relpath = self.segment_relpath(generation, offset, end);
            let dst = self.path_for_relpath(&relpath);
            let meta = copy_source_range(
                &self.source_path,
                &dst,
                offset,
                end,
                starts_at_line_boundary,
            )
            .with_context(|| {
                format!(
                    "copy WAL backup extent {}..{} from {}",
                    offset,
                    end,
                    self.source_path.display()
                )
            })?;
            let segment = WalBackupSegment {
                relpath,
                generation,
                start_offset: offset,
                end_offset: end,
                bytes: end.saturating_sub(offset),
                starts_at_line_boundary,
                ends_at_line_boundary: meta.ends_at_line_boundary,
                first_index: meta.first_index,
                last_index: meta.last_index,
                first_ms: meta.first_ms,
                last_ms: meta.last_ms,
                sha256_hex: Some(meta.sha256_hex),
            };
            starts_at_line_boundary = segment.ends_at_line_boundary;
            offset = end;
            segments.push(segment.clone());
            new_segments.push(segment);
        }

        let manifest_relpath = self.manifest_relpath(now_unix_s, generation, source_len);
        let latest_relpath = self.latest_manifest_relpath();
        let tail_probe = source_tail_probe(&self.source_path, source_len)?;
        let manifest = WalBackupManifest {
            format: MANIFEST_FORMAT,
            created_unix_s: now_unix_s,
            node_id: self.node_id.clone(),
            source_log: self.source_log.clone(),
            generation,
            source_len,
            tail_probe_offset: tail_probe.offset,
            tail_probe_hex: tail_probe.hex,
            manifest_relpath: manifest_relpath.clone(),
            latest_relpath: latest_relpath.clone(),
            segments,
        };
        self.write_manifest(&manifest, &manifest_relpath)?;
        self.write_manifest(&manifest, &latest_relpath)?;
        self.prune_old_manifests()?;

        Ok(WalBackupSyncResult {
            manifest: Some(manifest),
            new_segments,
            manifest_relpaths: vec![manifest_relpath, latest_relpath],
            source_len,
            changed: true,
            generation_reset,
        })
    }

    pub fn load_latest_manifest(&self) -> anyhow::Result<Option<WalBackupManifest>> {
        let path = self.path_for_relpath(&self.latest_manifest_relpath());
        match std::fs::read(&path) {
            Ok(raw) => Ok(Some(serde_json::from_slice(&raw)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list_manifests(&self) -> anyhow::Result<Vec<WalBackupManifest>> {
        let dir = self.path_for_relpath(&format!("{}/lsmt/manifests", self.node_base()));
        let mut out = Vec::new();
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for ent in entries {
                    let ent = ent?;
                    let path = ent.path();
                    if !path.is_file()
                        || path.file_name().and_then(|n| n.to_str()) == Some("latest.json")
                    {
                        continue;
                    }
                    if path.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    let raw = std::fs::read(&path)?;
                    let manifest: WalBackupManifest = serde_json::from_slice(&raw)?;
                    out.push(manifest);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        out.sort_by_key(|m| (m.created_unix_s, m.generation, m.source_len));
        Ok(out)
    }

    pub fn latest_extents(&self) -> anyhow::Result<Vec<WalBackupSegment>> {
        Ok(self
            .load_latest_manifest()?
            .map(|m| m.segments)
            .unwrap_or_default())
    }

    pub fn select_manifest_at_or_before(
        &self,
        created_unix_s: Option<u64>,
    ) -> anyhow::Result<Option<WalBackupManifest>> {
        let Some(cutoff) = created_unix_s else {
            return self.load_latest_manifest();
        };
        let mut selected = None;
        for manifest in self.list_manifests()? {
            if manifest.created_unix_s <= cutoff {
                selected = Some(manifest);
            }
        }
        Ok(selected)
    }

    pub fn recover_to_path(
        &self,
        manifest: &WalBackupManifest,
        out_path: &Path,
        target: RecoveryTarget,
    ) -> anyhow::Result<u64> {
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        self.verify_manifest(manifest)?;
        let tmp = tmp_path(out_path, "recovering");
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        let bytes = match target {
            RecoveryTarget::Latest => self.concat_segments(manifest, &mut out, None)?,
            RecoveryTarget::Offset(max_offset) => {
                self.concat_segments(manifest, &mut out, Some(max_offset))?
            }
            RecoveryTarget::Index(_) | RecoveryTarget::Ms(_) => {
                self.concat_lines_until(manifest, &mut out, target)?
            }
        };
        out.flush()?;
        out.sync_data()?;
        drop(out);
        std::fs::rename(&tmp, out_path)?;
        Ok(bytes)
    }

    pub fn verify_latest(&self) -> anyhow::Result<VerifyReport> {
        let Some(manifest) = self.load_latest_manifest()? else {
            return Ok(VerifyReport {
                checked_segments: 0,
                bytes: 0,
            });
        };
        self.verify_manifest(&manifest)
    }

    pub fn verify_manifest(&self, manifest: &WalBackupManifest) -> anyhow::Result<VerifyReport> {
        let mut expected_start = 0u64;
        let mut bytes = 0u64;
        for segment in &manifest.segments {
            if segment.start_offset != expected_start {
                bail!(
                    "wal backup extent gap/overlap: expected start {}, got {} ({})",
                    expected_start,
                    segment.start_offset,
                    segment.relpath
                );
            }
            if segment.end_offset < segment.start_offset {
                bail!("wal backup extent has negative range: {}", segment.relpath);
            }
            if segment.bytes != segment.end_offset.saturating_sub(segment.start_offset) {
                bail!("wal backup extent byte count mismatch: {}", segment.relpath);
            }
            let path = self.path_for_relpath(&segment.relpath);
            let len = std::fs::metadata(&path)
                .with_context(|| format!("missing wal backup segment {}", path.display()))?
                .len();
            if len != segment.bytes {
                bail!(
                    "wal backup segment length mismatch for {}: manifest={} disk={}",
                    segment.relpath,
                    segment.bytes,
                    len
                );
            }
            if let Some(expected_hash) = segment.sha256_hex.as_deref() {
                let got_hash = file_sha256_hex(&path)
                    .with_context(|| format!("hash wal backup segment {}", path.display()))?;
                if got_hash != expected_hash {
                    bail!(
                        "wal backup segment checksum mismatch for {}: manifest={} disk={}",
                        segment.relpath,
                        expected_hash,
                        got_hash
                    );
                }
            }
            bytes = bytes.saturating_add(len);
            expected_start = segment.end_offset;
        }
        if expected_start != manifest.source_len {
            bail!(
                "wal backup manifest source_len mismatch: segments end at {}, manifest says {}",
                expected_start,
                manifest.source_len
            );
        }
        Ok(VerifyReport {
            checked_segments: manifest.segments.len(),
            bytes,
        })
    }

    fn concat_segments(
        &self,
        manifest: &WalBackupManifest,
        out: &mut File,
        max_offset: Option<u64>,
    ) -> anyhow::Result<u64> {
        let mut written = 0u64;
        for segment in &manifest.segments {
            let wanted_end = max_offset
                .map(|m| m.min(segment.end_offset))
                .unwrap_or(segment.end_offset);
            if wanted_end <= segment.start_offset {
                break;
            }
            let bytes_to_copy = wanted_end.saturating_sub(segment.start_offset);
            let mut input = File::open(self.path_for_relpath(&segment.relpath))?;
            copy_n(&mut input, out, bytes_to_copy)?;
            written = written.saturating_add(bytes_to_copy);
            if max_offset.is_some_and(|m| m <= segment.end_offset) {
                break;
            }
        }
        Ok(written)
    }

    fn concat_lines_until(
        &self,
        manifest: &WalBackupManifest,
        out: &mut File,
        target: RecoveryTarget,
    ) -> anyhow::Result<u64> {
        let mut written = 0u64;
        let mut filter = LineRecoveryFilter::new(target);
        let mut buf = vec![0u8; COPY_BUF_BYTES];
        for segment in &manifest.segments {
            let mut input = File::open(self.path_for_relpath(&segment.relpath))?;
            loop {
                let n = input.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let outcome = filter.feed(&buf[..n], out)?;
                written = written.saturating_add(outcome.written);
                if outcome.stop {
                    return Ok(written);
                }
            }
        }
        Ok(written)
    }

    fn node_base(&self) -> String {
        format!("v1/nodes/{}", self.node_id)
    }

    fn segment_relpath(&self, generation: u64, start: u64, end: u64) -> String {
        format!(
            "{}/lsmt/segments/g{:020}/{:020}-{:020}.wal",
            self.node_base(),
            generation,
            start,
            end
        )
    }

    fn manifest_relpath(&self, created_unix_s: u64, generation: u64, source_len: u64) -> String {
        format!(
            "{}/lsmt/manifests/{:020}-g{:020}-{:020}.json",
            self.node_base(),
            created_unix_s,
            generation,
            source_len
        )
    }

    fn latest_manifest_relpath(&self) -> String {
        format!("{}/lsmt/manifests/latest.json", self.node_base())
    }

    fn path_for_relpath(&self, relpath: &str) -> PathBuf {
        self.root.join(relpath)
    }

    fn write_manifest(&self, manifest: &WalBackupManifest, relpath: &str) -> anyhow::Result<()> {
        let path = self.path_for_relpath(relpath);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = tmp_path(&path, "writing");
        let raw = serde_json::to_vec_pretty(manifest)?;
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(&raw)?;
        f.write_all(b"\n")?;
        f.flush()?;
        f.sync_data()?;
        drop(f);
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    fn prune_old_manifests(&self) -> anyhow::Result<()> {
        let manifests = self.list_manifests()?;
        if manifests.len() <= self.max_local_manifests {
            return self.prune_unreferenced_segments();
        }
        let remove_count = manifests.len() - self.max_local_manifests;
        for manifest in manifests.iter().take(remove_count) {
            let path = self.path_for_relpath(&manifest.manifest_relpath);
            let _ = std::fs::remove_file(path);
        }
        self.prune_unreferenced_segments()
    }

    fn prune_unreferenced_segments(&self) -> anyhow::Result<()> {
        let mut referenced = HashSet::new();
        for manifest in self.list_manifests()? {
            for segment in manifest.segments {
                referenced.insert(segment.relpath);
            }
        }
        if let Some(latest) = self.load_latest_manifest()? {
            for segment in latest.segments {
                referenced.insert(segment.relpath);
            }
        }

        let segments_dir = self.path_for_relpath(&format!("{}/lsmt/segments", self.node_base()));
        prune_unreferenced_segment_files(&self.root, &segments_dir, &referenced)
    }
}

fn prune_unreferenced_segment_files(
    root: &Path,
    dir: &Path,
    referenced: &HashSet<String>,
) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for ent in entries {
        let ent = ent?;
        let path = ent.path();
        let ft = ent.file_type()?;
        if ft.is_dir() {
            prune_unreferenced_segment_files(root, &path, referenced)?;
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        if !ft.is_file() || path.extension().and_then(|e| e.to_str()) != Some("wal") {
            continue;
        }
        let relpath = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))?;
        if !referenced.contains(&relpath) {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

pub fn list_local_manifests(root: &Path) -> anyhow::Result<Vec<WalBackupManifest>> {
    let mut out: Vec<WalBackupManifest> = Vec::new();
    let base = root.join("v1").join("nodes");
    match std::fs::read_dir(&base) {
        Ok(nodes) => {
            for node in nodes {
                let node = node?;
                if !node.file_type()?.is_dir() {
                    continue;
                }
                let manifest_dir = node.path().join("lsmt").join("manifests");
                let Ok(entries) = std::fs::read_dir(&manifest_dir) else {
                    continue;
                };
                for ent in entries {
                    let ent = ent?;
                    let path = ent.path();
                    if !path.is_file()
                        || path.file_name().and_then(|n| n.to_str()) == Some("latest.json")
                        || path.extension().and_then(|e| e.to_str()) != Some("json")
                    {
                        continue;
                    }
                    let raw = std::fs::read(&path)?;
                    out.push(serde_json::from_slice(&raw)?);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    out.sort_by_key(|m| {
        (
            m.created_unix_s,
            m.node_id.clone(),
            m.generation,
            m.source_len,
        )
    });
    Ok(out)
}

pub fn select_manifest_from_list(
    manifests: &[WalBackupManifest],
    created_unix_s: Option<u64>,
) -> Option<WalBackupManifest> {
    select_manifest_from_candidates(manifests, created_unix_s)
}

pub fn filter_manifests_by_node(
    manifests: &[WalBackupManifest],
    node_id: Option<&str>,
) -> Vec<WalBackupManifest> {
    let Some(raw_node_id) = node_id.map(str::trim).filter(|v| !v.is_empty()) else {
        return manifests.to_vec();
    };
    let sanitized_node_id = sanitize_component(raw_node_id);
    manifests
        .iter()
        .filter(|m| m.node_id == raw_node_id || m.node_id == sanitized_node_id)
        .cloned()
        .collect()
}

pub fn select_manifest_from_list_for_node(
    manifests: &[WalBackupManifest],
    created_unix_s: Option<u64>,
    node_id: Option<&str>,
) -> Option<WalBackupManifest> {
    let filtered = filter_manifests_by_node(manifests, node_id);
    select_manifest_from_candidates(&filtered, created_unix_s)
}

fn select_manifest_from_candidates(
    manifests: &[WalBackupManifest],
    created_unix_s: Option<u64>,
) -> Option<WalBackupManifest> {
    let mut candidates = manifests
        .iter()
        .filter(|m| created_unix_s.map_or(true, |cutoff| m.created_unix_s <= cutoff))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|m| (m.created_unix_s, m.generation, m.source_len));
    candidates.pop()
}

pub fn store_for_manifest(root: PathBuf, manifest: &WalBackupManifest) -> WalBackupStore {
    WalBackupStore::new(
        PathBuf::from(&manifest.source_log),
        root,
        manifest.node_id.clone(),
        DEFAULT_MAX_SEGMENT_BYTES,
        DEFAULT_MAX_LOCAL_MANIFESTS,
    )
}

pub async fn restore_wal_from_backup(cfg: &WalRestoreConfig) -> anyhow::Result<WalRestoreOutcome> {
    if !cfg.overwrite_existing {
        if let Ok(meta) = std::fs::metadata(&cfg.target_path) {
            if meta.len() > 0 {
                return Ok(WalRestoreOutcome::SkippedExisting {
                    path: cfg.target_path.clone(),
                    bytes: meta.len(),
                });
            }
        }
    }

    let (root, manifest) = match &cfg.source {
        WalRestoreSource::Local(dir) => {
            let manifests = list_local_manifests(dir)?;
            let Some(manifest) = select_manifest_from_list_for_node(
                &manifests,
                cfg.manifest_unix_at_or_before,
                cfg.node_id.as_deref(),
            ) else {
                return Ok(WalRestoreOutcome::NoManifest);
            };
            (dir.clone(), manifest)
        }
        WalRestoreSource::S3 { uri, cache_dir } => {
            let Some(manifest) = restore_s3_manifest_to_cache(
                uri,
                cache_dir,
                cfg.manifest_unix_at_or_before,
                cfg.node_id.as_deref(),
            )
            .await?
            else {
                return Ok(WalRestoreOutcome::NoManifest);
            };
            (cache_dir.clone(), manifest)
        }
    };

    let store = store_for_manifest(root, &manifest);
    let bytes = store.recover_to_path(&manifest, &cfg.target_path, cfg.target)?;
    Ok(WalRestoreOutcome::Restored {
        path: cfg.target_path.clone(),
        bytes,
        source_len: manifest.source_len,
        manifest_relpath: manifest.manifest_relpath,
    })
}

#[cfg(feature = "s3")]
async fn restore_s3_manifest_to_cache(
    uri: &str,
    cache_dir: &Path,
    manifest_unix_at_or_before: Option<u64>,
    node_id: Option<&str>,
) -> anyhow::Result<Option<WalBackupManifest>> {
    let client = S3WalBackupClient::from_uri(uri).await?;
    let manifests = client.list_manifests().await?;
    let Some(manifest) =
        select_manifest_from_list_for_node(&manifests, manifest_unix_at_or_before, node_id)
    else {
        return Ok(None);
    };
    client
        .download_manifest_to_cache(&manifest, cache_dir)
        .await?;
    Ok(Some(manifest))
}

#[cfg(not(feature = "s3"))]
async fn restore_s3_manifest_to_cache(
    _uri: &str,
    _cache_dir: &Path,
    _manifest_unix_at_or_before: Option<u64>,
    _node_id: Option<&str>,
) -> anyhow::Result<Option<WalBackupManifest>> {
    bail!("S3 WAL restore requires the slopmud_walbackup s3 feature");
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WalSegmentLineMeta {
    ends_at_line_boundary: bool,
    first_index: Option<u64>,
    last_index: Option<u64>,
    first_ms: Option<u64>,
    last_ms: Option<u64>,
    sha256_hex: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RaftLineMeta {
    index: u64,
    ms: u64,
}

struct WalMetadataScanner {
    prefix: Vec<u8>,
    at_line_boundary: bool,
    first_index: Option<u64>,
    last_index: Option<u64>,
    first_ms: Option<u64>,
    last_ms: Option<u64>,
}

impl WalMetadataScanner {
    fn new(starts_at_line_boundary: bool) -> Self {
        Self {
            prefix: Vec::with_capacity(256),
            at_line_boundary: starts_at_line_boundary,
            first_index: None,
            last_index: None,
            first_ms: None,
            last_ms: None,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.at_line_boundary && self.prefix.len() < MAX_META_PREFIX_BYTES {
                self.prefix.push(b);
            }
            if b == b'\n' {
                if self.at_line_boundary {
                    self.record_current_prefix();
                }
                self.prefix.clear();
                self.at_line_boundary = true;
            } else if !self.at_line_boundary {
                continue;
            } else if self.prefix.len() >= MAX_META_PREFIX_BYTES {
                self.at_line_boundary = false;
                self.prefix.clear();
            }
        }
    }

    fn finish(mut self) -> WalSegmentLineMeta {
        if self.at_line_boundary && !self.prefix.is_empty() {
            self.record_current_prefix();
            self.at_line_boundary = false;
        }
        WalSegmentLineMeta {
            ends_at_line_boundary: self.at_line_boundary,
            first_index: self.first_index,
            last_index: self.last_index,
            first_ms: self.first_ms,
            last_ms: self.last_ms,
            sha256_hex: String::new(),
        }
    }

    fn record_current_prefix(&mut self) {
        if let Some(meta) = parse_raft_line_meta_prefix(&self.prefix) {
            self.first_index.get_or_insert(meta.index);
            self.first_ms.get_or_insert(meta.ms);
            self.last_index = Some(meta.index);
            self.last_ms = Some(meta.ms);
        }
    }
}

fn copy_source_range(
    source: &Path,
    dst: &Path,
    start: u64,
    end: u64,
    starts_at_line_boundary: bool,
) -> anyhow::Result<WalSegmentLineMeta> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(dst, "writing");
    let mut input = File::open(source)?;
    input.seek(SeekFrom::Start(start))?;
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)?;
    let mut scanner = WalMetadataScanner::new(starts_at_line_boundary);
    let mut hasher = Sha256::new();
    let mut remaining = end.saturating_sub(start);
    let mut buf = vec![0u8; COPY_BUF_BYTES];

    while remaining > 0 {
        let want = (remaining as usize).min(buf.len());
        let n = input.read(&mut buf[..want])?;
        if n == 0 {
            bail!("source WAL ended before requested backup extent");
        }
        output.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        scanner.feed(&buf[..n]);
        remaining = remaining.saturating_sub(n as u64);
    }

    output.flush()?;
    output.sync_data()?;
    drop(output);
    std::fs::rename(tmp, dst)?;
    let mut meta = scanner.finish();
    meta.sha256_hex = hex_encode(&hasher.finalize());
    Ok(meta)
}

fn file_sha256_hex(path: &Path) -> anyhow::Result<String> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn copy_n(input: &mut File, output: &mut File, bytes: u64) -> anyhow::Result<()> {
    let mut remaining = bytes;
    let mut buf = vec![0u8; COPY_BUF_BYTES];
    while remaining > 0 {
        let want = (remaining as usize).min(buf.len());
        let n = input.read(&mut buf[..want])?;
        if n == 0 {
            bail!("backup segment ended early");
        }
        output.write_all(&buf[..n])?;
        remaining = remaining.saturating_sub(n as u64);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TailProbe {
    offset: u64,
    hex: String,
}

fn source_tail_probe(path: &Path, source_len: u64) -> anyhow::Result<TailProbe> {
    if source_len == 0 {
        return Ok(TailProbe {
            offset: 0,
            hex: String::new(),
        });
    }
    let probe_len = source_len.min(64);
    let offset = source_len.saturating_sub(probe_len);
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; probe_len as usize];
    f.read_exact(&mut buf)?;
    Ok(TailProbe {
        offset,
        hex: hex_encode(&buf),
    })
}

fn source_tail_probe_matches(path: &Path, manifest: &WalBackupManifest) -> anyhow::Result<bool> {
    if manifest.tail_probe_hex.is_empty() {
        return Ok(true);
    }
    let expected = hex_decode(&manifest.tail_probe_hex)?;
    let current_len = std::fs::metadata(path)?.len();
    if manifest
        .tail_probe_offset
        .saturating_add(expected.len() as u64)
        > current_len
    {
        return Ok(false);
    }
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(manifest.tail_probe_offset))?;
    let mut got = vec![0u8; expected.len()];
    f.read_exact(&mut got)?;
    Ok(got == expected)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(raw: &str) -> anyhow::Result<Vec<u8>> {
    let raw = raw.as_bytes();
    if raw.len() % 2 != 0 {
        bail!("invalid odd-length hex tail probe");
    }
    let mut out = Vec::with_capacity(raw.len() / 2);
    let mut i = 0usize;
    while i < raw.len() {
        let hi = hex_val(raw[i]).ok_or_else(|| anyhow::anyhow!("invalid hex tail probe"))?;
        let lo = hex_val(raw[i + 1]).ok_or_else(|| anyhow::anyhow!("invalid hex tail probe"))?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Default)]
struct FeedOutcome {
    written: u64,
    stop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CurrentLineDecision {
    Unknown,
    Include,
}

struct LineRecoveryFilter {
    target: RecoveryTarget,
    pending: Vec<u8>,
    decision: CurrentLineDecision,
}

impl LineRecoveryFilter {
    fn new(target: RecoveryTarget) -> Self {
        Self {
            target,
            pending: Vec::with_capacity(256),
            decision: CurrentLineDecision::Unknown,
        }
    }

    fn feed(&mut self, bytes: &[u8], out: &mut File) -> anyhow::Result<FeedOutcome> {
        let mut pos = 0usize;
        let mut outcome = FeedOutcome::default();
        while pos < bytes.len() {
            match self.decision {
                CurrentLineDecision::Unknown => {
                    let b = bytes[pos];
                    pos += 1;
                    if self.pending.len() < MAX_META_PREFIX_BYTES {
                        self.pending.push(b);
                    }
                    if let Some(meta) = parse_raft_line_meta_prefix(&self.pending) {
                        if !self.target.includes(meta) {
                            outcome.stop = true;
                            return Ok(outcome);
                        }
                        out.write_all(&self.pending)?;
                        outcome.written = outcome.written.saturating_add(self.pending.len() as u64);
                        self.pending.clear();
                        self.decision = CurrentLineDecision::Include;
                        if b == b'\n' {
                            self.decision = CurrentLineDecision::Unknown;
                        }
                        continue;
                    }
                    if b == b'\n' || self.pending.len() >= MAX_META_PREFIX_BYTES {
                        out.write_all(&self.pending)?;
                        outcome.written = outcome.written.saturating_add(self.pending.len() as u64);
                        self.pending.clear();
                        self.decision = if b == b'\n' {
                            CurrentLineDecision::Unknown
                        } else {
                            CurrentLineDecision::Include
                        };
                    }
                }
                CurrentLineDecision::Include => {
                    let rest = &bytes[pos..];
                    if let Some(nl) = rest.iter().position(|&b| b == b'\n') {
                        let end = pos + nl + 1;
                        out.write_all(&bytes[pos..end])?;
                        outcome.written = outcome.written.saturating_add((end - pos) as u64);
                        pos = end;
                        self.decision = CurrentLineDecision::Unknown;
                        self.pending.clear();
                    } else {
                        out.write_all(rest)?;
                        outcome.written = outcome.written.saturating_add(rest.len() as u64);
                        break;
                    }
                }
            }
        }
        Ok(outcome)
    }
}

fn parse_raft_line_meta_prefix(prefix: &[u8]) -> Option<RaftLineMeta> {
    let index = find_json_u64_field(prefix, "index")?;
    let ms = find_json_u64_field(prefix, "ms")?;
    Some(RaftLineMeta { index, ms })
}

fn find_json_u64_field(buf: &[u8], field: &str) -> Option<u64> {
    let needle = format!("\"{field}\":");
    let needle = needle.as_bytes();
    let start = buf.windows(needle.len()).position(|w| w == needle)? + needle.len();
    let mut i = start;
    while i < buf.len() && buf[i].is_ascii_whitespace() {
        i += 1;
    }
    let digit_start = i;
    while i < buf.len() && buf[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return None;
    }
    if i == buf.len() {
        return None;
    }
    std::str::from_utf8(&buf[digit_start..i]).ok()?.parse().ok()
}

pub async fn run_backup_loop(cfg: WalBackupConfig) {
    #[cfg(feature = "s3")]
    let uploader = build_wal_uploader(&cfg).await;
    #[cfg(not(feature = "s3"))]
    if cfg.upload_enabled {
        warn!("wal backup upload requested but this binary was built without S3 support");
    }

    let mut store = WalBackupStore::from_config(&cfg);
    let mut ticker = tokio::time::interval(cfg.interval);
    loop {
        ticker.tick().await;
        let now = unix_now_s();
        #[cfg(feature = "s3")]
        let root = store.root().to_path_buf();
        match store.sync_once(now) {
            Ok(result) => {
                if !result.changed {
                    continue;
                }
                #[cfg(feature = "s3")]
                if let Some(client) = uploader.as_ref() {
                    if let Err(err) = client.upload_sync_result(&root, &result).await {
                        warn!(err=%err, "wal backup s3 upload failed");
                    }
                }
            }
            Err(err) => warn!(err=%err, "wal backup sync failed"),
        }
    }
}

#[cfg(feature = "s3")]
async fn build_wal_uploader(cfg: &WalBackupConfig) -> Option<S3WalBackupClient> {
    if !cfg.upload_enabled {
        return None;
    }
    match cfg.s3_bucket.clone() {
        Some(bucket) => match S3WalBackupClient::new(bucket, cfg.s3_prefix.clone()).await {
            Ok(client) => Some(client),
            Err(err) => {
                warn!(err=%err, "wal backup s3 client initialization failed; s3 uploads disabled");
                None
            }
        },
        None => {
            warn!("wal backup upload enabled without S3 bucket; s3 uploads disabled");
            None
        }
    }
}

#[cfg(feature = "s3")]
#[derive(Clone)]
pub struct S3WalBackupClient {
    client: S3Client,
    bucket: String,
    prefix: String,
}

#[cfg(feature = "s3")]
impl S3WalBackupClient {
    pub async fn new(bucket: String, prefix: String) -> anyhow::Result<Self> {
        let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Ok(Self {
            client: S3Client::new(&aws_cfg),
            bucket,
            prefix: normalize_prefix(&prefix),
        })
    }

    pub async fn from_uri(uri: &str) -> anyhow::Result<Self> {
        let (bucket, prefix) = parse_s3_uri(uri)?;
        Self::new(bucket, prefix).await
    }

    pub async fn upload_sync_result(
        &self,
        root: &Path,
        result: &WalBackupSyncResult,
    ) -> anyhow::Result<()> {
        if !result.changed {
            return Ok(());
        }
        for segment in &result.new_segments {
            self.put_relpath(root, &segment.relpath, "application/octet-stream")
                .await?;
        }
        for relpath in &result.manifest_relpaths {
            self.put_relpath(root, relpath, "application/json").await?;
        }
        Ok(())
    }

    pub async fn list_manifests(&self) -> anyhow::Result<Vec<WalBackupManifest>> {
        let mut out = Vec::new();
        let prefix = self.key_for_relpath("v1/nodes/");
        let mut continuation = None::<String>;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix);
            if let Some(token) = continuation.as_deref() {
                req = req.continuation_token(token);
            }
            let resp = req.send().await?;
            for obj in resp.contents() {
                let Some(key) = obj.key() else {
                    continue;
                };
                if !key.ends_with(".json") || key.ends_with("/latest.json") {
                    continue;
                }
                if !key.contains("/lsmt/manifests/") {
                    continue;
                }
                let rel = self.relpath_for_key(key);
                let manifest = self.get_manifest_relpath(&rel).await?;
                out.push(manifest);
            }
            continuation = resp.next_continuation_token().map(|s| s.to_string());
            if continuation.is_none() {
                break;
            }
        }
        out.sort_by_key(|m| (m.created_unix_s, m.generation, m.source_len));
        Ok(out)
    }

    pub async fn download_manifest_to_cache(
        &self,
        manifest: &WalBackupManifest,
        cache_dir: &Path,
    ) -> anyhow::Result<WalBackupStore> {
        for segment in &manifest.segments {
            self.download_relpath_if_needed(&segment.relpath, segment.bytes, cache_dir)
                .await?;
        }
        write_cache_manifest(cache_dir, manifest, &manifest.manifest_relpath)?;
        write_cache_manifest(cache_dir, manifest, &manifest.latest_relpath)?;
        Ok(WalBackupStore::new(
            PathBuf::from(&manifest.source_log),
            cache_dir.to_path_buf(),
            manifest.node_id.clone(),
            DEFAULT_MAX_SEGMENT_BYTES,
            DEFAULT_MAX_LOCAL_MANIFESTS,
        ))
    }

    async fn get_manifest_relpath(&self, relpath: &str) -> anyhow::Result<WalBackupManifest> {
        let key = self.key_for_relpath(relpath);
        let obj = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await?;
        let raw = obj.body.collect().await?.into_bytes();
        Ok(serde_json::from_slice(&raw)?)
    }

    async fn put_relpath(
        &self,
        root: &Path,
        relpath: &str,
        content_type: &str,
    ) -> anyhow::Result<()> {
        let path = root.join(relpath);
        let key = self.key_for_relpath(relpath);
        let body = ByteStream::from_path(&path).await?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .content_type(content_type)
            .server_side_encryption(ServerSideEncryption::Aes256)
            .body(body)
            .send()
            .await?;
        info!(bucket=%self.bucket, key=%key, "wal backup uploaded");
        Ok(())
    }

    async fn download_relpath_if_needed(
        &self,
        relpath: &str,
        expected_len: u64,
        cache_dir: &Path,
    ) -> anyhow::Result<()> {
        let path = cache_dir.join(relpath);
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) == expected_len {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = tmp_path(&path, "downloading");
        let obj = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(self.key_for_relpath(relpath))
            .send()
            .await?;
        let mut input = obj.body.into_async_read();
        let f = tokio::fs::File::create(&tmp).await?;
        let mut output = BufWriter::new(f);
        tokio::io::copy(&mut input, &mut output).await?;
        output.flush().await?;
        output.into_inner().sync_data().await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    fn key_for_relpath(&self, relpath: &str) -> String {
        join_prefix(&self.prefix, relpath)
    }

    fn relpath_for_key<'a>(&self, key: &'a str) -> &'a str {
        key.strip_prefix(&self.prefix)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or(key)
    }
}

#[cfg(feature = "s3")]
#[derive(Clone, Debug)]
pub struct EventLogUploadConfig {
    pub spool_dir: PathBuf,
    pub s3_bucket: String,
    pub s3_prefix: String,
    pub upload_delete_local: bool,
}

#[cfg(feature = "s3")]
impl EventLogUploadConfig {
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let upload_enabled = std::env::var("SLOPMUD_EVENTLOG_UPLOAD_ENABLED")
            .ok()
            .is_some_and(|v| parse_bool_lossy(&v));
        if !upload_enabled {
            return Ok(None);
        }
        let Some(s3_bucket) = std::env::var("SLOPMUD_EVENTLOG_S3_BUCKET")
            .ok()
            .filter(|v| !v.trim().is_empty())
        else {
            return Ok(None);
        };
        let spool_dir = std::env::var("SLOPMUD_EVENTLOG_SPOOL_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("locks/eventlog"));
        let s3_prefix = std::env::var("SLOPMUD_EVENTLOG_S3_PREFIX")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "slopmud/eventlog".to_string());
        let upload_delete_local = !std::env::var("SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL")
            .ok()
            .is_some_and(|v| v == "0" || v.eq_ignore_ascii_case("false"));
        Ok(Some(Self {
            spool_dir,
            s3_bucket,
            s3_prefix,
            upload_delete_local,
        }))
    }
}

#[cfg(feature = "s3")]
pub async fn upload_eventlog_relpaths(
    cfg: &EventLogUploadConfig,
    relpaths: &[String],
) -> anyhow::Result<usize> {
    if relpaths.is_empty() {
        return Ok(0);
    }
    let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let s3 = S3Client::new(&aws_cfg);
    let mut uploaded = 0usize;
    for rel in relpaths {
        if upload_one_eventlog_relpath(cfg, &s3, rel).await? {
            uploaded += 1;
        }
    }
    Ok(uploaded)
}

#[cfg(feature = "s3")]
pub fn scan_eventlog_backlog(cfg: &EventLogUploadConfig) -> anyhow::Result<Vec<String>> {
    let today = chrono::Utc::now().date_naive();
    let mut rels = Vec::new();
    scan_eventlog_dir(
        cfg.spool_dir.as_path(),
        cfg.spool_dir.as_path(),
        today,
        &mut rels,
    )?;
    Ok(rels)
}

#[cfg(feature = "s3")]
async fn upload_one_eventlog_relpath(
    cfg: &EventLogUploadConfig,
    s3: &S3Client,
    rel: &str,
) -> anyhow::Result<bool> {
    let path = cfg.spool_dir.join(rel);
    if !path.is_file() {
        return Ok(false);
    }
    let key = join_prefix(&cfg.s3_prefix, rel);
    let body = ByteStream::from_path(&path).await?;
    s3.put_object()
        .bucket(&cfg.s3_bucket)
        .key(&key)
        .content_type("text/plain; charset=utf-8")
        .server_side_encryption(ServerSideEncryption::Aes256)
        .body(body)
        .send()
        .await?;
    info!(bucket=%cfg.s3_bucket, key=%key, "eventlog uploaded");
    if cfg.upload_delete_local {
        let _ = std::fs::remove_file(&path);
    }
    Ok(true)
}

#[cfg(feature = "s3")]
fn scan_eventlog_dir(
    root: &Path,
    dir: &Path,
    today: chrono::NaiveDate,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for ent in entries {
        let ent = ent?;
        let path = ent.path();
        let ft = ent.file_type()?;
        if ft.is_dir() {
            scan_eventlog_dir(root, &path, today, out)?;
            continue;
        }
        if !ft.is_file() || path.extension().and_then(|e| e.to_str()) != Some("log") {
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
        if eventlog_relpath_is_today(&rel, today) {
            continue;
        }
        out.push(rel);
    }
    Ok(())
}

#[cfg(feature = "s3")]
fn eventlog_relpath_is_today(rel: &str, today: chrono::NaiveDate) -> bool {
    use chrono::Datelike;

    let parts = rel.split('/').collect::<Vec<_>>();
    if parts.len() < 5 {
        return false;
    }
    let Some(d_stem) = parts[parts.len() - 1].strip_suffix(".log") else {
        return false;
    };
    let Ok(y) = parts[parts.len() - 3].parse::<i32>() else {
        return false;
    };
    let Ok(m) = parts[parts.len() - 2].parse::<u32>() else {
        return false;
    };
    let Ok(d) = d_stem.parse::<u32>() else {
        return false;
    };
    y == today.year() && m == today.month() && d == today.day()
}

#[cfg(feature = "s3")]
fn write_cache_manifest(
    cache_dir: &Path,
    manifest: &WalBackupManifest,
    relpath: &str,
) -> anyhow::Result<()> {
    let path = cache_dir.join(relpath);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

pub fn parse_s3_uri(uri: &str) -> anyhow::Result<(String, String)> {
    let rest = uri
        .strip_prefix("s3://")
        .ok_or_else(|| anyhow::anyhow!("expected s3://bucket/prefix"))?;
    let mut parts = rest.splitn(2, '/');
    let bucket = parts.next().unwrap_or_default().trim();
    if bucket.is_empty() {
        bail!("missing S3 bucket");
    }
    let prefix = parts.next().unwrap_or_default();
    Ok((bucket.to_string(), normalize_prefix(prefix)))
}

pub fn manifest_summary_lines(manifests: &[WalBackupManifest]) -> Vec<String> {
    manifests
        .iter()
        .map(|m| {
            let first = m.segments.first();
            let last = m.segments.last();
            format!(
                "created_unix_s={} node={} generation={} source_len={} segments={} first_offset={} last_offset={} first_index={} last_index={} first_ms={} last_ms={} manifest={}",
                m.created_unix_s,
                m.node_id,
                m.generation,
                m.source_len,
                m.segments.len(),
                first.map(|s| s.start_offset).unwrap_or(0),
                last.map(|s| s.end_offset).unwrap_or(0),
                first.and_then(|s| s.first_index).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                last.and_then(|s| s.last_index).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                first.and_then(|s| s.first_ms).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                last.and_then(|s| s.last_ms).map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                m.manifest_relpath,
            )
        })
        .collect()
}

pub fn extent_summary_lines(segments: &[WalBackupSegment]) -> Vec<String> {
    segments
        .iter()
        .map(|s| {
            format!(
                "generation={} offsets={}..{} bytes={} line_boundary={}..{} index={}..{} ms={}..{} path={}",
                s.generation,
                s.start_offset,
                s.end_offset,
                s.bytes,
                s.starts_at_line_boundary,
                s.ends_at_line_boundary,
                s.first_index.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                s.last_index.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                s.first_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                s.last_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                s.relpath,
            )
        })
        .collect()
}

pub fn unix_now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_bool(raw: &str) -> anyhow::Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => bail!("invalid bool value {other:?}"),
    }
}

#[cfg(feature = "s3")]
fn parse_bool_lossy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_optional_bool_env(name: &str) -> anyhow::Result<Option<bool>> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| parse_bool(&v))
        .transpose()
}

fn parse_u64_env(name: &str, default: u64) -> anyhow::Result<u64> {
    std::env::var(name)
        .ok()
        .map(|v| v.parse().with_context(|| format!("parse {name}={v:?}")))
        .transpose()
        .map(|v| v.unwrap_or(default))
}

fn parse_optional_u64_env(name: &str) -> anyhow::Result<Option<u64>> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.parse().with_context(|| format!("parse {name}={v:?}")))
        .transpose()
}

fn parse_recovery_target_env(
    offset_env: &str,
    index_env: &str,
    ms_env: &str,
) -> anyhow::Result<RecoveryTarget> {
    let mut targets = Vec::new();
    if let Some(v) = parse_optional_u64_env(offset_env)? {
        targets.push(RecoveryTarget::Offset(v));
    }
    if let Some(v) = parse_optional_u64_env(index_env)? {
        targets.push(RecoveryTarget::Index(v));
    }
    if let Some(v) = parse_optional_u64_env(ms_env)? {
        targets.push(RecoveryTarget::Ms(v));
    }
    match targets.len() {
        0 => Ok(RecoveryTarget::Latest),
        1 => Ok(targets.remove(0)),
        _ => bail!("use at most one WAL restore recovery target"),
    }
}

fn parse_usize_env(name: &str, default: usize) -> anyhow::Result<usize> {
    std::env::var(name)
        .ok()
        .map(|v| v.parse().with_context(|| format!("parse {name}={v:?}")))
        .transpose()
        .map(|v| v.unwrap_or(default))
}

fn sanitize_component(raw: &str) -> String {
    let s = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if s.is_empty() { "node".to_string() } else { s }
}

fn normalize_prefix(prefix: &str) -> String {
    prefix.trim_matches('/').to_string()
}

#[cfg(feature = "s3")]
fn join_prefix(prefix: &str, rel: &str) -> String {
    let prefix = normalize_prefix(prefix);
    let rel = rel.trim_start_matches('/');
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

fn restore_s3_uri_from_env() -> anyhow::Result<Option<String>> {
    if let Some(uri) = std::env::var("SLOPMUD_WAL_RESTORE_S3_URI")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        let (bucket, prefix) = parse_s3_uri(&uri)?;
        return Ok(Some(format_s3_uri(&bucket, &prefix)));
    }
    let bucket = std::env::var("SLOPMUD_WAL_RESTORE_S3_BUCKET")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let prefix = std::env::var("SLOPMUD_WAL_RESTORE_S3_PREFIX")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_S3_PREFIX.to_string());
    Ok(bucket.map(|bucket| format_s3_uri(&bucket, &prefix)))
}

fn backup_s3_uri_from_env() -> anyhow::Result<Option<String>> {
    let bucket = std::env::var("SLOPMUD_WAL_BACKUP_S3_BUCKET")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let prefix = std::env::var("SLOPMUD_WAL_BACKUP_S3_PREFIX")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_S3_PREFIX.to_string());
    Ok(bucket.map(|bucket| format_s3_uri(&bucket, &prefix)))
}

fn restore_cache_dir_from_env(target_path: &Path) -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var("SLOPMUD_WAL_RESTORE_CACHE_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        return Ok(PathBuf::from(dir));
    }
    Ok(PathBuf::from(format!(
        "{}{}",
        target_path.display(),
        DEFAULT_RESTORE_CACHE_SUFFIX
    )))
}

fn restore_node_id_from_env() -> Option<String> {
    [
        "SLOPMUD_WAL_RESTORE_NODE_ID",
        "SLOPMUD_WAL_BACKUP_NODE_ID",
        "SHARD_RAFT_NODE_ID",
        "NODE_ID",
    ]
    .iter()
    .find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

fn format_s3_uri(bucket: &str, prefix: &str) -> String {
    let prefix = normalize_prefix(prefix);
    if prefix.is_empty() {
        format!("s3://{bucket}")
    } else {
        format!("s3://{bucket}/{prefix}")
    }
}

fn tmp_path(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("walbackup");
    path.with_file_name(format!(".{name}.{suffix}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "slopmud_walbackup_{label}_{}_{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn append_wal(path: &Path, entries: &[(u64, u64, &str)]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        for (index, ms, body) in entries {
            writeln!(
                f,
                "{{\"index\":{index},\"term\":1,\"ms\":{ms},\"entry\":{{\"body\":\"{body}\"}}}}"
            )
            .unwrap();
        }
    }

    fn store(source: PathBuf, root: PathBuf, max_segment_bytes: u64) -> WalBackupStore {
        WalBackupStore::new(source, root, "n/0".to_string(), max_segment_bytes, 10)
    }

    #[test]
    fn streaming_backup_writes_raw_extents_and_latest_manifest() {
        let dir = temp_dir("streaming");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b")]);
        let mut store = store(source.clone(), root.clone(), 1024);

        let result = store.sync_once(100).unwrap();
        assert!(result.changed);
        assert_eq!(result.new_segments.len(), 1);
        let manifest = store.load_latest_manifest().unwrap().unwrap();
        assert_eq!(
            manifest.source_len,
            std::fs::metadata(&source).unwrap().len()
        );
        assert_eq!(manifest.segments[0].first_index, Some(1));
        assert_eq!(manifest.segments[0].last_index, Some(2));
        let segment_hash = file_sha256_hex(&root.join(&manifest.segments[0].relpath)).unwrap();
        assert_eq!(
            manifest.segments[0].sha256_hex.as_deref(),
            Some(segment_hash.as_str())
        );
        assert_eq!(
            std::fs::read_to_string(root.join(&manifest.segments[0].relpath)).unwrap(),
            std::fs::read_to_string(source).unwrap()
        );
    }

    #[test]
    fn streaming_backup_is_idempotent_when_wal_unchanged() {
        let dir = temp_dir("idempotent");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a")]);
        let mut store = store(source, root, 1024);

        let first = store.sync_once(100).unwrap();
        assert!(first.changed);
        let second = store.sync_once(160).unwrap();
        assert!(!second.changed);
        assert!(second.new_segments.is_empty());
    }

    #[test]
    fn streaming_backup_splits_large_wal_into_bounded_extents() {
        let dir = temp_dir("split");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "abcdef"), (2, 20, "ghijkl")]);
        let mut store = store(source.clone(), root, 40);

        let result = store.sync_once(100).unwrap();
        assert!(result.new_segments.len() >= 2);
        let manifest = result.manifest.unwrap();
        assert_eq!(
            manifest.source_len,
            std::fs::metadata(source).unwrap().len()
        );
        assert_eq!(manifest.segments.first().unwrap().start_offset, 0);
        assert_eq!(
            manifest.segments.last().unwrap().end_offset,
            manifest.source_len
        );
    }

    #[test]
    fn streaming_backup_rotates_generation_after_wal_truncation() {
        let dir = temp_dir("truncate");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b")]);
        let mut store = store(source.clone(), root, 1024);
        let first = store.sync_once(100).unwrap().manifest.unwrap();
        std::fs::write(&source, "").unwrap();
        append_wal(&source, &[(1, 30, "after")]);

        let second = store.sync_once(200).unwrap();
        assert!(second.generation_reset);
        let second = second.manifest.unwrap();
        assert_eq!(second.generation, first.generation + 1);
        assert_eq!(second.segments[0].start_offset, 0);
        assert_eq!(second.segments[0].first_ms, Some(30));
    }

    #[test]
    fn manifest_retention_prunes_unreferenced_old_generation_segments() {
        let dir = temp_dir("prune_segments");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a")]);
        let mut store =
            WalBackupStore::new(source.clone(), root.clone(), "n0".to_string(), 1024, 1);
        let first = store.sync_once(100).unwrap().manifest.unwrap();
        let old_segment = root.join(&first.segments[0].relpath);
        assert!(old_segment.exists());

        std::fs::write(&source, "").unwrap();
        append_wal(&source, &[(1, 20, "after")]);
        store.sync_once(200).unwrap();

        assert!(
            !old_segment.exists(),
            "old generation segment should be pruned after its manifest ages out"
        );
    }

    #[test]
    fn list_manifests_and_extents_are_sorted_and_human_readable() {
        let dir = temp_dir("list");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        let mut store = store(source.clone(), root, 1024);
        append_wal(&source, &[(1, 10, "a")]);
        store.sync_once(100).unwrap();
        append_wal(&source, &[(2, 20, "b")]);
        store.sync_once(200).unwrap();

        let manifests = store.list_manifests().unwrap();
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].created_unix_s, 100);
        assert_eq!(manifests[1].created_unix_s, 200);
        let lines = manifest_summary_lines(&manifests);
        assert!(lines[1].contains("source_len="));
        let extents = store.latest_extents().unwrap();
        let extent_lines = extent_summary_lines(&extents);
        assert!(extent_lines.iter().any(|l| l.contains("path=")));
    }

    #[test]
    fn generic_listing_discovers_manifests_without_node_hint_and_selects_cutoff() {
        let dir = temp_dir("generic_list");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        let mut store = store(source.clone(), root.clone(), 1024);
        append_wal(&source, &[(1, 10, "a")]);
        store.sync_once(100).unwrap();
        append_wal(&source, &[(2, 20, "b")]);
        store.sync_once(200).unwrap();

        let manifests = list_local_manifests(&root).unwrap();
        assert_eq!(manifests.len(), 2);
        assert_eq!(
            select_manifest_from_list(&manifests, Some(150))
                .unwrap()
                .created_unix_s,
            100
        );
        assert_eq!(
            select_manifest_from_list(&manifests, None)
                .unwrap()
                .created_unix_s,
            200
        );
    }

    #[test]
    fn manifest_selection_can_filter_by_node_id() {
        let dir = temp_dir("node_filter");
        let source_a = dir.join("a.jsonl");
        let source_b = dir.join("b.jsonl");
        let root = dir.join("backup");
        append_wal(&source_a, &[(1, 10, "a")]);
        append_wal(&source_b, &[(1, 20, "b")]);
        let mut store_a = WalBackupStore::new(source_a, root.clone(), "n/0".to_string(), 1024, 10);
        let mut store_b = WalBackupStore::new(source_b, root.clone(), "n1".to_string(), 1024, 10);
        store_a.sync_once(100).unwrap();
        store_b.sync_once(200).unwrap();

        let manifests = list_local_manifests(&root).unwrap();
        assert_eq!(
            select_manifest_from_list(&manifests, None).unwrap().node_id,
            "n1"
        );
        let selected = select_manifest_from_list_for_node(&manifests, None, Some("n/0")).unwrap();
        assert_eq!(selected.node_id, "n_0");
        assert_eq!(selected.created_unix_s, 100);
        assert!(select_manifest_from_list_for_node(&manifests, None, Some("missing")).is_none());
    }

    #[test]
    fn recovery_by_offset_reassembles_requested_prefix() {
        let dir = temp_dir("recover_offset");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b")]);
        let mut store = store(source.clone(), root, 16);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        let raw = std::fs::read(&source).unwrap();
        let out = dir.join("recover.jsonl");

        let wrote = store
            .recover_to_path(&manifest, &out, RecoveryTarget::Offset(12))
            .unwrap();
        assert_eq!(wrote, 12);
        assert_eq!(std::fs::read(out).unwrap(), raw[..12].to_vec());
    }

    #[test]
    fn recovery_by_index_stops_before_later_entry() {
        let dir = temp_dir("recover_index");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b"), (3, 30, "c")]);
        let mut store = store(source, root, 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        let out = dir.join("recover.jsonl");

        store
            .recover_to_path(&manifest, &out, RecoveryTarget::Index(2))
            .unwrap();
        let recovered = std::fs::read_to_string(out).unwrap();
        assert!(recovered.contains("\"index\":1"));
        assert!(recovered.contains("\"index\":2"));
        assert!(!recovered.contains("\"index\":3"));
    }

    #[test]
    fn recovery_by_ms_stops_before_later_entry() {
        let dir = temp_dir("recover_ms");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b"), (3, 30, "c")]);
        let mut store = store(source, root, 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        let out = dir.join("recover.jsonl");

        store
            .recover_to_path(&manifest, &out, RecoveryTarget::Ms(20))
            .unwrap();
        let recovered = std::fs::read_to_string(out).unwrap();
        assert!(recovered.contains("\"ms\":10"));
        assert!(recovered.contains("\"ms\":20"));
        assert!(!recovered.contains("\"ms\":30"));
    }

    #[test]
    fn verify_catches_missing_segment() {
        let dir = temp_dir("verify_missing");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a")]);
        let mut store = store(source, root.clone(), 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        std::fs::remove_file(root.join(&manifest.segments[0].relpath)).unwrap();

        let err = store.verify_latest().unwrap_err().to_string();
        assert!(err.contains("missing wal backup segment"));
    }

    #[test]
    fn verify_catches_same_length_corrupt_segment() {
        let dir = temp_dir("verify_corrupt");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a")]);
        let mut store = store(source, root.clone(), 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        let segment_path = root.join(&manifest.segments[0].relpath);
        let mut raw = std::fs::read(&segment_path).unwrap();
        raw[0] = if raw[0] == b'{' { b'[' } else { b'{' };
        std::fs::write(&segment_path, raw).unwrap();

        let err = store.verify_latest().unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"));
    }

    #[test]
    fn recovery_rejects_corrupt_segment_before_replacing_output() {
        let dir = temp_dir("recover_corrupt");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        append_wal(&source, &[(1, 10, "a")]);
        let mut store = store(source, root.clone(), 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();
        let out = dir.join("recover.jsonl");
        std::fs::write(&out, b"keep me\n").unwrap();
        let segment_path = root.join(&manifest.segments[0].relpath);
        let mut raw = std::fs::read(&segment_path).unwrap();
        raw[0] = if raw[0] == b'{' { b'[' } else { b'{' };
        std::fs::write(&segment_path, raw).unwrap();

        let err = store
            .recover_to_path(&manifest, &out, RecoveryTarget::Latest)
            .unwrap_err()
            .to_string();
        assert!(err.contains("checksum mismatch"));
        assert_eq!(std::fs::read_to_string(out).unwrap(), "keep me\n");
    }

    #[tokio::test]
    async fn restore_from_local_backup_is_idempotent_and_can_restore_latest() {
        let dir = temp_dir("restore_local");
        let source = dir.join("raft.jsonl");
        let root = dir.join("backup");
        let out = dir.join("restored.jsonl");
        append_wal(&source, &[(1, 10, "a"), (2, 20, "b")]);
        let mut store = store(source.clone(), root.clone(), 1024);
        let manifest = store.sync_once(100).unwrap().manifest.unwrap();

        let cfg = WalRestoreConfig {
            target_path: out.clone(),
            source: WalRestoreSource::Local(root),
            node_id: None,
            target: RecoveryTarget::Latest,
            manifest_unix_at_or_before: None,
            overwrite_existing: false,
            missing_manifest_ok: false,
        };
        let outcome = restore_wal_from_backup(&cfg).await.unwrap();
        assert_eq!(
            outcome,
            WalRestoreOutcome::Restored {
                path: out.clone(),
                bytes: manifest.source_len,
                source_len: manifest.source_len,
                manifest_relpath: manifest.manifest_relpath,
            }
        );
        assert_eq!(
            std::fs::read(&out).unwrap(),
            std::fs::read(&source).unwrap()
        );

        let second = restore_wal_from_backup(&cfg).await.unwrap();
        assert_eq!(
            second,
            WalRestoreOutcome::SkippedExisting {
                path: out,
                bytes: manifest.source_len,
            }
        );
    }

    #[tokio::test]
    async fn restore_from_local_backup_can_select_node_manifest() {
        let dir = temp_dir("restore_node");
        let source_a = dir.join("a.jsonl");
        let source_b = dir.join("b.jsonl");
        let root = dir.join("backup");
        let out = dir.join("restored.jsonl");
        append_wal(&source_a, &[(1, 10, "a")]);
        append_wal(&source_b, &[(1, 20, "b")]);
        let mut store_a =
            WalBackupStore::new(source_a.clone(), root.clone(), "n/0".to_string(), 1024, 10);
        let mut store_b =
            WalBackupStore::new(source_b.clone(), root.clone(), "n1".to_string(), 1024, 10);
        let manifest_a = store_a.sync_once(100).unwrap().manifest.unwrap();
        store_b.sync_once(200).unwrap();

        let cfg = WalRestoreConfig {
            target_path: out.clone(),
            source: WalRestoreSource::Local(root),
            node_id: Some("n/0".to_string()),
            target: RecoveryTarget::Latest,
            manifest_unix_at_or_before: None,
            overwrite_existing: false,
            missing_manifest_ok: false,
        };
        let outcome = restore_wal_from_backup(&cfg).await.unwrap();
        assert_eq!(
            outcome,
            WalRestoreOutcome::Restored {
                path: out.clone(),
                bytes: manifest_a.source_len,
                source_len: manifest_a.source_len,
                manifest_relpath: manifest_a.manifest_relpath,
            }
        );
        assert_eq!(
            std::fs::read(&out).unwrap(),
            std::fs::read(&source_a).unwrap()
        );
    }

    #[test]
    fn parse_s3_uri_normalizes_prefix() {
        let (bucket, prefix) = parse_s3_uri("s3://bucket/a/b/").unwrap();
        assert_eq!(bucket, "bucket");
        assert_eq!(prefix, "a/b");
    }
}
