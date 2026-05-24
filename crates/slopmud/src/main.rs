use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use argon2::Argon2;
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use compliance::LogStream;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use memchr::memchr;
use mudproto::session::SessionId;
use mudproto::shard::{
    REQ_ATTACH, REQ_DETACH, REQ_INPUT, REQ_INPUT_BLOB, REQ_INPUT_IDEMPOTENT, ShardResp,
    build_input_idempotent_body,
};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use slopio::frame::{FrameReader, FrameWriter};
use slopio::telnet::IacParser;
use slopio::writev::write_all_bytes_vectored;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tracing::{Level, info, warn};
use zeroize::Zeroize;

use crate::commands::{handle_who_command, handle_whoami_command};

mod ban;
mod commands;
mod email;
mod eventlog;
mod hold;
mod kzc;
mod nearline;

const LOGIN_BACKOFF_BASE: Duration = Duration::from_secs(1);
const LOGIN_BACKOFF_MAX: Duration = Duration::from_secs(30);
const LOGIN_BACKOFF_RESET_AFTER: Duration = Duration::from_secs(10 * 60);
const LOGIN_THROTTLE_MAX_IPS: usize = 2048;
const LOGIN_THROTTLE_MAX_NAMES: usize = 2048;

const SCROLLBACK_MAX_LINES: usize = 1500;
const SCROLLBACK_MAX_LINE_CHARS: usize = 512;

// Snowflake-style 64-bit IDs for scrollback lines:
//   42 bits: milliseconds since LINEID_EPOCH_UNIX_MS
//   10 bits: node id (derived from NODE_ID)
//   12 bits: per-ms sequence
const LINEID_EPOCH_UNIX_MS: u64 = 1704067200000; // 2024-01-01T00:00:00Z

const REPORT_LAST_DEFAULT: usize = 30;
const REPORT_LAST_MAX: usize = 200;
const REPORT_SEARCH_LIMIT: usize = 20;
const REPORT_CONTEXT_LINES: usize = 3;
const REPORT_NOTE_MAX_CHARS: usize = 500;
const BLOB_STREAM_CHUNK_BYTES: usize = 1024 * 1024;
const MAX_DECLARED_BLOB_BYTES: u64 = 8 * 1024 * 1024 * 1024;

const REPORT_REASONS: &[(&str, &str)] = &[
    ("bullying", "Bullying / harassment"),
    ("threats_violence", "Threats / violence"),
    ("self_harm", "Self-harm / suicide"),
    ("sexual", "Sexual content"),
    ("hate", "Hate or harassment"),
    ("impersonation", "Impersonation / fraud"),
    ("spam", "Spam / scam"),
    ("other", "Other"),
];

fn backoff_delay(failures: u32) -> Duration {
    // 1s, 2s, 4s, ... capped.
    let pow = failures.saturating_sub(1).min(16);
    let mult: u32 = 1u32.checked_shl(pow).unwrap_or(u32::MAX);
    LOGIN_BACKOFF_BASE
        .checked_mul(mult)
        .unwrap_or(LOGIN_BACKOFF_MAX)
        .min(LOGIN_BACKOFF_MAX)
}

fn wait_seconds(d: Duration) -> u64 {
    let ms = d.as_millis();
    if ms == 0 {
        0
    } else {
        // Round up to whole seconds so the user doesn't retry too early.
        let s = (ms + 999) / 1000;
        u64::try_from(s).unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone, Copy)]
struct ThrottleEntry {
    failures: u32,
    last_failure: std::time::Instant,
    next_allowed: std::time::Instant,
}

#[derive(Debug, Default)]
struct LoginThrottle {
    by_ip: HashMap<IpAddr, ThrottleEntry>,
    by_name: HashMap<String, ThrottleEntry>,
}

impl LoginThrottle {
    fn prune(&mut self, now: std::time::Instant) {
        self.by_ip
            .retain(|_, e| now.duration_since(e.last_failure) <= LOGIN_BACKOFF_RESET_AFTER);
        self.by_name
            .retain(|_, e| now.duration_since(e.last_failure) <= LOGIN_BACKOFF_RESET_AFTER);

        // Safety caps to avoid unbounded growth under attack.
        if self.by_name.len() > LOGIN_THROTTLE_MAX_NAMES {
            self.by_name.clear();
        }
        if self.by_ip.len() > LOGIN_THROTTLE_MAX_IPS {
            self.by_ip.clear();
        }
    }

    fn wait(&mut self, ip: IpAddr, name: &str, now: std::time::Instant) -> Duration {
        self.prune(now);

        let mut wait = Duration::from_secs(0);
        if let Some(e) = self.by_ip.get(&ip) {
            if now < e.next_allowed {
                wait = wait.max(e.next_allowed.saturating_duration_since(now));
            }
        }
        if !name.is_empty() {
            if let Some(e) = self.by_name.get(name) {
                if now < e.next_allowed {
                    wait = wait.max(e.next_allowed.saturating_duration_since(now));
                }
            }
        }
        wait
    }

    fn note_failure(&mut self, ip: IpAddr, name: &str, now: std::time::Instant) -> Duration {
        self.prune(now);

        let ip_delay = {
            let e = self.by_ip.entry(ip).or_insert(ThrottleEntry {
                failures: 0,
                last_failure: now,
                next_allowed: now,
            });
            e.failures = e.failures.saturating_add(1);
            let d = backoff_delay(e.failures);
            e.last_failure = now;
            e.next_allowed = now + d;
            d
        };

        let name_delay = if name.is_empty() {
            Duration::from_secs(0)
        } else {
            let e = self
                .by_name
                .entry(name.to_string())
                .or_insert(ThrottleEntry {
                    failures: 0,
                    last_failure: now,
                    next_allowed: now,
                });
            e.failures = e.failures.saturating_add(1);
            let d = backoff_delay(e.failures);
            e.last_failure = now;
            e.next_allowed = now + d;
            d
        };

        ip_delay.max(name_delay)
    }

    fn note_success(&mut self, ip: IpAddr, name: &str) {
        self.by_ip.remove(&ip);
        if !name.is_empty() {
            self.by_name.remove(name);
        }
    }
}

#[derive(Clone)]
struct ServerInfo {
    started_instant: std::time::Instant,
    started_unix: u64,
    shard_addr: String,
    shard_addrs: Vec<String>,
    bind: SocketAddr,
}

const COC_LINE_ITEMS: [&str; 8] = [
    "1. nothing illegal",
    "2. hard R for violence, hard PG for sex/nudity",
    "3. no soliciting",
    "4. anything you submit - consider it publicly licensed and publicly published",
    "5. don't spam",
    "6. prioritize great experiences for humans",
    "7. don't lie about being a bot",
    "8. zero privacy (except passwords): we will share logs with various folks and train our models on them",
];

const PUBLIC_ACK_VERSION: u32 = 1;
const COC_ACK_VERSION: u32 = 1;
const LEGACY_PUBLIC_ACK_VERSION: u32 = 1;
const LEGACY_COC_ACK_VERSION: u32 = 1;

fn default_legacy_public_ack_version() -> u32 {
    LEGACY_PUBLIC_ACK_VERSION
}

fn default_legacy_coc_ack_version() -> u32 {
    LEGACY_COC_ACK_VERSION
}

const RACE_TOKENS: [&str; 9] = [
    "dragonborn",
    "dwarf",
    "elf",
    "gnome",
    "goliath",
    "halfling",
    "human",
    "orc",
    "tiefling",
];

const CLASS_TOKENS: [&str; 12] = [
    "barbarian",
    "bard",
    "cleric",
    "druid",
    "fighter",
    "monk",
    "paladin",
    "ranger",
    "rogue",
    "sorcerer",
    "warlock",
    "wizard",
];

fn is_allowed_token(s: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|x| *x == s)
}

fn normalize_pronouns(locale: &str, s: &str) -> Option<&'static str> {
    // Locale-specific aliases can go here. For now we support a minimal English set.
    // Return canonical key.
    let lc = s.trim().to_ascii_lowercase();
    if lc.is_empty() {
        return None;
    }
    match locale {
        "en" | "en-us" | "en_us" => match lc.as_str() {
            "he" | "him" => Some("he"),
            "she" | "her" => Some("she"),
            "they" | "them" => Some("they"),
            _ => None,
        },
        _ => match lc.as_str() {
            "he" | "him" => Some("he"),
            "she" | "her" => Some("she"),
            "they" | "them" => Some("they"),
            _ => None,
        },
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "slopmud (session broker)\n\n\
USAGE:\n  slopmud [--bind HOST:PORT] [--shard-addr HOST:PORT]\n\n\
ENV:\n  SLOPMUD_BIND               default 0.0.0.0:4000\n  SHARD_ADDR                 default 127.0.0.1:5000\n  SHARD_ADDRS                optional comma-separated shard failover list\n  NODE_ID                    optional (for logs only)\n  SLOPMUD_ACCOUNTS_PATH       optional; default accounts.json (in WorkingDirectory)\n  SLOPMUD_LOCALE              optional; default en\n  SLOPMUD_ADMIN_BIND          optional; default 127.0.0.1:4011 (local admin JSON)\n  SLOPMUD_BANS_PATH           optional; default locks/bans.json\n  SBC_ADMIN_SOCK              optional; default /run/slopmud/sbc-admin.sock\n  SBC_EVENTS_SOCK             optional; default /run/slopmud/sbc-events.sock\n  SLOPMUD_SBC_ENABLED         optional; default 1 (set 0/false/no/off to skip SBC event subscriber)\n  SLOPMUD_EMAIL_MODE          optional; default disabled (disabled | ses | smtp | file)\n  SLOPMUD_EMAIL_FROM          required for ses/smtp; optional for file\n  SLOPMUD_SMTP_HOST           required for smtp\n  SLOPMUD_SMTP_PORT           optional; default 587\n  SLOPMUD_SMTP_USERNAME       optional\n  SLOPMUD_SMTP_PASSWORD       optional\n  SLOPMUD_EMAIL_FILE_DIR      optional; default /tmp/slopmud_email_outbox\n  SLOPMUD_EVENTLOG_ENABLED    optional; default 0\n  SLOPMUD_EVENTLOG_SPOOL_DIR  optional; default locks/eventlog\n  SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S optional; default 60\n  SLOPMUD_EVENTLOG_S3_BUCKET  optional; if set, uploads target this bucket\n  SLOPMUD_EVENTLOG_S3_PREFIX  optional; default slopmud/eventlog\n  SLOPMUD_EVENTLOG_UPLOAD_ENABLED optional; default 0\n  SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL optional; default 1\n  SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S optional; default 600\n  SLOPMUD_NEARLINE_ENABLED    optional; default 1\n  SLOPMUD_NEARLINE_DIR        optional; default locks/nearline_scrollback\n  SLOPMUD_NEARLINE_MAX_SEGMENTS optional; default 12\n  SLOPMUD_NEARLINE_SEGMENT_MAX_BYTES optional; default 2000000\n  SLOPMUD_GOOGLE_OAUTH_DIR    optional; default locks/google_oauth (shared with static_web)\n  SLOPMUD_GOOGLE_AUTH_BASE_URL optional; default http://127.0.0.1:8080 (where to open OAuth in browser)\n  SLOPMUD_OIDC_TOKEN_URL      optional; if set, mint a session token at login\n  SLOPMUD_OIDC_CLIENT_ID      required if token url set\n  SLOPMUD_OIDC_CLIENT_SECRET  required if token url set\n  SLOPMUD_OIDC_SCOPE          optional; default slopmud:session\n  SLOPMUD_WEBAUTH_JWT_SECRET optional; if set, WEB_AUTH must include valid HS256 JWT proof from slopmud_web\n"
    );
    std::process::exit(2);
}

fn parse_shard_target(raw: &str) -> String {
    let target = raw.trim().to_string();
    if target.is_empty() {
        usage_and_exit();
    }
    target
}

fn parse_shard_addrs(primary: &str, raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return vec![primary.to_string()];
    };
    let mut out = Vec::new();
    for part in raw.split(',') {
        let target = parse_shard_target(part);
        if !out.contains(&target) {
            out.push(target);
        }
    }
    if out.is_empty() {
        vec![primary.to_string()]
    } else {
        out
    }
}

fn parse_shard_addrs_env(primary: &str) -> Vec<String> {
    let raw = std::env::var("SHARD_ADDRS").ok();
    parse_shard_addrs(primary, raw.as_deref())
}

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    shard_addr: String,
    shard_addrs: Vec<String>,
    node_id: Option<String>,
    // Accounts DB (stores only password hashes, never raw passwords).
    accounts_path: String,
    // Directory used for cross-process OAuth handoffs (static_web writes results here).
    google_oauth_dir: String,
    // Base URL for the user to open in a browser for OAuth (points at static_web).
    google_auth_base_url: String,
    // If set, mint a session-scoped access token from an internal OIDC token endpoint.
    // The password is never sent to this service.
    #[allow(dead_code)]
    oidc_token_url: Option<String>,
    #[allow(dead_code)]
    oidc_client_id: Option<String>,
    #[allow(dead_code)]
    oidc_client_secret: Option<String>,
    #[allow(dead_code)]
    oidc_scope: Option<String>,
    webauth_jwt_secret: Option<String>,
    locale: String,

    admin_bind: SocketAddr,
    bans_path: PathBuf,
    sbc_admin_sock: PathBuf,
    sbc_events_sock: PathBuf,
    sbc_enabled: bool,
    #[allow(dead_code)]
    email: email::EmailConfig,
    eventlog: eventlog::EventLogConfig,
    nearline: nearline::NearlineConfig,
    blob_spool_dir: PathBuf,
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("SLOPMUD_BIND")
        .unwrap_or_else(|_| "0.0.0.0:4000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut shard_addr = parse_shard_target(
        &std::env::var("SHARD_ADDR").unwrap_or_else(|_| "127.0.0.1:5000".to_string()),
    );
    let mut shard_addrs = parse_shard_addrs_env(&shard_addr);

    let node_id = std::env::var("NODE_ID").ok();
    let accounts_path =
        std::env::var("SLOPMUD_ACCOUNTS_PATH").unwrap_or_else(|_| "accounts.json".to_string());
    let google_oauth_dir = std::env::var("SLOPMUD_GOOGLE_OAUTH_DIR")
        .unwrap_or_else(|_| "locks/google_oauth".to_string());
    let google_auth_base_url = std::env::var("SLOPMUD_GOOGLE_AUTH_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let oidc_token_url = std::env::var("SLOPMUD_OIDC_TOKEN_URL").ok();
    let oidc_client_id = std::env::var("SLOPMUD_OIDC_CLIENT_ID").ok();
    let oidc_client_secret = std::env::var("SLOPMUD_OIDC_CLIENT_SECRET").ok();
    let oidc_scope = std::env::var("SLOPMUD_OIDC_SCOPE").ok();
    let webauth_jwt_secret = std::env::var("SLOPMUD_WEBAUTH_JWT_SECRET")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let locale = std::env::var("SLOPMUD_LOCALE").unwrap_or_else(|_| "en".to_string());

    let admin_bind: SocketAddr = std::env::var("SLOPMUD_ADMIN_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4011".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let bans_path: PathBuf = std::env::var("SLOPMUD_BANS_PATH")
        .unwrap_or_else(|_| "locks/bans.json".to_string())
        .into();

    let sbc_admin_sock: PathBuf = std::env::var("SBC_ADMIN_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-admin.sock".to_string())
        .into();
    let sbc_events_sock: PathBuf = std::env::var("SBC_EVENTS_SOCK")
        .unwrap_or_else(|_| "/run/slopmud/sbc-events.sock".to_string())
        .into();
    let sbc_enabled = std::env::var("SLOPMUD_SBC_ENABLED")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "no" || v == "off")
        })
        .unwrap_or(true);

    let mut email = email::EmailConfig::default();
    email.mode = std::env::var("SLOPMUD_EMAIL_MODE").unwrap_or_else(|_| email.mode.clone());
    email.from = std::env::var("SLOPMUD_EMAIL_FROM").ok();
    email.smtp_host = std::env::var("SLOPMUD_SMTP_HOST").ok();
    email.smtp_port = std::env::var("SLOPMUD_SMTP_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(email.smtp_port);
    email.smtp_username = std::env::var("SLOPMUD_SMTP_USERNAME").unwrap_or_default();
    email.smtp_password = std::env::var("SLOPMUD_SMTP_PASSWORD").unwrap_or_default();
    if let Ok(v) = std::env::var("SLOPMUD_EMAIL_FILE_DIR") {
        if !v.trim().is_empty() {
            email.file_dir = v.into();
        }
    }

    let mut eventlog = eventlog::EventLogConfig::default();
    eventlog.enabled = std::env::var("SLOPMUD_EVENTLOG_ENABLED")
        .ok()
        .is_some_and(|v| v == "1");
    if let Ok(v) = std::env::var("SLOPMUD_EVENTLOG_SPOOL_DIR") {
        if !v.trim().is_empty() {
            eventlog.spool_dir = v.into();
        }
    }
    eventlog.flush_interval_s = std::env::var("SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(eventlog.flush_interval_s);
    eventlog.s3_bucket = std::env::var("SLOPMUD_EVENTLOG_S3_BUCKET")
        .ok()
        .filter(|v| !v.trim().is_empty());
    if let Ok(v) = std::env::var("SLOPMUD_EVENTLOG_S3_PREFIX") {
        if !v.trim().is_empty() {
            eventlog.s3_prefix = v;
        }
    }
    eventlog.upload_enabled = std::env::var("SLOPMUD_EVENTLOG_UPLOAD_ENABLED")
        .ok()
        .is_some_and(|v| v == "1");
    eventlog.upload_delete_local = !std::env::var("SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL")
        .ok()
        .is_some_and(|v| v == "0");
    eventlog.upload_scan_interval_s = std::env::var("SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(eventlog.upload_scan_interval_s);

    let mut nearline = nearline::NearlineConfig::default();
    nearline.enabled = !std::env::var("SLOPMUD_NEARLINE_ENABLED")
        .ok()
        .is_some_and(|v| v.trim() == "0");
    if let Ok(v) = std::env::var("SLOPMUD_NEARLINE_DIR") {
        if !v.trim().is_empty() {
            nearline.dir = v.into();
        }
    }
    nearline.max_segments = std::env::var("SLOPMUD_NEARLINE_MAX_SEGMENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(nearline.max_segments);
    nearline.segment_max_bytes = std::env::var("SLOPMUD_NEARLINE_SEGMENT_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(nearline.segment_max_bytes);
    let blob_spool_dir: PathBuf = std::env::var("SLOPMUD_BLOB_SPOOL_DIR")
        .unwrap_or_else(|_| "locks/blob_spool".to_string())
        .into();

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--shard-addr" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                shard_addr = parse_shard_target(&v);
                shard_addrs = vec![shard_addr.clone()];
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        bind,
        shard_addr,
        shard_addrs,
        node_id,
        accounts_path,
        google_oauth_dir,
        google_auth_base_url,
        oidc_token_url,
        oidc_client_id,
        oidc_client_secret,
        oidc_scope,
        webauth_jwt_secret,
        locale,
        admin_bind,
        bans_path,
        sbc_admin_sock,
        sbc_events_sock,
        sbc_enabled,
        email,
        eventlog,
        nearline,
        blob_spool_dir,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LineId(u64);

impl LineId {
    fn encode(self) -> String {
        // Crockford-ish base32, lowercase, fixed-width (13 chars) for compactness + easy parsing.
        // Alphabet: 0-9 a-z without i, l, o, u (32 chars).
        const ALPH: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
        let mut v = self.0;
        let mut out = [b'0'; 13];
        for i in (0..13).rev() {
            out[i] = ALPH[(v & 31) as usize];
            v >>= 5;
        }
        String::from_utf8_lossy(&out).to_string()
    }

    fn decode(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('@');
        if s.is_empty() {
            return None;
        }

        // Fast-path: decimal (legacy/testing).
        if s.as_bytes().iter().all(|c| c.is_ascii_digit()) {
            return s.parse::<u64>().ok().map(LineId);
        }

        fn val(c: char) -> Option<u8> {
            match c {
                '0' => Some(0),
                '1' => Some(1),
                '2' => Some(2),
                '3' => Some(3),
                '4' => Some(4),
                '5' => Some(5),
                '6' => Some(6),
                '7' => Some(7),
                '8' => Some(8),
                '9' => Some(9),
                'a' => Some(10),
                'b' => Some(11),
                'c' => Some(12),
                'd' => Some(13),
                'e' => Some(14),
                'f' => Some(15),
                'g' => Some(16),
                'h' => Some(17),
                'j' => Some(18),
                'k' => Some(19),
                'm' => Some(20),
                'n' => Some(21),
                'p' => Some(22),
                'q' => Some(23),
                'r' => Some(24),
                's' => Some(25),
                't' => Some(26),
                'v' => Some(27),
                'w' => Some(28),
                'x' => Some(29),
                'y' => Some(30),
                'z' => Some(31),
                // Common confusions.
                'o' => Some(0),
                'i' | 'l' => Some(1),
                _ => None,
            }
        }

        let mut v: u64 = 0;
        for c in s.chars() {
            let c = c.to_ascii_lowercase();
            let d = val(c)?;
            v = v.checked_mul(32)?;
            v = v.checked_add(u64::from(d))?;
        }
        Some(LineId(v))
    }

    fn timestamp_unix_ms(self) -> Option<u64> {
        let delta_ms = self.0 >> 22;
        Some(LINEID_EPOCH_UNIX_MS.saturating_add(delta_ms))
    }
}

impl std::fmt::Display for LineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.encode())
    }
}

#[derive(Debug)]
struct LineIdGen {
    node_bits: u16,
    last_ts_ms: u64,
    seq: u16,
}

impl LineIdGen {
    fn new(node_id: Option<&str>) -> Self {
        let node_bits = node_id_bits(node_id);
        Self {
            node_bits,
            last_ts_ms: 0,
            seq: 0,
        }
    }

    fn next_id(&mut self, now_unix_ms: u64) -> LineId {
        let mut ts_ms = now_unix_ms.max(self.last_ts_ms);
        if ts_ms == self.last_ts_ms {
            self.seq = self.seq.wrapping_add(1) & 0x0fff;
            if self.seq == 0 {
                // Sequence wrapped within the same millisecond; bump time forward.
                ts_ms = ts_ms.saturating_add(1);
            }
        } else {
            self.seq = 0;
        }
        self.last_ts_ms = ts_ms;

        let delta_ms = ts_ms.saturating_sub(LINEID_EPOCH_UNIX_MS);
        let id = (delta_ms << 22)
            | ((u64::from(self.node_bits) & 0x03ff) << 12)
            | (u64::from(self.seq) & 0x0fff);
        LineId(id)
    }
}

fn node_id_bits(node_id: Option<&str>) -> u16 {
    let Some(node_id) = node_id.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        let mut b = [0u8; 2];
        getrandom::getrandom(&mut b).ok();
        return u16::from_be_bytes(b) & 0x03ff;
    };
    if let Ok(v) = node_id.parse::<u16>() {
        return v & 0x03ff;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    node_id.hash(&mut h);
    (h.finish() as u16) & 0x03ff
}

#[derive(Debug, Clone)]
struct ScrollLine {
    id: LineId,
    ts_unix_ms: u64,
    text: String,
}

#[derive(Debug)]
struct Scrollback {
    cap: usize,
    lines: VecDeque<ScrollLine>,
}

impl Scrollback {
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            lines: VecDeque::new(),
        }
    }

    fn push_line(&mut self, id: LineId, ts_unix_ms: u64, text: String) {
        self.lines.push_back(ScrollLine {
            id,
            ts_unix_ms,
            text,
        });
        while self.lines.len() > self.cap {
            let _ = self.lines.pop_front();
        }
    }

    fn last_n(&self, n: usize) -> Vec<ScrollLine> {
        let n = n.min(self.lines.len());
        let mut out = self.lines.iter().rev().take(n).cloned().collect::<Vec<_>>();
        out.reverse();
        out
    }

    fn search(&self, q: &str, limit: usize) -> Vec<ScrollLine> {
        let q = q.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let q_lc = q.to_ascii_lowercase();

        let mut out = Vec::new();
        for l in self.lines.iter().rev() {
            if l.text.to_ascii_lowercase().contains(&q_lc) {
                out.push(l.clone());
                if out.len() >= limit.max(1) {
                    break;
                }
            }
        }
        out
    }

    fn find_with_context(&self, id: LineId, ctx: usize) -> Option<(ScrollLine, Vec<ScrollLine>)> {
        let mut idx: Option<usize> = None;
        for (i, l) in self.lines.iter().enumerate() {
            if l.id == id {
                idx = Some(i);
                break;
            }
        }
        let idx = idx?;

        let start = idx.saturating_sub(ctx);
        let end = (idx + ctx).min(self.lines.len().saturating_sub(1));

        let mut context = Vec::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i < start {
                continue;
            }
            if i > end {
                break;
            }
            context.push(l.clone());
        }

        let target = context
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .or_else(|| self.lines.get(idx).cloned())?;
        Some((target, context))
    }
}

fn extract_output_lines(b: &[u8]) -> Vec<String> {
    let s = escape_log_text(b);
    let mut out = Vec::new();
    for raw in s.split('\n') {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        // Avoid cluttering searches with prompts.
        if line == ">" {
            continue;
        }
        out.push(line.to_string());
    }
    out
}

#[allow(dead_code)]
fn extract_scrollback_lines(b: &[u8]) -> Vec<String> {
    extract_output_lines(b)
        .into_iter()
        .map(|line| clamp_chars(&line, SCROLLBACK_MAX_LINE_CHARS))
        .collect()
}

fn fmt_hhmmss(ts_unix_ms: u64) -> String {
    let ts_ms = i64::try_from(ts_unix_ms).unwrap_or(0);
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .or_else(|| Utc.timestamp_millis_opt(0).single())
        .expect("timestamp millis");
    dt.format("%H:%M:%S").to_string()
}

fn report_usage_text() -> String {
    let mut s = String::new();
    s.push_str("report:\r\n");
    s.push_str(" - report last [n]\r\n");
    s.push_str(" - report search <text>\r\n");
    s.push_str(" - report reasons\r\n");
    s.push_str(" - report submit <line_id> <reason> [note...]\r\n");
    s.push_str(" - report locate <line_id>\r\n");
    s.push_str("\r\n");
    s.push_str(&format!(
        "notes:\r\n - hot scrollback stores your most recent ~{} output lines\r\n",
        SCROLLBACK_MAX_LINES
    ));
    s.push_str(" - nearline disk may keep more history (if enabled)\r\n");
    s.push_str(" - use `report search` or `report last` to find a line_id\r\n");
    s.push_str(" - reports are logged for review\r\n");
    s.push_str("\r\n");
    s.push_str("> ");
    s
}

fn report_reasons_text() -> String {
    let mut s = String::new();
    s.push_str("report reasons:\r\n");
    for (k, label) in REPORT_REASONS {
        s.push_str(&format!(" - {k}: {label}\r\n"));
    }
    s.push_str("\r\n> ");
    s
}

fn normalize_report_reason(s: &str) -> Option<&'static str> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    for (k, _) in REPORT_REASONS {
        if s == *k {
            return Some(*k);
        }
    }
    match s.as_str() {
        "harassment" => Some("bullying"),
        "bullying_harassment" => Some("bullying"),
        "threats" => Some("threats_violence"),
        "violence" => Some("threats_violence"),
        "threats/violence" => Some("threats_violence"),
        "suicide" => Some("self_harm"),
        "self-harm" => Some("self_harm"),
        "selfharm" => Some("self_harm"),
        "sex" => Some("sexual"),
        "nsfw" => Some("sexual"),
        "racism" => Some("hate"),
        "hate_speech" => Some("hate"),
        "impersonation/fraud" => Some("impersonation"),
        "scam" => Some("spam"),
        _ => None,
    }
}

fn clamp_chars(s: &str, max: usize) -> String {
    let s = s.trim();
    if max == 0 || s.is_empty() {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = s.chars().take(max).collect::<String>();
    out.push_str(" [truncated]");
    out
}

async fn handle_report_command(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    holds: &Arc<tokio::sync::Mutex<hold::HoldCache>>,
    nearline: &Arc<nearline::NearlineRing>,
    eventlog: &Arc<eventlog::EventLog>,
    session: SessionId,
    peer_ip: IpAddr,
    name: &str,
    line: &str,
) -> String {
    let scrollback = {
        let si = { sessions.lock().await.get(&session).cloned() };
        match si {
            Some(si) => si.scrollback,
            None => {
                return "report: not attached\r\n> ".to_string();
            }
        }
    };

    let mut it = line.split_whitespace();
    let _ = it.next(); // "report"
    let sub = it.next().unwrap_or("").to_ascii_lowercase();

    let held = holds.lock().await.is_held(name).is_some();

    match sub.as_str() {
        "" => report_usage_text(),
        "reasons" => report_reasons_text(),
        "last" => {
            let n = it
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(REPORT_LAST_DEFAULT)
                .clamp(1, REPORT_LAST_MAX);

            let mut s = String::new();
            s.push_str(&format!("report last {n}:\r\n"));
            let hot = {
                let sb = scrollback.lock().await;
                sb.last_n(n)
            };
            let mut printed = 0usize;

            if hot.len() >= n {
                for l in hot {
                    s.push_str(&format!(
                        " [{id} {ts}] {text}\r\n",
                        id = l.id,
                        ts = fmt_hhmmss(l.ts_unix_ms),
                        text = l.text
                    ));
                    printed = printed.saturating_add(1);
                }
            } else {
                let need = n.saturating_sub(hot.len());
                let hot_ids = hot
                    .iter()
                    .map(|l| l.id.to_string())
                    .collect::<std::collections::HashSet<_>>();

                let disk = nearline.last_n(name, n).await;
                let disk_empty = disk.is_empty();
                let mut older: Vec<(String, u64, String)> = Vec::new();
                if !disk_empty {
                    for l in disk {
                        if hot_ids.contains(&l.id) {
                            continue;
                        }
                        let text = if held { redact_pii(&l.text) } else { l.text };
                        older.push((l.id, l.ts_unix_ms, text));
                    }
                }

                let start = older.len().saturating_sub(need);
                for (id, ts_unix_ms, text) in older.into_iter().skip(start) {
                    s.push_str(&format!(
                        " [{id} {ts}] {text}\r\n",
                        id = id,
                        ts = fmt_hhmmss(ts_unix_ms),
                        text = text
                    ));
                    printed = printed.saturating_add(1);
                }
                for l in hot {
                    s.push_str(&format!(
                        " [{id} {ts}] {text}\r\n",
                        id = l.id,
                        ts = fmt_hhmmss(l.ts_unix_ms),
                        text = l.text
                    ));
                    printed = printed.saturating_add(1);
                }
            }

            if printed == 0 {
                s.push_str(" (scrollback empty)\r\n");
            }
            s.push_str("\r\n> ");
            s
        }
        "search" => {
            let q = it.collect::<Vec<_>>().join(" ").trim().to_string();
            if q.is_empty() {
                return "report search: missing text\r\n\r\n> ".to_string();
            }

            let mut s = String::new();
            s.push_str(&format!(
                "report search {q:?} (newest first; up to {lim}):\r\n",
                lim = REPORT_SEARCH_LIMIT
            ));
            let hot = {
                let sb = scrollback.lock().await;
                sb.search(&q, REPORT_SEARCH_LIMIT)
            };

            let mut out = Vec::new();
            let mut seen = std::collections::HashSet::<String>::new();
            for l in hot {
                let id = l.id.to_string();
                seen.insert(id.clone());
                out.push((id, l.ts_unix_ms, l.text));
                if out.len() >= REPORT_SEARCH_LIMIT {
                    break;
                }
            }

            if out.len() < REPORT_SEARCH_LIMIT {
                let disk = nearline.search(name, &q, REPORT_SEARCH_LIMIT).await;
                for l in disk {
                    if seen.contains(&l.id) {
                        continue;
                    }
                    seen.insert(l.id.clone());
                    let text = if held { redact_pii(&l.text) } else { l.text };
                    out.push((l.id, l.ts_unix_ms, text));
                    if out.len() >= REPORT_SEARCH_LIMIT {
                        break;
                    }
                }
            }

            for (id, ts_unix_ms, text) in out.iter() {
                s.push_str(&format!(
                    " [{id} {ts}] {text}\r\n",
                    id = id,
                    ts = fmt_hhmmss(*ts_unix_ms),
                    text = text
                ));
            }

            if out.is_empty() {
                s.push_str(" (no matches)\r\n");
            }
            s.push_str("\r\n> ");
            s
        }
        "locate" => {
            let Some(id) = it.next().and_then(LineId::decode) else {
                return "report locate: missing/bad line_id\r\n\r\n> ".to_string();
            };
            let Some(ts_unix_ms) = id.timestamp_unix_ms() else {
                return "report locate: failed to decode timestamp\r\n\r\n> ".to_string();
            };
            let ts_i64 = i64::try_from(ts_unix_ms).unwrap_or(0);
            let dt = Utc
                .timestamp_millis_opt(ts_i64)
                .single()
                .unwrap_or_else(|| Utc.timestamp_millis_opt(0).single().expect("ts 0"));

            let rel_char = compliance::object_relpath(LogStream::Character(name), dt);
            let rel_all = compliance::object_relpath(LogStream::All, dt);
            let local_char = eventlog.spool_path(&rel_char);
            let local_all = eventlog.spool_path(&rel_all);

            let mut s = String::new();
            s.push_str("report locate:\r\n");
            s.push_str(&format!(" - line_id: {id}\r\n"));
            s.push_str(&format!(" - ts_utc: {}\r\n", dt.to_rfc3339()));
            s.push_str(&format!(" - eventlog_char_relpath: {rel_char}\r\n"));
            s.push_str(&format!(
                " - eventlog_char_spool: {}\r\n",
                local_char.display()
            ));
            s.push_str(&format!(" - eventlog_all_relpath: {rel_all}\r\n"));
            s.push_str(&format!(
                " - eventlog_all_spool: {}\r\n",
                local_all.display()
            ));
            if let Some((bucket, key)) = eventlog.public_s3_key(&rel_char) {
                s.push_str(&format!(" - s3_bucket: {bucket}\r\n"));
                s.push_str(&format!(" - s3_key: {key}\r\n"));
            }
            s.push_str(" - grep: rg 'line_id=\"<id>\"' <file>\r\n");
            s.push_str("\r\n> ");
            s
        }
        "submit" => {
            let Some(id) = it.next().and_then(LineId::decode) else {
                return "report submit: missing/bad line_id\r\n\r\n> ".to_string();
            };
            let id_str = id.to_string();
            let reason_raw = it.next().unwrap_or("");
            let Some(reason) = normalize_report_reason(reason_raw) else {
                let mut s = String::new();
                s.push_str("report submit: bad reason\r\n");
                s.push_str("use: report reasons\r\n\r\n> ");
                return s;
            };
            let note = clamp_chars(&it.collect::<Vec<_>>().join(" "), REPORT_NOTE_MAX_CHARS);

            let mut source = "hot";
            let target_ts_unix_ms: u64;
            let target_text: String;
            let mut context_lines: Vec<(String, u64, String)> = Vec::new();

            // 1) Hot in-memory scrollback for this session.
            if let Some((target, context)) = {
                let sb = scrollback.lock().await;
                sb.find_with_context(id, REPORT_CONTEXT_LINES)
            } {
                target_ts_unix_ms = target.ts_unix_ms;
                target_text = target.text.clone();
                for l in &context {
                    context_lines.push((l.id.to_string(), l.ts_unix_ms, l.text.clone()));
                }
            } else if let Some((target, context)) = nearline
                .find_with_context(name, &id_str, REPORT_CONTEXT_LINES)
                .await
            {
                // 2) Nearline disk ring buffer (filtered by name).
                source = "nearline";
                target_ts_unix_ms = target.ts_unix_ms;
                target_text = target.text.clone();
                for l in &context {
                    context_lines.push((l.id.clone(), l.ts_unix_ms, l.text.clone()));
                }
            } else {
                return format!("report submit: unknown line_id {id_str}\r\n\r\n> ");
            }

            let target_text_log = if held {
                redact_pii(&target_text)
            } else {
                target_text.clone()
            };
            let note_log = if held {
                redact_pii(&note)
            } else {
                note.clone()
            };
            let target_text_view = if held && source != "hot" {
                target_text_log.clone()
            } else {
                target_text.clone()
            };

            let now = Utc::now();
            let ts = now.to_rfc3339();
            let sid = session_hex(session);
            let sid_short = sid.get(0..8).unwrap_or(sid.as_str());
            let report_id = format!("rep-{}-{sid_short}-{id_str}", now.timestamp());

            let mut ctx_s = String::new();
            for (cid, cts_ms, ctext) in &context_lines {
                let ctext = if held {
                    redact_pii(ctext)
                } else {
                    ctext.clone()
                };
                ctx_s.push_str(&format!(
                    "[{id} {ts}] {text}\n",
                    id = cid,
                    ts = fmt_hhmmss(*cts_ms),
                    text = ctext
                ));
            }
            ctx_s = ctx_s.trim_end().to_string();

            let entry = format!(
                "ts={} kind=abuse_report report_id={} reporter_session={} reporter_ip={} reporter_name={} reported_line_id={} reported_line_ts_unix_ms={} reported_text={} reason={} note={} source={} context={}",
                logfmt_str(&ts),
                logfmt_str(&report_id),
                logfmt_str(&sid),
                logfmt_str(&peer_ip.to_string()),
                logfmt_str(name),
                logfmt_str(&id_str),
                logfmt_str(&target_ts_unix_ms.to_string()),
                logfmt_str(&target_text_log),
                logfmt_str(reason),
                logfmt_str(&note_log),
                logfmt_str(source),
                logfmt_str(&ctx_s),
            );

            // Always emit a warning so reports are visible even if eventlog is disabled.
            warn!(
                report_id = %report_id,
                reporter_name = %name,
                reporter_ip = %peer_ip,
                reporter_session = %sid,
                reported_line_id = %id_str,
                reason = %reason,
                reported_text = %clamp_chars(&target_text_log, 120),
                "abuse report submitted"
            );

            eventlog.log_line(LogStream::Reports, &entry).await;
            eventlog.log_line(LogStream::All, &entry).await;
            if !name.trim().is_empty() {
                eventlog.log_line(LogStream::Character(name), &entry).await;
            }

            let mut s = String::new();
            s.push_str("report submitted:\r\n");
            s.push_str(&format!(" - report_id: {report_id}\r\n"));
            s.push_str(&format!(
                " - line: [{id} {}] {}\r\n",
                fmt_hhmmss(target_ts_unix_ms),
                target_text_view
            ));
            s.push_str(&format!(" - reason: {reason}\r\n"));
            if !note.is_empty() {
                s.push_str(&format!(" - note: {note}\r\n"));
            }
            s.push_str("\r\n> ");
            s
        }
        _ => report_usage_text(),
    }
}

fn account_usage_text() -> String {
    let mut s = String::new();
    s.push_str("account:\r\n");
    s.push_str("use:\r\n");
    s.push_str(" - account email\r\n");
    s.push_str(" - account email set <addr>\r\n");
    s.push_str(" - account email clear\r\n");
    s.push_str("\r\n> ");
    s
}

fn accounthold_usage_text() -> String {
    let mut s = String::new();
    s.push_str("accounthold:\r\n");
    s.push_str("use:\r\n");
    s.push_str(" - accounthold list\r\n");
    s.push_str(" - accounthold show <name>\r\n");
    s.push_str(" - accounthold add <name> [reason...]\r\n");
    s.push_str(" - accounthold del <name>\r\n");
    s.push_str("\r\n> ");
    s
}

async fn sbc_send_admin_req(
    sock: &PathBuf,
    req: &sbc_core::AdminReq,
) -> anyhow::Result<sbc_core::AdminResp> {
    let mut stream = UnixStream::connect(sock)
        .await
        .map_err(|e| anyhow::anyhow!("connect sbc admin sock {}: {e}", sock.display()))?;
    stream
        .write_all(serde_json::to_string(req)?.as_bytes())
        .await?;
    stream.write_all(b"\n").await?;
    let (rd, _) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    let raw = line.trim();
    if raw.is_empty() {
        return Err(anyhow::anyhow!("empty sbc admin response"));
    }
    Ok(serde_json::from_str(raw)?)
}

async fn sbc_holds_events_task(
    events_sock: PathBuf,
    holds: Arc<tokio::sync::Mutex<hold::HoldCache>>,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
) {
    let sub = sbc_core::EventsReq::Subscribe {
        mode: sbc_core::SubscribeMode::Snapshot,
    };

    loop {
        let mut stream = match UnixStream::connect(&events_sock).await {
            Ok(s) => s,
            Err(e) => {
                warn!(err=%e, path=%events_sock.display(), "sbc events connect failed");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        if let Err(e) = stream
            .write_all(serde_json::to_string(&sub).unwrap().as_bytes())
            .await
        {
            warn!(err=%e, "failed to subscribe to sbc events");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
        let _ = stream.write_all(b"\n").await;

        let (rd, _) = stream.into_split();
        let mut rd = BufReader::new(rd);
        let mut line = String::new();

        loop {
            line.clear();
            match rd.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    warn!(err=%e, "sbc events read failed");
                    break;
                }
            }
            let raw = line.trim();
            if raw.is_empty() {
                continue;
            }
            let env: sbc_core::EventEnvelope = match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => {
                    warn!(err=%e, line=%raw, "bad sbc event json");
                    continue;
                }
            };

            match env.event {
                sbc_core::Event::Snapshot { holds: h, .. } => {
                    let held_names = h
                        .iter()
                        .map(|e| e.name_lc.trim().to_ascii_lowercase())
                        .filter(|k| !k.is_empty())
                        .collect::<std::collections::HashSet<_>>();

                    holds.lock().await.apply_snapshot(env.index, h);

                    let mut m = sessions.lock().await;
                    for si in m.values_mut() {
                        let k = si.name.trim().to_ascii_lowercase();
                        si.held = held_names.contains(&k);
                    }
                }
                sbc_core::Event::LegalHoldUpserted { entry } => {
                    let name_lc = entry.name_lc.trim().to_ascii_lowercase();
                    holds.lock().await.apply_upsert(env.index, entry);

                    if !name_lc.is_empty() {
                        let mut m = sessions.lock().await;
                        for si in m.values_mut() {
                            if si.name.trim().to_ascii_lowercase() == name_lc {
                                si.held = true;
                            }
                        }
                    }
                }
                sbc_core::Event::LegalHoldDeleted { name_lc } => {
                    let name_lc = name_lc.trim().to_ascii_lowercase();
                    holds.lock().await.apply_delete(env.index, &name_lc);

                    if !name_lc.is_empty() {
                        let mut m = sessions.lock().await;
                        for si in m.values_mut() {
                            if si.name.trim().to_ascii_lowercase() == name_lc {
                                si.held = false;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn handle_accounthold_command(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    holds: &Arc<tokio::sync::Mutex<hold::HoldCache>>,
    sbc_admin_sock: &PathBuf,
    eventlog: &Arc<eventlog::EventLog>,
    peer_ip: IpAddr,
    session: SessionId,
    actor: &str,
    line: &str,
) -> String {
    if !peer_ip.is_loopback() {
        return "accounthold: permission denied\r\n\r\n> ".to_string();
    }

    let mut it = line.split_whitespace();
    let _ = it.next(); // "accounthold"
    let sub = it.next().unwrap_or("").to_ascii_lowercase();

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match sub.as_str() {
        "" | "help" => accounthold_usage_text(),
        "list" => {
            let entries = { holds.lock().await.snapshot() };
            let mut s = String::new();
            s.push_str("accounthold list:\r\n");
            if entries.is_empty() {
                s.push_str(" (none)\r\n");
            } else {
                for h in entries {
                    s.push_str(&format!(
                        " - {name} (created_unix={created_unix} by={by})\r\n",
                        name = h.name_lc,
                        created_unix = h.created_at_unix,
                        by = h.created_by
                    ));
                }
            }
            s.push_str("\r\n> ");
            s
        }
        "show" | "status" => {
            let raw = it.next().unwrap_or("");
            let nm = sanitize_name(raw);
            if nm.is_empty() {
                return "accounthold show: missing/bad name\r\n\r\n> ".to_string();
            }
            let rec = { holds.lock().await.is_held(&nm).cloned() };
            let mut s = String::new();
            s.push_str("accounthold show:\r\n");
            s.push_str(&format!(" - name: {}\r\n", nm.to_ascii_lowercase()));
            match rec {
                Some(h) => {
                    s.push_str(" - held: 1\r\n");
                    s.push_str(&format!(" - created_unix: {}\r\n", h.created_at_unix));
                    s.push_str(&format!(" - created_by: {}\r\n", h.created_by));
                    if !h.reason.trim().is_empty() {
                        s.push_str(&format!(" - reason: {}\r\n", h.reason));
                    }
                }
                None => {
                    s.push_str(" - held: 0\r\n");
                }
            }
            s.push_str("\r\n> ");
            s
        }
        "add" | "set" | "hold" => {
            let raw = it.next().unwrap_or("");
            let nm = sanitize_name(raw);
            if nm.is_empty() {
                return "accounthold add: missing/bad name\r\n\r\n> ".to_string();
            }
            let reason_raw = it.collect::<Vec<_>>().join(" ");
            let reason = clamp_chars(&reason_raw, 200);
            let name_lc = nm.to_ascii_lowercase();

            let req = sbc_core::AdminReq::UpsertLegalHold {
                name: name_lc.clone(),
                created_by: actor.to_string(),
                reason: reason.clone(),
            };
            let resp = match sbc_send_admin_req(sbc_admin_sock, &req).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(err=%e, "accounthold add: sbc admin unavailable");
                    return "accounthold add: raft unavailable\r\n\r\n> ".to_string();
                }
            };

            let (idx, entry) = match resp {
                sbc_core::AdminResp::OkLegalHold { index, entry } => (index, entry),
                sbc_core::AdminResp::Ok { index } => (
                    index,
                    sbc_core::LegalHoldEntry {
                        name_lc: name_lc.clone(),
                        created_at_unix: now_unix,
                        created_by: actor.to_string(),
                        reason: reason.clone(),
                    },
                ),
                sbc_core::AdminResp::Err { message } => {
                    warn!(message=%message, "accounthold add rejected");
                    return format!("accounthold add: rejected ({message})\r\n\r\n> ");
                }
                other => {
                    warn!(resp=?other, "accounthold add unexpected response");
                    return "accounthold add: failed\r\n\r\n> ".to_string();
                }
            };

            let changed = {
                let mut h = holds.lock().await;
                let existed = h.is_held(&name_lc).is_some();
                h.apply_upsert(idx, entry);
                !existed
            };

            {
                let mut m = sessions.lock().await;
                for si in m.values_mut() {
                    if si.name.trim().to_ascii_lowercase() == name_lc {
                        si.held = true;
                    }
                }
            }

            let ts = Utc::now().to_rfc3339();
            let sid = session_hex(session);
            let entry = format!(
                "ts={} kind=accounthold action=add session={} ip={} actor={} target={} reason={}",
                logfmt_str(&ts),
                logfmt_str(&sid),
                logfmt_str(&peer_ip.to_string()),
                logfmt_str(actor),
                logfmt_str(&name_lc),
                logfmt_str(&reason),
            );
            eventlog.log_line(LogStream::All, &entry).await;
            if !actor.trim().is_empty() {
                eventlog.log_line(LogStream::Character(actor), &entry).await;
            }

            if changed {
                format!("ok: account hold added for {name_lc}\r\n\r\n> ")
            } else {
                format!("ok: account hold already present for {name_lc}\r\n\r\n> ")
            }
        }
        "del" | "remove" | "release" | "clear" => {
            let raw = it.next().unwrap_or("");
            let nm = sanitize_name(raw);
            if nm.is_empty() {
                return "accounthold del: missing/bad name\r\n\r\n> ".to_string();
            }
            let name_lc = nm.to_ascii_lowercase();

            let req = sbc_core::AdminReq::DeleteLegalHold {
                name: name_lc.clone(),
            };
            let resp = match sbc_send_admin_req(sbc_admin_sock, &req).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(err=%e, "accounthold del: sbc admin unavailable");
                    return "accounthold del: raft unavailable\r\n\r\n> ".to_string();
                }
            };
            let idx = match resp {
                sbc_core::AdminResp::Ok { index } => index,
                sbc_core::AdminResp::Err { message } => {
                    warn!(message=%message, "accounthold del rejected");
                    return format!("accounthold del: rejected ({message})\r\n\r\n> ");
                }
                other => {
                    warn!(resp=?other, "accounthold del unexpected response");
                    return "accounthold del: failed\r\n\r\n> ".to_string();
                }
            };

            let existed = {
                let mut h = holds.lock().await;
                let existed = h.is_held(&name_lc).is_some();
                h.apply_delete(idx, &name_lc);
                existed
            };

            {
                let mut m = sessions.lock().await;
                for si in m.values_mut() {
                    if si.name.trim().to_ascii_lowercase() == name_lc {
                        si.held = false;
                    }
                }
            }

            let ts = Utc::now().to_rfc3339();
            let sid = session_hex(session);
            let entry = format!(
                "ts={} kind=accounthold action=del session={} ip={} actor={} target={}",
                logfmt_str(&ts),
                logfmt_str(&sid),
                logfmt_str(&peer_ip.to_string()),
                logfmt_str(actor),
                logfmt_str(&name_lc),
            );
            eventlog.log_line(LogStream::All, &entry).await;
            if !actor.trim().is_empty() {
                eventlog.log_line(LogStream::Character(actor), &entry).await;
            }

            if existed {
                format!("ok: account hold removed for {name_lc}\r\n\r\n> ")
            } else {
                format!("ok: account hold not present for {name_lc}\r\n\r\n> ")
            }
        }
        _ => accounthold_usage_text(),
    }
}

async fn handle_account_command(
    accounts: &Arc<tokio::sync::Mutex<Accounts>>,
    name: &str,
    line: &str,
) -> String {
    let mut it = line.split_whitespace();
    let _ = it.next(); // "account"
    let sub = it.next().unwrap_or("").to_ascii_lowercase();

    match sub.as_str() {
        "" | "help" => account_usage_text(),
        "email" => {
            let action = it.next().unwrap_or("").to_ascii_lowercase();

            match action.as_str() {
                "" | "show" => {
                    let (email, google_email) = {
                        let a = accounts.lock().await;
                        match a.by_name.get(name) {
                            Some(r) => (r.email.clone(), r.auth_email_for_method("google")),
                            None => (None, None),
                        }
                    };

                    let mut s = String::new();
                    s.push_str("account email:\r\n");
                    s.push_str(&format!(
                        " - configured: {}\r\n",
                        email.as_deref().unwrap_or("(none)")
                    ));
                    s.push_str(&format!(
                        " - google: {}\r\n",
                        google_email.as_deref().unwrap_or("(none)")
                    ));
                    s.push_str("use:\r\n");
                    s.push_str(" - account email set <addr>\r\n");
                    s.push_str(" - account email clear\r\n");
                    s.push_str("\r\n> ");
                    s
                }
                "set" => {
                    let raw = it.collect::<Vec<_>>().join(" ");
                    let Some(email) = normalize_email(&raw) else {
                        return "account email set: bad email (example: alice@example.com)\r\n\r\n> "
                            .to_string();
                    };

                    let mut a = accounts.lock().await;
                    let Some(r) = a.by_name.get_mut(name) else {
                        return "account: not found\r\n\r\n> ".to_string();
                    };

                    let changed = r.email.as_deref() != Some(email.as_str());
                    r.email = Some(email.clone());

                    if changed {
                        if let Err(e) = a.save() {
                            warn!(name = %name, err = %e, "accounts save failed");
                            return "account email: failed to save\r\n\r\n> ".to_string();
                        }
                        format!("ok: email set to {email}\r\n\r\n> ")
                    } else {
                        format!("ok: email already set to {email}\r\n\r\n> ")
                    }
                }
                "clear" | "unset" | "remove" => {
                    let mut a = accounts.lock().await;
                    let Some(r) = a.by_name.get_mut(name) else {
                        return "account: not found\r\n\r\n> ".to_string();
                    };

                    if r.email.is_none() {
                        return "ok: email already clear\r\n\r\n> ".to_string();
                    }
                    r.email = None;
                    if let Err(e) = a.save() {
                        warn!(name = %name, err = %e, "accounts save failed");
                        return "account email: failed to save\r\n\r\n> ".to_string();
                    }
                    "ok: email cleared\r\n\r\n> ".to_string()
                }
                _ => account_usage_text(),
            }
        }
        _ => account_usage_text(),
    }
}

#[derive(Debug, Clone)]
struct SessionInfo {
    name: String,
    held: bool,
    is_bot: bool,
    auth: Option<Bytes>,
    race: String,
    class: String,
    sex: String,
    pronouns: String,
    peer_ip: IpAddr,
    write_tx: ClientWriteTx,
    disconnect_tx: tokio::sync::watch::Sender<bool>,
    scrollback: Arc<tokio::sync::Mutex<Scrollback>>,
    next_cmd_id: u64,
}

#[derive(Debug)]
enum ClientWrite {
    Bytes(Bytes),
    Blob {
        prefix: Bytes,
        path: PathBuf,
        len: u64,
        suffix: Bytes,
    },
}

#[derive(Debug, Clone)]
struct ClientWriteTx(tokio::sync::mpsc::Sender<ClientWrite>);

impl ClientWriteTx {
    async fn send(&self, bytes: Bytes) -> Result<(), ()> {
        self.0.send(ClientWrite::Bytes(bytes)).await.map_err(|_| ())
    }

    async fn send_blob(
        &self,
        prefix: Bytes,
        path: Bytes,
        len: u64,
        suffix: Bytes,
    ) -> Result<(), ()> {
        let path = PathBuf::from(String::from_utf8_lossy(path.as_ref()).into_owned());
        self.0
            .send(ClientWrite::Blob {
                prefix,
                path,
                len,
                suffix,
            })
            .await
            .map_err(|_| ())
    }
}

#[derive(Debug, Clone, Serialize)]
struct ShardAuthBlob {
    acct: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    google_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oidc_sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oidc_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    caps: Option<Vec<String>>,
}

fn make_shard_auth_blob(
    acct: &str,
    method: &str,
    google_sub: Option<&str>,
    google_email: Option<&str>,
    oidc_sub: Option<&str>,
    oidc_email: Option<&str>,
    caps: Option<&[String]>,
) -> Bytes {
    let b = ShardAuthBlob {
        acct: acct.trim().to_string(),
        method: method.trim().to_ascii_lowercase(),
        google_sub: google_sub
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        google_email: google_email
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        oidc_sub: oidc_sub
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        oidc_email: oidc_email
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        caps: caps
            .map(|v| {
                v.iter()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty() && s.len() <= 64)
                    .take(32)
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty()),
    };
    // Broker is the auth boundary; if this fails, we prefer a hard error over silently dropping.
    Bytes::from(serde_json::to_vec(&b).expect("serialize shard auth blob"))
}

#[derive(Debug, Clone)]
struct ShardMsg {
    t: u8,
    session: SessionId,
    body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnState {
    NeedName,
    NeedAuthMethod,
    NeedPasswordCreate,
    NeedPasswordLogin,
    NeedGoogleWait,
    NeedBotDisclosure,
    NeedPublicAck,
    NeedCocAck,
    NeedRace,
    NeedClass,
    NeedSex,
    NeedPronouns,
    InWorld,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct AccountAuthIdentity {
    method: String, // google | oidc | future methods
    sub: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct AccountRec {
    name: String,
    #[serde(default)]
    pw_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auth_identities: Vec<AccountAuthIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    google_sub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    google_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oidc_sub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oidc_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    caps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    is_bot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    race: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pronouns: Option<String>,
    #[serde(default = "default_legacy_public_ack_version")]
    public_ack_version: u32,
    #[serde(default = "default_legacy_coc_ack_version")]
    coc_ack_version: u32,
    // User-configured email address for notifications. Not used for auth.
    #[serde(default)]
    email: Option<String>,
    created_unix: u64,
}

impl AccountRec {
    fn absorb_legacy_auth_fields(&mut self) {
        if let Some(sub) = self.google_sub.clone() {
            let email = self.google_email.clone();
            let _ = self.link_auth_identity("google", &sub, email);
        }
        if let Some(sub) = self.oidc_sub.clone() {
            let email = self.oidc_email.clone();
            let _ = self.link_auth_identity("oidc", &sub, email);
        }
        self.google_sub = None;
        self.google_email = None;
        self.oidc_sub = None;
        self.oidc_email = None;
    }

    fn has_auth_method(&self, method: &str) -> bool {
        let method = method.trim().to_ascii_lowercase();
        self.auth_identities.iter().any(|id| id.method == method)
    }

    fn has_auth_identity(&self, method: &str, sub: &str) -> bool {
        let method = method.trim().to_ascii_lowercase();
        let sub = sub.trim();
        self.auth_identities
            .iter()
            .any(|id| id.method == method && id.sub == sub)
    }

    fn auth_email_for_method(&self, method: &str) -> Option<String> {
        let method = method.trim().to_ascii_lowercase();
        self.auth_identities
            .iter()
            .find(|id| id.method == method)
            .and_then(|id| id.email.clone())
    }

    fn auth_email_for_identity(&self, method: &str, sub: &str) -> Option<String> {
        let method = method.trim().to_ascii_lowercase();
        let sub = sub.trim();
        self.auth_identities
            .iter()
            .find(|id| id.method == method && id.sub == sub)
            .and_then(|id| id.email.clone())
    }

    fn link_auth_identity(&mut self, method: &str, sub: &str, email: Option<String>) -> bool {
        let method = method.trim().to_ascii_lowercase();
        let sub = sub.trim();
        if method.is_empty() || sub.is_empty() {
            return false;
        }
        if let Some(existing) = self
            .auth_identities
            .iter_mut()
            .find(|id| id.method == method && id.sub == sub)
        {
            if email.is_some() && existing.email != email {
                existing.email = email;
                return true;
            }
            return false;
        }
        self.auth_identities.push(AccountAuthIdentity {
            method,
            sub: sub.to_string(),
            email,
        });
        true
    }
}

fn normalize_stored_race(s: &str) -> Option<String> {
    let token = s.trim().to_ascii_lowercase();
    is_allowed_token(&token, &RACE_TOKENS).then_some(token)
}

fn normalize_stored_class(s: &str) -> Option<String> {
    let token = s.trim().to_ascii_lowercase();
    is_allowed_token(&token, &CLASS_TOKENS).then_some(token)
}

fn normalize_stored_sex(s: &str) -> Option<String> {
    let token = s.trim().to_ascii_lowercase();
    matches!(token.as_str(), "male" | "female" | "none" | "other").then_some(token)
}

fn stored_account_onboarding(rec: &AccountRec) -> Option<(bool, String, String, String, String)> {
    let bot = rec.is_bot?;
    let race = normalize_stored_race(rec.race.as_deref()?)?;
    let class = normalize_stored_class(rec.class.as_deref()?)?;
    let sex = normalize_stored_sex(rec.sex.as_deref()?)?;
    let pronouns = normalize_pronouns("en", rec.pronouns.as_deref()?)?.to_string();
    Some((bot, race, class, sex, pronouns))
}

fn seed_legacy_onboarding_defaults(
    is_bot: &mut Option<bool>,
    race: &mut Option<String>,
    class: &mut Option<String>,
    sex: &mut Option<String>,
    pronouns: &mut Option<String>,
) {
    *is_bot = Some(false);
    *race = Some("human".to_string());
    *class = Some("fighter".to_string());
    *sex = Some("none".to_string());
    *pronouns = Some("they".to_string());
}

fn seed_account_onboarding(
    rec: &AccountRec,
    is_bot: &mut Option<bool>,
    race: &mut Option<String>,
    class: &mut Option<String>,
    sex: &mut Option<String>,
    pronouns: &mut Option<String>,
) -> bool {
    if let Some((bot, race_s, class_s, sex_s, pronouns_s)) = stored_account_onboarding(rec) {
        *is_bot = Some(bot);
        *race = Some(race_s);
        *class = Some(class_s);
        *sex = Some(sex_s);
        *pronouns = Some(pronouns_s);
        true
    } else {
        seed_legacy_onboarding_defaults(is_bot, race, class, sex, pronouns);
        false
    }
}

fn store_account_onboarding(
    rec: &mut AccountRec,
    is_bot: bool,
    race: &str,
    class: &str,
    sex: &str,
    pronouns: &str,
) {
    rec.is_bot = Some(is_bot);
    rec.race = Some(race.to_string());
    rec.class = Some(class.to_string());
    rec.sex = Some(sex.to_string());
    rec.pronouns = Some(pronouns.to_string());
}

fn prepare_existing_account_onboarding(
    rec: &AccountRec,
    is_bot: &mut Option<bool>,
    race: &mut Option<String>,
    class: &mut Option<String>,
    sex: &mut Option<String>,
    pronouns: &mut Option<String>,
    public_ack_version: &mut u32,
    coc_ack_version: &mut u32,
    persist_account_profile: &mut bool,
) -> ConnState {
    *public_ack_version = rec.public_ack_version;
    *coc_ack_version = rec.coc_ack_version;
    if !seed_account_onboarding(rec, is_bot, race, class, sex, pronouns) {
        *persist_account_profile = false;
    }
    if *public_ack_version < PUBLIC_ACK_VERSION {
        ConnState::NeedPublicAck
    } else if *coc_ack_version < COC_ACK_VERSION {
        ConnState::NeedCocAck
    } else {
        ConnState::NeedSex
    }
}

fn prompt_bot_disclosure() -> Bytes {
    Bytes::from_static(
        b"character creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
    )
}

fn prompt_public_ack() -> Bytes {
    Bytes::from_static(
        b"character creation (step 3/4)\r\ncontent + licensing:\r\n- anything you submit - consider it publicly licensed and publicly published\r\n- zero privacy: logs may be shared and used for training\r\n- exception: passwords are never logged/echoed; only password hashes are stored\r\ntype: agree\r\n> ",
    )
}

fn prompt_coc_ack() -> Bytes {
    let mut b = Vec::new();
    b.extend_from_slice(b"character creation (step 4/4)\r\ncode of conduct:\r\n");
    for li in COC_LINE_ITEMS {
        b.extend_from_slice(li.as_bytes());
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(b"type: agree\r\n> ");
    Bytes::from(b)
}

fn prompt_race() -> Bytes {
    Bytes::from_static(
        b"character creation (step 5/7)\r\nchoose race:\r\ntype: race list | race <name>\r\n> ",
    )
}

fn prompt_for_onboarding_state(state: ConnState) -> Option<Bytes> {
    match state {
        ConnState::NeedBotDisclosure => Some(prompt_bot_disclosure()),
        ConnState::NeedPublicAck => Some(prompt_public_ack()),
        ConnState::NeedCocAck => Some(prompt_coc_ack()),
        ConnState::NeedRace => Some(prompt_race()),
        _ => None,
    }
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
    #[serde(default)]
    return_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WebAuthReq {
    action: String, // login | create | auto | link
    method: String, // password | google | oidc
    name: String,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    #[serde(default)]
    oidc_sub: Option<String>,
    #[serde(default)]
    oidc_email: Option<String>,
    #[serde(default)]
    caps: Option<Vec<String>>,
    #[serde(default)]
    jwt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebAuthJwtClaims {
    iss: String,
    aud: String,
    iat: usize,
    nbf: usize,
    exp: usize,
    action: String,
    method: String,
    name: String,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    #[serde(default)]
    oidc_sub: Option<String>,
    #[serde(default)]
    oidc_email: Option<String>,
    #[serde(default)]
    caps: Option<Vec<String>>,
}

fn verify_webauth_jwt(cfg: &Config, req: &WebAuthReq) -> Result<(), &'static str> {
    let Some(secret) = cfg.webauth_jwt_secret.as_deref() else {
        return Ok(());
    };
    let tok = req.jwt.as_deref().map(str::trim).unwrap_or("");
    if tok.is_empty() {
        return Err("missing jwt");
    }

    let mut v = Validation::new(Algorithm::HS256);
    v.set_audience(&["slopmud_broker"]);
    v.set_issuer(&["slopmud_web"]);
    v.leeway = 5;

    let claims = match jsonwebtoken::decode::<WebAuthJwtClaims>(
        tok,
        &DecodingKey::from_secret(secret.as_bytes()),
        &v,
    ) {
        Ok(c) => c.claims,
        Err(_) => return Err("bad jwt"),
    };

    if claims.action.trim().to_ascii_lowercase() != req.action.trim().to_ascii_lowercase() {
        return Err("jwt action mismatch");
    }
    if claims.method.trim().to_ascii_lowercase() != req.method.trim().to_ascii_lowercase() {
        return Err("jwt method mismatch");
    }
    if claims.name.trim() != req.name.trim() {
        return Err("jwt name mismatch");
    }
    if claims.google_sub != req.google_sub {
        return Err("jwt google_sub mismatch");
    }
    if claims.google_email != req.google_email {
        return Err("jwt google_email mismatch");
    }
    if claims.oidc_sub != req.oidc_sub {
        return Err("jwt oidc_sub mismatch");
    }
    if claims.oidc_email != req.oidc_email {
        return Err("jwt oidc_email mismatch");
    }
    if claims.caps != req.caps {
        return Err("jwt caps mismatch");
    }
    Ok(())
}

#[derive(Debug)]
struct Accounts {
    path: String,
    by_name: HashMap<String, AccountRec>,
}

impl Accounts {
    fn load(path: String) -> Self {
        let mut by_name = HashMap::new();
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<Vec<AccountRec>>(&s) {
                for mut a in v {
                    a.absorb_legacy_auth_fields();
                    by_name.insert(a.name.clone(), a);
                }
            }
        }
        Self { path, by_name }
    }

    fn save(&self) -> anyhow::Result<()> {
        let mut v = self.by_name.values().cloned().collect::<Vec<_>>();
        for rec in v.iter_mut() {
            rec.absorb_legacy_auth_fields();
        }
        v.sort_by(|a, b| a.name.cmp(&b.name));
        let s = serde_json::to_string_pretty(&v)?;
        let tmp = format!("{}.tmp", self.path);
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn linked_names_for_identity(&self, method: &str, sub: &str) -> Vec<String> {
        self.by_name
            .values()
            .filter_map(|r| {
                if r.has_auth_identity(method, sub) {
                    Some(r.name.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    fn identity_linked_to_other_account(&self, method: &str, sub: &str, name: &str) -> bool {
        let name = name.trim().to_ascii_lowercase();
        self.by_name
            .values()
            .any(|r| r.name.trim().to_ascii_lowercase() != name && r.has_auth_identity(method, sub))
    }
}

fn new_session_id() -> SessionId {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("getrandom");
    SessionId::from_be_bytes(b)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,slopmud=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    let cfg = Arc::new(parse_args());
    let listener = TcpListener::bind(cfg.bind).await?;

    let server_info = Arc::new(ServerInfo {
        started_instant: std::time::Instant::now(),
        started_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        shard_addr: cfg.shard_addr.clone(),
        shard_addrs: cfg.shard_addrs.clone(),
        bind: cfg.bind,
    });

    let sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let accounts: Arc<tokio::sync::Mutex<Accounts>> = Arc::new(tokio::sync::Mutex::new(
        Accounts::load(cfg.accounts_path.clone()),
    ));
    let login_throttle: Arc<tokio::sync::Mutex<LoginThrottle>> =
        Arc::new(tokio::sync::Mutex::new(LoginThrottle::default()));
    let bans: Arc<tokio::sync::Mutex<ban::BanState>> = Arc::new(tokio::sync::Mutex::new(
        ban::BanState::load(cfg.bans_path.clone()),
    ));
    let holds: Arc<tokio::sync::Mutex<hold::HoldCache>> =
        Arc::new(tokio::sync::Mutex::new(hold::HoldCache::new()));
    let eventlog = Arc::new(eventlog::EventLog::new(cfg.eventlog.clone()).await);
    let line_ids: Arc<tokio::sync::Mutex<LineIdGen>> = Arc::new(tokio::sync::Mutex::new(
        LineIdGen::new(cfg.node_id.as_deref()),
    ));
    let nearline = Arc::new(nearline::NearlineRing::new(cfg.nearline.clone()).await);

    let (shard_tx, shard_rx) = tokio::sync::mpsc::channel::<ShardMsg>(4096);
    tokio::spawn(shard_manager_task(
        cfg.shard_addrs.clone(),
        sessions.clone(),
        line_ids.clone(),
        nearline.clone(),
        eventlog.clone(),
        shard_rx,
    ));

    tokio::spawn(admin_server_task(
        cfg.admin_bind,
        bans.clone(),
        sessions.clone(),
        accounts.clone(),
    ));

    if cfg.sbc_enabled {
        tokio::spawn(sbc_holds_events_task(
            cfg.sbc_events_sock.clone(),
            holds.clone(),
            sessions.clone(),
        ));
    } else {
        info!("sbc event subscriber disabled");
    }

    info!(
        bind = %cfg.bind,
        shard_addr = %cfg.shard_addr,
        shard_addrs = ?cfg.shard_addrs,
        node_id = %cfg.node_id.as_deref().unwrap_or("-"),
        admin_bind = %cfg.admin_bind,
        bans_path = %cfg.bans_path.display(),
        sbc_admin_sock = %cfg.sbc_admin_sock.display(),
        sbc_events_sock = %cfg.sbc_events_sock.display(),
        sbc_enabled = cfg.sbc_enabled,
        "session broker listening"
    );

    loop {
        let (stream, peer) = listener.accept().await?;
        let sessions = sessions.clone();
        let shard_tx = shard_tx.clone();
        let server_info = server_info.clone();
        let cfg = cfg.clone();
        let accounts = accounts.clone();
        let login_throttle = login_throttle.clone();
        let bans = bans.clone();
        let holds = holds.clone();
        let nearline = nearline.clone();
        let eventlog = eventlog.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(
                stream,
                peer,
                sessions,
                shard_tx,
                server_info,
                cfg,
                accounts,
                login_throttle,
                bans,
                holds,
                nearline,
                eventlog,
            )
            .await
            {
                warn!(peer = %peer, err = %e, "connection ended with error");
            }
        });
    }
}

async fn shard_manager_task(
    shard_addrs: Vec<String>,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    line_ids: Arc<tokio::sync::Mutex<LineIdGen>>,
    nearline: Arc<nearline::NearlineRing>,
    eventlog: Arc<eventlog::EventLog>,
    mut rx: tokio::sync::mpsc::Receiver<ShardMsg>,
) {
    if shard_addrs.is_empty() {
        warn!("no shard addresses configured");
        return;
    }

    let mut addr_i = 0usize;
    let mut pending: VecDeque<ShardMsg> = VecDeque::new();
    let mut inflight: VecDeque<ShardMsg> = VecDeque::new();

    loop {
        let shard_addr = shard_addrs[addr_i % shard_addrs.len()].clone();
        match TcpStream::connect(shard_addr.as_str()).await {
            Ok(stream) => {
                info!(shard_addr = %shard_addr, "connected to shard");

                let (rd, wr) = stream.into_split();
                let mut fr = FrameReader::new(rd);
                let mut fw = FrameWriter::new(wr);

                // Re-attach all live sessions.
                let snapshot = {
                    let m = sessions.lock().await;
                    m.iter()
                        .map(|(sid, s)| {
                            (
                                *sid,
                                s.is_bot,
                                s.auth.clone(),
                                s.race.clone(),
                                s.class.clone(),
                                s.sex.clone(),
                                s.pronouns.clone(),
                                s.name.clone(),
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let mut reattach_failed = false;
                for (sid, is_bot, auth, race, class, sex, pronouns, name) in snapshot {
                    let body = attach_body(
                        is_bot,
                        true,
                        auth.as_deref(),
                        &race,
                        &class,
                        &sex,
                        &pronouns,
                        name.as_bytes(),
                    );
                    if let Err(err) = write_req(&mut fw, REQ_ATTACH, sid, &body).await {
                        warn!(shard_addr = %shard_addr, err=%err, "reattach to shard failed");
                        reattach_failed = true;
                        break;
                    }
                }
                if !reattach_failed {
                    if let Err(err) = fw.flush().await {
                        warn!(shard_addr = %shard_addr, err=%err, "reattach flush to shard failed");
                        reattach_failed = true;
                    }
                }
                if reattach_failed {
                    addr_i = addr_i.wrapping_add(1);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }

                // Connection loop.
                loop {
                    if let Some(msg) = pending.pop_front() {
                        if let Err(err) = write_req(&mut fw, msg.t, msg.session, &msg.body).await {
                            warn!(shard_addr = %shard_addr, err=%err, "pending shard request write failed");
                            pending.push_front(msg);
                            break;
                        }
                        if shard_msg_expects_response(&msg) {
                            inflight.push_back(msg);
                        }
                        continue;
                    }

                    tokio::select! {
                        msg = rx.recv() => {
                            let Some(msg) = msg else {
                                return;
                            };
                            if let Err(err) = write_req(&mut fw, msg.t, msg.session, &msg.body).await {
                                warn!(shard_addr = %shard_addr, err=%err, "shard request write failed");
                                pending.push_front(msg);
                                break;
                            }
                            if shard_msg_expects_response(&msg) {
                                inflight.push_back(msg);
                            }
                        }
                        res = fr.read_frame() => {
                            let frame = match res {
                                Ok(Some(f)) => f,
                                Ok(None) => break,
                                Err(_) => break,
                            };
                            match mudproto::shard::parse_resp(frame) {
                                Ok(resp) => {
                                    ack_inflight_for_response(&mut inflight, &resp);
                                    route_resp(resp, &sessions, &line_ids, &nearline, &eventlog)
                                        .await
                                }
                                Err(e) => {
                                    warn!(err=%e, "bad shard response");
                                }
                            }
                        }
                    }
                }

                // Shard connection dropped.
                requeue_inflight(&mut pending, &mut inflight);
                warn!(shard_addr = %shard_addr, "shard disconnected; failing over");
                addr_i = addr_i.wrapping_add(1);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                warn!(shard_addr = %shard_addr, err=%e, "shard offline; trying next shard");
                addr_i = addr_i.wrapping_add(1);
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

fn shard_msg_expects_response(msg: &ShardMsg) -> bool {
    msg.t != REQ_DETACH
}

fn ack_inflight_for_response(inflight: &mut VecDeque<ShardMsg>, resp: &ShardResp) {
    let session = match resp {
        ShardResp::Output { session, .. }
        | ShardResp::Err { session, .. }
        | ShardResp::OutputBlob { session, .. } => *session,
    };
    if let Some(i) = inflight.iter().position(|msg| msg.session == session) {
        inflight.remove(i);
    }
}

fn requeue_inflight(pending: &mut VecDeque<ShardMsg>, inflight: &mut VecDeque<ShardMsg>) {
    while let Some(msg) = inflight.pop_back() {
        pending.push_front(msg);
    }
}

async fn route_resp(
    resp: ShardResp,
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    line_ids: &Arc<tokio::sync::Mutex<LineIdGen>>,
    nearline: &Arc<nearline::NearlineRing>,
    eventlog: &Arc<eventlog::EventLog>,
) {
    match resp {
        ShardResp::Output { session, line } => {
            let si = { sessions.lock().await.get(&session).cloned() };
            if let Some(si) = si {
                let now = Utc::now();
                let now_ms = u64::try_from(now.timestamp_millis()).unwrap_or(0);
                let sid_hex = session_hex(session);
                let name = si.name.clone();
                let ip = si.peer_ip.to_string();
                let held = si.held;
                let texts = extract_output_lines(line.as_ref());
                let mut log_lines: Vec<(LineId, String)> = Vec::new();
                if !texts.is_empty() {
                    let mut id_gen = line_ids.lock().await;
                    let mut sb = si.scrollback.lock().await;
                    for text in texts {
                        let id = id_gen.next_id(now_ms);
                        let sb_text = clamp_chars(&text, SCROLLBACK_MAX_LINE_CHARS);
                        sb.push_line(id, now_ms, sb_text.clone());

                        let disk_text = if held {
                            redact_pii(&sb_text)
                        } else {
                            sb_text.clone()
                        };
                        nearline.try_append(nearline::NearlineRecord {
                            v: 1,
                            id: id.to_string(),
                            ts_unix_ms: now_ms,
                            kind: "output".to_string(),
                            session: sid_hex.clone(),
                            name: name.clone(),
                            ip: ip.clone(),
                            text: disk_text,
                        });

                        let log_text = if held { redact_pii(&text) } else { text };
                        log_lines.push((id, log_text));
                    }
                }

                let ts = now.to_rfc3339();
                // Log before writing to the client so disconnects still show the final output.
                for (line_id, text) in log_lines {
                    let entry = format!(
                        "ts={} kind=output session={} ip={} name={} line_id={} text={}",
                        logfmt_str(&ts),
                        logfmt_str(&sid_hex),
                        logfmt_str(&ip),
                        logfmt_str(&name),
                        logfmt_str(&line_id.to_string()),
                        logfmt_str(&text)
                    );
                    eventlog.log_line(LogStream::All, &entry).await;
                    eventlog.log_line(LogStream::Character(&name), &entry).await;
                }

                let _ = si.write_tx.send(line).await;
            }
        }
        ShardResp::Err { session, msg } => {
            let si = { sessions.lock().await.get(&session).cloned() };
            if let Some(si) = si {
                let now = Utc::now();
                let now_ms = u64::try_from(now.timestamp_millis()).unwrap_or(0);
                let sid_hex = session_hex(session);
                let name = si.name.clone();
                let ip = si.peer_ip.to_string();
                let held = si.held;
                let texts = extract_output_lines(msg.as_ref());
                let mut log_lines: Vec<(LineId, String)> = Vec::new();
                if !texts.is_empty() {
                    let mut id_gen = line_ids.lock().await;
                    let mut sb = si.scrollback.lock().await;
                    for text in texts {
                        let id = id_gen.next_id(now_ms);
                        let sb_text = clamp_chars(&text, SCROLLBACK_MAX_LINE_CHARS);
                        sb.push_line(id, now_ms, sb_text.clone());

                        let disk_text = if held {
                            redact_pii(&sb_text)
                        } else {
                            sb_text.clone()
                        };
                        nearline.try_append(nearline::NearlineRecord {
                            v: 1,
                            id: id.to_string(),
                            ts_unix_ms: now_ms,
                            kind: "err".to_string(),
                            session: sid_hex.clone(),
                            name: name.clone(),
                            ip: ip.clone(),
                            text: disk_text,
                        });

                        let log_text = if held { redact_pii(&text) } else { text };
                        log_lines.push((id, log_text));
                    }
                }

                let ts = now.to_rfc3339();
                for (line_id, text) in log_lines {
                    let entry = format!(
                        "ts={} kind=err session={} ip={} name={} line_id={} text={}",
                        logfmt_str(&ts),
                        logfmt_str(&sid_hex),
                        logfmt_str(&ip),
                        logfmt_str(&name),
                        logfmt_str(&line_id.to_string()),
                        logfmt_str(&text)
                    );
                    eventlog.log_line(LogStream::All, &entry).await;
                    eventlog.log_line(LogStream::Character(&name), &entry).await;
                }

                let _ = si.write_tx.send(msg).await;
            }
        }
        ShardResp::OutputBlob {
            session,
            prefix,
            path,
            len,
            suffix,
        } => {
            let si = { sessions.lock().await.get(&session).cloned() };
            if let Some(si) = si {
                let now = Utc::now();
                let now_ms = u64::try_from(now.timestamp_millis()).unwrap_or(0);
                let sid_hex = session_hex(session);
                let name = si.name.clone();
                let ip = si.peer_ip.to_string();
                let path_text = String::from_utf8_lossy(path.as_ref()).to_string();
                let text = format!("# blob output: {len} bytes from {path_text}");

                {
                    let mut id_gen = line_ids.lock().await;
                    let mut sb = si.scrollback.lock().await;
                    let id = id_gen.next_id(now_ms);
                    sb.push_line(id, now_ms, clamp_chars(&text, SCROLLBACK_MAX_LINE_CHARS));

                    nearline.try_append(nearline::NearlineRecord {
                        v: 1,
                        id: id.to_string(),
                        ts_unix_ms: now_ms,
                        kind: "output_blob".to_string(),
                        session: sid_hex.clone(),
                        name: name.clone(),
                        ip: ip.clone(),
                        text: if si.held {
                            redact_pii(&text)
                        } else {
                            text.clone()
                        },
                    });

                    let ts = now.to_rfc3339();
                    let entry = format!(
                        "ts={} kind=output_blob session={} ip={} name={} line_id={} bytes={} path={}",
                        logfmt_str(&ts),
                        logfmt_str(&sid_hex),
                        logfmt_str(&ip),
                        logfmt_str(&name),
                        logfmt_str(&id.to_string()),
                        logfmt_str(&len.to_string()),
                        logfmt_str(if si.held { "[redacted]" } else { &path_text }),
                    );
                    eventlog.log_line(LogStream::All, &entry).await;
                    eventlog.log_line(LogStream::Character(&name), &entry).await;
                }

                let _ = si.write_tx.send_blob(prefix, path, len, suffix).await;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdminReq {
    BanCharacter {
        name: String,
        created_by: String,
        reason: String,
    },
    BanIpPrefix {
        cidr: String,
        created_by: String,
        reason: String,
    },
    ListBans {},
    ListSessions {},
    CreateAccountPassword {
        name: String,
        password: String,
        #[serde(default)]
        caps: Option<Vec<String>>,
    },
    SetAccountPassword {
        name: String,
        password: String,
    },
    GrantAccountCaps {
        name: String,
        caps: Vec<String>,
    },
    GetAccount {
        name: String,
    },
    ListAccounts {},
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AdminResp {
    Ok {
        kicked: u64,
    },
    OkBans {
        bans: ban::BanListFile,
    },
    OkSessions {
        humans: Vec<String>,
        bots: Vec<String>,
    },
    OkAccount {
        name: String,
        has_password: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        caps: Vec<String>,
    },
    OkAccounts {
        names: Vec<String>,
    },
    Err {
        message: String,
    },
}

fn normalize_caps_list(caps: &[String]) -> Vec<String> {
    // Keep this compatible with shard-side capability parsing:
    // - lowercase
    // - ASCII only, no whitespace/control
    // - conservative charset: [a-z0-9._-]
    // - cap count and length limits to avoid unbounded growth
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in caps.iter() {
        if out.len() >= 32 {
            break;
        }
        let t = raw.trim().to_ascii_lowercase();
        if t.is_empty() || t.len() > 64 {
            continue;
        }
        if !t.is_ascii()
            || t.chars()
                .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
        {
            continue;
        }
        if !t.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
        }) {
            continue;
        }
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out.sort_unstable();
    out
}

async fn admin_server_task(
    bind: SocketAddr,
    bans: Arc<tokio::sync::Mutex<ban::BanState>>,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    accounts: Arc<tokio::sync::Mutex<Accounts>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    info!(bind=%bind, "admin server listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let bans = bans.clone();
        let sessions = sessions.clone();
        let accounts = accounts.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_admin_conn(stream, bans, sessions, accounts).await {
                warn!(peer=%peer, err=%e, "admin request failed");
            }
        });
    }
}

async fn handle_admin_conn(
    stream: TcpStream,
    bans: Arc<tokio::sync::Mutex<ban::BanState>>,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    accounts: Arc<tokio::sync::Mutex<Accounts>>,
) -> anyhow::Result<()> {
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);

    let mut line = String::new();
    rd.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let req: AdminReq = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            let resp = AdminResp::Err {
                message: format!("bad json: {e}"),
            };
            wr.write_all(serde_json::to_string(&resp)?.as_bytes())
                .await?;
            wr.write_all(b"\n").await?;
            return Ok(());
        }
    };

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let resp = match req {
        AdminReq::BanCharacter {
            name,
            created_by,
            reason,
        } => {
            let nm = sanitize_name(&name);
            if nm.is_empty() {
                AdminResp::Err {
                    message: "bad name".to_string(),
                }
            } else {
                let _changed = {
                    let mut b = bans.lock().await;
                    b.upsert_char_ban(&nm, now_unix, created_by, reason)?
                };
                let kicked = kick_by_char(&sessions, &nm).await;
                AdminResp::Ok { kicked }
            }
        }
        AdminReq::BanIpPrefix {
            cidr,
            created_by,
            reason,
        } => {
            let pfx = {
                let mut b = bans.lock().await;
                let (_changed, pfx) = b.upsert_ip_ban(&cidr, now_unix, created_by, reason)?;
                pfx
            };
            let kicked = kick_by_ip(&sessions, &pfx).await;
            AdminResp::Ok { kicked }
        }
        AdminReq::ListBans {} => {
            let b = bans.lock().await;
            AdminResp::OkBans {
                bans: b.snapshot_file(),
            }
        }
        AdminReq::ListSessions {} => {
            let snapshot = {
                let m = sessions.lock().await;
                m.values()
                    .map(|s| (s.name.clone(), s.is_bot))
                    .collect::<Vec<_>>()
            };

            let mut humans = Vec::new();
            let mut bots = Vec::new();
            for (name, is_bot) in snapshot {
                if is_bot {
                    bots.push(name);
                } else {
                    humans.push(name);
                }
            }

            humans.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
            humans.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
            bots.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
            bots.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

            AdminResp::OkSessions { humans, bots }
        }
        AdminReq::CreateAccountPassword {
            name,
            password,
            caps,
        } => {
            let uname = sanitize_name(&name);
            if uname.is_empty() {
                AdminResp::Err {
                    message: "bad name".to_string(),
                }
            } else if password.as_bytes().len() < 8 {
                AdminResp::Err {
                    message: "password too short (min 8)".to_string(),
                }
            } else {
                let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
                let hash = Argon2::default()
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(|e| anyhow::anyhow!("hash_password failed: {e}"))?
                    .to_string();
                let caps = caps
                    .map(|v| normalize_caps_list(&v))
                    .filter(|v| !v.is_empty());
                let caps_vec = caps.clone().unwrap_or_default();

                let created = {
                    let mut a = accounts.lock().await;
                    if a.by_name.contains_key(&uname) {
                        false
                    } else {
                        a.by_name.insert(
                            uname.clone(),
                            AccountRec {
                                name: uname.clone(),
                                pw_hash: Some(hash),
                                auth_identities: Vec::new(),
                                google_sub: None,
                                google_email: None,
                                oidc_sub: None,
                                oidc_email: None,
                                caps,
                                is_bot: None,
                                race: None,
                                class: None,
                                sex: None,
                                pronouns: None,
                                public_ack_version: 0,
                                coc_ack_version: 0,
                                email: None,
                                created_unix: now_unix,
                            },
                        );
                        a.save()?;
                        true
                    }
                };

                if !created {
                    AdminResp::Err {
                        message: "account already exists".to_string(),
                    }
                } else {
                    AdminResp::OkAccount {
                        name: uname,
                        has_password: true,
                        caps: caps_vec,
                    }
                }
            }
        }
        AdminReq::SetAccountPassword { name, password } => {
            let uname = sanitize_name(&name);
            if uname.is_empty() {
                AdminResp::Err {
                    message: "bad name".to_string(),
                }
            } else if password.as_bytes().len() < 8 {
                AdminResp::Err {
                    message: "password too short (min 8)".to_string(),
                }
            } else {
                let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
                let hash = Argon2::default()
                    .hash_password(password.as_bytes(), &salt)
                    .map_err(|e| anyhow::anyhow!("hash_password failed: {e}"))?
                    .to_string();

                let out = {
                    let mut a = accounts.lock().await;
                    if let Some(r) = a.by_name.get_mut(&uname) {
                        r.pw_hash = Some(hash);
                        let has_password =
                            r.pw_hash.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
                        let caps = r.caps.clone().unwrap_or_default();
                        a.save()?;
                        AdminResp::OkAccount {
                            name: uname,
                            has_password,
                            caps,
                        }
                    } else {
                        AdminResp::Err {
                            message: "account not found".to_string(),
                        }
                    }
                };

                out
            }
        }
        AdminReq::GrantAccountCaps { name, caps } => {
            let uname = sanitize_name(&name);
            if uname.is_empty() {
                AdminResp::Err {
                    message: "bad name".to_string(),
                }
            } else {
                let add = normalize_caps_list(&caps);
                if add.is_empty() {
                    AdminResp::Err {
                        message: "no valid caps".to_string(),
                    }
                } else {
                    let out = {
                        let mut a = accounts.lock().await;
                        if let Some(r) = a.by_name.get_mut(&uname) {
                            let mut merged = r.caps.clone().unwrap_or_default();
                            merged.extend(add);
                            merged = normalize_caps_list(&merged);
                            r.caps = if merged.is_empty() {
                                None
                            } else {
                                Some(merged.clone())
                            };
                            let has_password =
                                r.pw_hash.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
                            a.save()?;
                            AdminResp::OkAccount {
                                name: uname,
                                has_password,
                                caps: merged,
                            }
                        } else {
                            AdminResp::Err {
                                message: "account not found".to_string(),
                            }
                        }
                    };
                    out
                }
            }
        }
        AdminReq::GetAccount { name } => {
            let uname = sanitize_name(&name);
            if uname.is_empty() {
                AdminResp::Err {
                    message: "bad name".to_string(),
                }
            } else {
                let rec = {
                    let a = accounts.lock().await;
                    a.by_name.get(&uname).cloned()
                };
                match rec {
                    None => AdminResp::Err {
                        message: "account not found".to_string(),
                    },
                    Some(r) => AdminResp::OkAccount {
                        name: r.name,
                        has_password: r.pw_hash.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
                        caps: r.caps.unwrap_or_default(),
                    },
                }
            }
        }
        AdminReq::ListAccounts {} => {
            let mut names = {
                let a = accounts.lock().await;
                a.by_name.keys().cloned().collect::<Vec<_>>()
            };
            names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
            AdminResp::OkAccounts { names }
        }
    };

    wr.write_all(serde_json::to_string(&resp)?.as_bytes())
        .await?;
    wr.write_all(b"\n").await?;
    Ok(())
}

async fn kick_by_char(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    name: &str,
) -> u64 {
    let name_lc = name.trim().to_ascii_lowercase();
    if name_lc.is_empty() {
        return 0;
    }

    let targets = {
        let m = sessions.lock().await;
        m.iter()
            .filter(|(_, s)| s.name.trim().to_ascii_lowercase() == name_lc)
            .map(|(_, s)| (s.write_tx.clone(), s.disconnect_tx.clone()))
            .collect::<Vec<_>>()
    };

    for (tx, disc) in &targets {
        let _ = tx
            .send(Bytes::from_static(b"\r\n# banned (character)\r\nbye\r\n"))
            .await;
        let _ = disc.send(true);
    }

    targets.len() as u64
}

async fn kick_by_ip(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    pfx: &sbc_core::IpPrefix,
) -> u64 {
    let targets = {
        let m = sessions.lock().await;
        m.iter()
            .filter(|(_, s)| pfx.contains_ip(s.peer_ip))
            .map(|(_, s)| (s.write_tx.clone(), s.disconnect_tx.clone()))
            .collect::<Vec<_>>()
    };

    for (tx, disc) in &targets {
        let _ = tx
            .send(Bytes::from_static(b"\r\n# banned (ip)\r\nbye\r\n"))
            .await;
        let _ = disc.send(true);
    }

    targets.len() as u64
}

async fn client_writer_loop(
    mut wr: tokio::net::tcp::OwnedWriteHalf,
    mut rx: tokio::sync::mpsc::Receiver<ClientWrite>,
) {
    while let Some(item) = rx.recv().await {
        let res = match item {
            ClientWrite::Bytes(bytes) => write_all_bytes_vectored(&mut wr, &[bytes]).await,
            ClientWrite::Blob {
                prefix,
                path,
                len,
                suffix,
            } => write_blob_to_client(&mut wr, prefix, &path, len, suffix).await,
        };
        if res.is_err() {
            break;
        }
    }
}

async fn write_blob_to_client(
    wr: &mut tokio::net::tcp::OwnedWriteHalf,
    prefix: Bytes,
    path: &Path,
    len: u64,
    suffix: Bytes,
) -> std::io::Result<()> {
    let mut parts = Vec::with_capacity(2);
    if !prefix.is_empty() {
        parts.push(prefix);
    }
    if !parts.is_empty() {
        write_all_bytes_vectored(wr, &parts).await?;
    }

    let sent = kzc::send_file_to_writer(wr, path, len).await?;
    if sent < len {
        warn!(
            path = %path.display(),
            expected = len,
            sent,
            "blob output ended before advertised byte length"
        );
    }

    if !suffix.is_empty() {
        write_all_bytes_vectored(wr, &[suffix]).await?;
    }
    Ok(())
}

fn parse_sayblob_len(line: &str) -> Option<Result<u64, &'static str>> {
    let mut words = line.split_ascii_whitespace();
    match words.next()? {
        "sayblob" | "say-blob" => {}
        _ => return None,
    }
    let Some(raw_len) = words.next() else {
        return Some(Err("usage: sayblob <byte-count>"));
    };
    if words.next().is_some() {
        return Some(Err("usage: sayblob <byte-count>"));
    }
    let Ok(len) = raw_len.parse::<u64>() else {
        return Some(Err("sayblob: bad byte count"));
    };
    if len == 0 || len > MAX_DECLARED_BLOB_BYTES {
        return Some(Err("sayblob: byte count out of range"));
    }
    Some(Ok(len))
}

async fn spool_blob_payload<R>(
    rd: &mut R,
    iac: &mut IacParser,
    linebuf: &mut Vec<u8>,
    spool_dir: &Path,
    session: SessionId,
    len: u64,
) -> std::io::Result<PathBuf>
where
    R: AsyncRead + Unpin,
{
    tokio::fs::create_dir_all(spool_dir).await?;
    let path = spool_dir.join(format!(
        "{}-{}.blob",
        session_hex(session),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut file = tokio::fs::File::create(&path).await?;
    let mut remaining = len;

    if !linebuf.is_empty() {
        let take = remaining.min(linebuf.len() as u64) as usize;
        file.write_all(&linebuf[..take]).await?;
        linebuf.drain(0..take);
        remaining = remaining.saturating_sub(take as u64);
    }

    let mut buf = vec![0u8; BLOB_STREAM_CHUNK_BYTES];
    while remaining > 0 {
        let read_cap = remaining.min(buf.len() as u64) as usize;
        let n = rd.read(&mut buf[..read_cap]).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "client disconnected during blob payload",
            ));
        }
        let (data, _replies) = iac.parse_cow(&buf[..n]);
        if data.is_empty() {
            continue;
        }
        let bytes = data.as_ref();
        let take = remaining.min(bytes.len() as u64) as usize;
        file.write_all(&bytes[..take]).await?;
        remaining = remaining.saturating_sub(take as u64);
        if take < bytes.len() {
            linebuf.extend_from_slice(&bytes[take..]);
        }
    }
    file.flush().await?;
    Ok(path)
}

async fn write_req(
    fw: &mut FrameWriter<tokio::net::tcp::OwnedWriteHalf>,
    t: u8,
    session: SessionId,
    body: &[u8],
) -> std::io::Result<()> {
    let mut hdr = [0u8; 1 + SessionId::LEN];
    hdr[0] = t;
    hdr[1..].copy_from_slice(&session.to_be_bytes());
    fw.write_frame_parts(&[&hdr, body]).await
}

fn attach_body(
    is_bot: bool,
    quiet: bool,
    auth: Option<&[u8]>,
    race: &str,
    class: &str,
    sex: &str,
    pronouns: &str,
    name: &[u8],
) -> Bytes {
    let mut b = Vec::with_capacity(
        1 + 2
            + auth.map(|a| a.len()).unwrap_or(0)
            + 1
            + race.len()
            + 1
            + class.len()
            + 1
            + sex.len()
            + 1
            + pronouns.len()
            + name.len(),
    );
    let mut flags = 0u8;
    if is_bot {
        flags |= 0x01;
    }
    if quiet {
        flags |= 0x08;
    }
    if auth.is_some() {
        flags |= 0x02;
    }
    flags |= 0x04; // build info always included by broker
    b.push(flags);
    if let Some(a) = auth {
        if a.len() <= u16::MAX as usize {
            let len = a.len() as u16;
            b.extend_from_slice(&len.to_be_bytes());
            b.extend_from_slice(a);
        } else {
            // Too long to encode; drop auth rather than truncating.
            warn!(len = a.len(), "auth token too long; dropping");
            b[0] &= !0x02;
        }
    }
    // race/class tokens (u8 length + bytes)
    let r = race.as_bytes();
    let c = class.as_bytes();
    let sx = sex.as_bytes();
    let pr = pronouns.as_bytes();
    b.push(r.len().min(255) as u8);
    b.extend_from_slice(&r[..r.len().min(255)]);
    b.push(c.len().min(255) as u8);
    b.extend_from_slice(&c[..c.len().min(255)]);
    b.push(sx.len().min(255) as u8);
    b.extend_from_slice(&sx[..sx.len().min(255)]);
    b.push(pr.len().min(255) as u8);
    b.extend_from_slice(&pr[..pr.len().min(255)]);
    b.extend_from_slice(name);
    Bytes::from(b)
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    access_token: String,
    token_type: Option<String>,
    expires_in: Option<u64>,
}

fn hex_lower(b: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(LUT[(x >> 4) as usize] as char);
        s.push(LUT[(x & 0x0f) as usize] as char);
    }
    s
}

fn session_hex(session: SessionId) -> String {
    hex_lower(&session.to_be_bytes())
}

fn escape_log_text(b: &[u8]) -> String {
    // Convert to UTF-8 (lossy) and strip any terminal NULs.
    let s = String::from_utf8_lossy(b).to_string();
    s.trim_end_matches('\0').to_string()
}

fn logfmt_str(s: &str) -> String {
    // Always quote; logfmt readers accept this and it avoids edge cases.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn redact_input_for_logs(line: &str) -> Cow<'_, str> {
    // Avoid leaking email addresses into logs by redacting the one command that carries it.
    let mut it = line.split_whitespace();
    let Some(cmd) = it.next() else {
        return Cow::Borrowed(line);
    };
    if !cmd.eq_ignore_ascii_case("account") {
        return Cow::Borrowed(line);
    }
    let Some(sub) = it.next() else {
        return Cow::Borrowed(line);
    };
    if !sub.eq_ignore_ascii_case("email") {
        return Cow::Borrowed(line);
    }
    let Some(action) = it.next() else {
        return Cow::Borrowed(line);
    };
    if action.eq_ignore_ascii_case("set") {
        return Cow::Borrowed("account email set <redacted>");
    }
    Cow::Borrowed(line)
}

#[allow(dead_code)]
async fn mint_internal_oidc_token(
    cfg: &Config,
    session: SessionId,
    sub: &str,
) -> anyhow::Result<Option<Bytes>> {
    let Some(url) = cfg.oidc_token_url.as_deref() else {
        return Ok(None);
    };
    let client_id = cfg.oidc_client_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!("SLOPMUD_OIDC_TOKEN_URL set but missing SLOPMUD_OIDC_CLIENT_ID")
    })?;
    let client_secret = cfg.oidc_client_secret.as_deref().ok_or_else(|| {
        anyhow::anyhow!("SLOPMUD_OIDC_TOKEN_URL set but missing SLOPMUD_OIDC_CLIENT_SECRET")
    })?;
    let scope = cfg.oidc_scope.as_deref().unwrap_or("slopmud:session");

    let sid_hex = hex_lower(&session.to_be_bytes());
    let http = reqwest::Client::new();
    let resp = http
        .post(url)
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "client_credentials"),
            ("sub", sub),
            ("sid", sid_hex.as_str()),
            ("scope", scope),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "oidc token endpoint returned {}",
            resp.status()
        ));
    }
    let t: OidcTokenResponse = resp.json().await?;
    Ok(Some(Bytes::from(t.access_token)))
}

async fn handle_conn(
    stream: TcpStream,
    peer: SocketAddr,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    shard_tx: tokio::sync::mpsc::Sender<ShardMsg>,
    server_info: Arc<ServerInfo>,
    cfg: Arc<Config>,
    accounts: Arc<tokio::sync::Mutex<Accounts>>,
    login_throttle: Arc<tokio::sync::Mutex<LoginThrottle>>,
    bans: Arc<tokio::sync::Mutex<ban::BanState>>,
    holds: Arc<tokio::sync::Mutex<hold::HoldCache>>,
    nearline: Arc<nearline::NearlineRing>,
    eventlog: Arc<eventlog::EventLog>,
) -> anyhow::Result<()> {
    let session = new_session_id();
    let mut peer_ip = peer.ip();
    let mut peer_port = peer.port();
    let trusted_proxy_peer = peer_ip.is_loopback();
    let (mut rd, wr) = stream.into_split();

    let (disconnect_tx, mut disconnect_rx) = tokio::sync::watch::channel(false);

    let (raw_write_tx, write_rx) = tokio::sync::mpsc::channel::<ClientWrite>(128);
    let write_tx = ClientWriteTx(raw_write_tx);
    let writer = tokio::spawn(client_writer_loop(wr, write_rx));

    // Log connect early (prior to optional proxy protocol rewriting).
    {
        let ts = Utc::now().to_rfc3339();
        let sid = session_hex(session);
        let entry = format!(
            "ts={} kind=connect session={} ip={} port={}",
            logfmt_str(&ts),
            logfmt_str(&sid),
            logfmt_str(&peer_ip.to_string()),
            logfmt_str(&peer_port.to_string()),
        );
        eventlog.log_line(LogStream::All, &entry).await;
    }

    let mut iac = IacParser::new();
    let mut linebuf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut name: Option<String> = None;
    let mut is_bot: Option<bool> = None;
    let mut auth_method: Option<String> = None;
    // Small JSON blob asserted to the shard for permissions (groups/capabilities).
    let mut auth_blob: Option<Bytes> = None;
    let mut google_sub: Option<String> = None;
    let mut google_email: Option<String> = None;
    let mut oidc_sub: Option<String> = None;
    let mut oidc_email: Option<String> = None;
    #[derive(Debug, Clone)]
    enum PendingAutoWebAuth {
        Google {
            sub: String,
            email: Option<String>,
            caps: Option<Vec<String>>,
        },
        Oidc {
            sub: String,
            email: Option<String>,
            caps: Option<Vec<String>>,
        },
    }
    let mut pending_auto_webauth: Option<PendingAutoWebAuth> = None;
    let mut google_oauth_code: Option<String> = None;
    let mut race: Option<String> = None;
    let mut class: Option<String> = None;
    let mut sex: Option<String> = None;
    let mut pronouns: Option<String> = None;
    let mut public_ack_version = 0u32;
    let mut coc_ack_version = 0u32;
    let mut persist_account_profile = true;
    let mut password_echo_disabled = false;
    let mut state = ConnState::NeedName;
    let mut proxy_checked = false;

    write_tx
        .send(Bytes::from_static(
            b"slopmud (alpha)\r\ncharacter creation (step 1/4)\r\nname: ",
        ))
        .await
        .ok();

    let mut buf = [0u8; 4096];
    'read: loop {
        let n = tokio::select! {
            res = rd.read(&mut buf) => res?,
            _ = disconnect_rx.changed() => 0usize,
        };
        if n == 0 {
            break;
        }

        let (data, replies) = iac.parse(&buf[..n]);
        if !replies.is_empty() {
            let _ = write_tx.send(Bytes::from(replies)).await;
        }
        if data.is_empty() {
            continue;
        }

        linebuf.extend_from_slice(&data);
        while let Some(mut line_bytes) = try_pop_line(&mut linebuf) {
            if line_bytes.is_empty() {
                continue;
            }

            match state {
                ConnState::NeedName => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    // Optional PROXY protocol v1 (only trusted from loopback).
                    if !proxy_checked && peer_ip.is_loopback() && line.starts_with("PROXY ") {
                        if let Some((ip, port)) = parse_proxy_line_v1(&line) {
                            let old_ip = peer_ip;
                            peer_ip = ip;
                            peer_port = port;
                            proxy_checked = true;

                            let ts = Utc::now().to_rfc3339();
                            let sid = session_hex(session);
                            let entry = format!(
                                "ts={} kind=proxy session={} ip_old={} ip={} port={}",
                                logfmt_str(&ts),
                                logfmt_str(&sid),
                                logfmt_str(&old_ip.to_string()),
                                logfmt_str(&peer_ip.to_string()),
                                logfmt_str(&peer_port.to_string()),
                            );
                            eventlog.log_line(LogStream::All, &entry).await;

                            // Apply IP bans immediately after proxy rewrite.
                            if let Some(b) = { bans.lock().await.is_ip_banned(peer_ip).cloned() } {
                                let ts = Utc::now().to_rfc3339();
                                let entry = format!(
                                    "ts={} kind=reject_ip_ban session={} ip={} cidr={} created_by={} reason={}",
                                    logfmt_str(&ts),
                                    logfmt_str(&sid),
                                    logfmt_str(&peer_ip.to_string()),
                                    logfmt_str(&b.cidr),
                                    logfmt_str(&b.created_by),
                                    logfmt_str(&b.reason),
                                );
                                eventlog.log_line(LogStream::All, &entry).await;

                                let _ = write_tx
                                    .send(Bytes::from_static(b"banned\r\nbye\r\n"))
                                    .await;
                                break 'read;
                            }

                            // Initial `name:` prompt was already sent when the session opened.
                            // Don't emit a second prompt after accepting PROXY metadata.
                            continue;
                        }
                        // Fall through: treat as a name if it's not a valid PROXY line.
                    }

                    // Apply IP bans for direct clients (non-proxied).
                    if !proxy_checked {
                        proxy_checked = true;
                        if let Some(b) = { bans.lock().await.is_ip_banned(peer_ip).cloned() } {
                            let ts = Utc::now().to_rfc3339();
                            let sid = session_hex(session);
                            let entry = format!(
                                "ts={} kind=reject_ip_ban session={} ip={} cidr={} created_by={} reason={}",
                                logfmt_str(&ts),
                                logfmt_str(&sid),
                                logfmt_str(&peer_ip.to_string()),
                                logfmt_str(&b.cidr),
                                logfmt_str(&b.created_by),
                                logfmt_str(&b.reason),
                            );
                            eventlog.log_line(LogStream::All, &entry).await;

                            let _ = write_tx
                                .send(Bytes::from_static(b"banned\r\nbye\r\n"))
                                .await;
                            break 'read;
                        }
                    }

                    // Web-only fast path: slopmud_web can pre-auth a resumable session before any
                    // in-band character creation prompts. Only accept this from trusted loopback
                    // peers (static_web / slopmud_web).
                    if trusted_proxy_peer {
                        let webauth_rest = line
                            .strip_prefix("WEB_AUTH ")
                            .or_else(|| line.strip_prefix("GMCP Slopmud.WebAuth "));
                        if let Some(rest) = webauth_rest {
                            let req: WebAuthReq = match serde_json::from_str(rest) {
                                Ok(v) => v,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(b"web_auth: bad json\r\nname: "))
                                        .await;
                                    continue;
                                }
                            };
                            if let Err(msg) = verify_webauth_jwt(&cfg, &req) {
                                let out = format!("web_auth: {msg}\r\nname: ");
                                let _ = write_tx.send(Bytes::from(out)).await;
                                continue;
                            }

                            let action = req.action.trim().to_ascii_lowercase();
                            let method = req.method.trim().to_ascii_lowercase();

                            let mut uname = sanitize_name(&req.name);
                            if action == "auto" && (method == "google" || method == "oidc") {
                                let sub = if method == "google" {
                                    req.google_sub.as_deref().unwrap_or("").trim()
                                } else {
                                    req.oidc_sub.as_deref().unwrap_or("").trim()
                                };
                                if sub.is_empty() {
                                    let msg = if method == "google" {
                                        b"web_auth: missing google_sub\r\nname: ".as_slice()
                                    } else {
                                        b"web_auth: missing oidc_sub\r\nname: ".as_slice()
                                    };
                                    let _ = write_tx.send(Bytes::copy_from_slice(msg)).await;
                                    continue;
                                }

                                let linked_names = {
                                    let a = accounts.lock().await;
                                    a.linked_names_for_identity(&method, sub)
                                };

                                match linked_names.as_slice() {
                                    [] => {
                                        // No linked account yet. Keep the player in-band at `name:`
                                        // so they can choose an in-game character name, then we'll
                                        // create/link via this pending web auth identity.
                                        pending_auto_webauth = Some(if method == "google" {
                                            PendingAutoWebAuth::Google {
                                                sub: sub.to_string(),
                                                email: req.google_email.clone(),
                                                caps: req.caps.clone(),
                                            }
                                        } else {
                                            PendingAutoWebAuth::Oidc {
                                                sub: sub.to_string(),
                                                email: req.oidc_email.clone(),
                                                caps: req.caps.clone(),
                                            }
                                        });
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: no linked character yet; enter a character name to create/link\r\nname: ",
                                            ))
                                            .await;
                                        continue;
                                    }
                                    [only] => {
                                        uname = only.clone();
                                    }
                                    _ => {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: multiple linked accounts\r\nname: ",
                                            ))
                                            .await;
                                        continue;
                                    }
                                }
                            }

                            if uname.is_empty() {
                                let _ = write_tx
                                    .send(Bytes::from_static(
                                        b"web_auth: bad name (use letters/numbers/_/-, max 20)\r\nname: ",
                                    ))
                                    .await;
                                continue;
                            }

                            if let Some(b) = { bans.lock().await.is_char_banned(&uname).cloned() } {
                                let ts = Utc::now().to_rfc3339();
                                let sid = session_hex(session);
                                let entry = format!(
                                    "ts={} kind=reject_char_ban session={} ip={} name={} created_by={} reason={}",
                                    logfmt_str(&ts),
                                    logfmt_str(&sid),
                                    logfmt_str(&peer_ip.to_string()),
                                    logfmt_str(&uname),
                                    logfmt_str(&b.created_by),
                                    logfmt_str(&b.reason),
                                );
                                eventlog.log_line(LogStream::All, &entry).await;

                                let _ = write_tx
                                    .send(Bytes::from_static(b"banned\r\nbye\r\n"))
                                    .await;
                                break 'read;
                            }

                            let mut account_created = false;
                            let ok = match (action.as_str(), method.as_str()) {
                                ("create", "password") => {
                                    let pw = req.password.as_deref().unwrap_or("").as_bytes();
                                    if pw.len() < 8 {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: password too short (min 8)\r\nname: ",
                                            ))
                                            .await;
                                        false
                                    } else if {
                                        let a = accounts.lock().await;
                                        a.by_name.contains_key(&uname)
                                    } {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: name already taken\r\nname: ",
                                            ))
                                            .await;
                                        false
                                    } else {
                                        let salt = SaltString::generate(
                                            &mut password_hash::rand_core::OsRng,
                                        );
                                        let hash = Argon2::default()
                                            .hash_password(pw, &salt)
                                            .map_err(|e| {
                                                anyhow::anyhow!("hash_password failed: {e}")
                                            })?
                                            .to_string();

                                        let now_unix = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();
                                        {
                                            let mut a = accounts.lock().await;
                                            a.by_name.insert(
                                                uname.clone(),
                                                AccountRec {
                                                    name: uname.clone(),
                                                    pw_hash: Some(hash),
                                                    auth_identities: Vec::new(),
                                                    google_sub: None,
                                                    google_email: None,
                                                    oidc_sub: None,
                                                    oidc_email: None,
                                                    caps: None,
                                                    is_bot: None,
                                                    race: None,
                                                    class: None,
                                                    sex: None,
                                                    pronouns: None,
                                                    public_ack_version: 0,
                                                    coc_ack_version: 0,
                                                    email: None,
                                                    created_unix: now_unix,
                                                },
                                            );
                                            a.save()?;
                                        }

                                        account_created = true;
                                        auth_method = Some("password".to_string());
                                        auth_blob = Some(make_shard_auth_blob(
                                            &uname,
                                            "password",
                                            None,
                                            None,
                                            None,
                                            None,
                                            req.caps.as_deref(),
                                        ));
                                        name = Some(uname.clone());
                                        true
                                    }
                                }
                                ("login", "password") => {
                                    let pw = req.password.as_deref().unwrap_or("").as_bytes();
                                    let rec = {
                                        let a = accounts.lock().await;
                                        a.by_name.get(&uname).cloned()
                                    };
                                    match rec {
                                        None => {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"web_auth: account not found\r\nname: ",
                                                ))
                                                .await;
                                            false
                                        }
                                        Some(r) => match r.pw_hash.as_deref() {
                                            None => {
                                                let _ = write_tx
                                                    .send(Bytes::from_static(
                                                        b"web_auth: account has no password set\r\nname: ",
                                                    ))
                                                    .await;
                                                false
                                            }
                                            Some(hash) => {
                                                let ok = if let Ok(ph) = PasswordHash::new(hash) {
                                                    Argon2::default()
                                                        .verify_password(pw, &ph)
                                                        .is_ok()
                                                } else {
                                                    false
                                                };
                                                if !ok {
                                                    let _ = write_tx
                                                        .send(Bytes::from_static(
                                                            b"web_auth: bad password\r\nname: ",
                                                        ))
                                                        .await;
                                                    false
                                                } else {
                                                    auth_method = Some("password".to_string());
                                                    auth_blob = Some(make_shard_auth_blob(
                                                        &uname,
                                                        "password",
                                                        None,
                                                        None,
                                                        None,
                                                        None,
                                                        r.caps.as_deref(),
                                                    ));
                                                    name = Some(uname.clone());
                                                    true
                                                }
                                            }
                                        },
                                    }
                                }
                                ("create", "google")
                                | ("login", "google")
                                | ("auto", "google")
                                | ("link", "google") => {
                                    let sub = req.google_sub.as_deref().unwrap_or("").trim();
                                    if sub.is_empty() {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: missing google_sub\r\nname: ",
                                            ))
                                            .await;
                                        false
                                    } else {
                                        let email = req.google_email.clone();
                                        let now_unix = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();
                                        let exists = {
                                            let a = accounts.lock().await;
                                            a.by_name.get(&uname).cloned()
                                        };

                                        if action == "link" {
                                            let pw =
                                                req.password.as_deref().unwrap_or("").as_bytes();
                                            if pw.is_empty() {
                                                let _ = write_tx
                                                    .send(Bytes::from_static(
                                                        b"web_auth: missing password for link\r\nname: ",
                                                    ))
                                                    .await;
                                                false
                                            } else {
                                                let mut a = accounts.lock().await;
                                                if a.identity_linked_to_other_account(
                                                    "google", sub, &uname,
                                                ) {
                                                    let _ = write_tx
                                                        .send(Bytes::from_static(
                                                            b"web_auth: google identity already linked to another account\r\nname: ",
                                                        ))
                                                        .await;
                                                    false
                                                } else {
                                                    let linked_email = match a
                                                        .by_name
                                                        .get_mut(&uname)
                                                    {
                                                        None => {
                                                            let _ = write_tx
                                                                .send(Bytes::from_static(
                                                                    b"web_auth: account not found\r\nname: ",
                                                                ))
                                                                .await;
                                                            None
                                                        }
                                                        Some(r) => match r.pw_hash.as_deref() {
                                                            None => {
                                                                let _ = write_tx
                                                                    .send(Bytes::from_static(
                                                                        b"web_auth: account has no password set\r\nname: ",
                                                                    ))
                                                                    .await;
                                                                None
                                                            }
                                                            Some(hash) => {
                                                                let ok = if let Ok(ph) =
                                                                    PasswordHash::new(hash)
                                                                {
                                                                    Argon2::default()
                                                                        .verify_password(pw, &ph)
                                                                        .is_ok()
                                                                } else {
                                                                    false
                                                                };
                                                                if !ok {
                                                                    let _ = write_tx
                                                                        .send(Bytes::from_static(
                                                                            b"web_auth: bad password\r\nname: ",
                                                                        ))
                                                                        .await;
                                                                    None
                                                                } else {
                                                                    let _ = r.link_auth_identity(
                                                                        "google",
                                                                        sub,
                                                                        email.clone(),
                                                                    );
                                                                    r.auth_email_for_identity(
                                                                        "google", sub,
                                                                    )
                                                                    .or(email.clone())
                                                                }
                                                            }
                                                        },
                                                    };
                                                    if let Some(linked_email) = linked_email {
                                                        a.save()?;
                                                        google_sub = Some(sub.to_string());
                                                        google_email = Some(linked_email);
                                                        auth_method = Some("google".to_string());
                                                        auth_blob = Some(make_shard_auth_blob(
                                                            &uname,
                                                            "google",
                                                            Some(sub),
                                                            google_email.as_deref(),
                                                            None,
                                                            None,
                                                            req.caps.as_deref(),
                                                        ));
                                                        name = Some(uname.clone());
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                }
                                            }
                                        } else if action == "login" || action == "auto" {
                                            match exists {
                                                None => {
                                                    let _ = write_tx
                                                        .send(Bytes::from_static(
                                                            b"web_auth: account not found\r\nname: ",
                                                        ))
                                                        .await;
                                                    false
                                                }
                                                Some(r) => {
                                                    if !r.has_auth_identity("google", sub) {
                                                        let _ = write_tx
                                                            .send(Bytes::from_static(
                                                                b"web_auth: account not linked to google\r\nname: ",
                                                            ))
                                                            .await;
                                                        false
                                                    } else {
                                                        google_sub = Some(sub.to_string());
                                                        google_email = r
                                                            .auth_email_for_identity("google", sub)
                                                            .or(email.clone());
                                                        auth_method = Some("google".to_string());
                                                        auth_blob = Some(make_shard_auth_blob(
                                                            &uname,
                                                            "google",
                                                            Some(sub),
                                                            google_email.as_deref(),
                                                            None,
                                                            None,
                                                            req.caps.as_deref(),
                                                        ));
                                                        name = Some(uname.clone());
                                                        true
                                                    }
                                                }
                                            }
                                        } else if exists.is_some() {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"web_auth: name already taken\r\nname: ",
                                                ))
                                                .await;
                                            false
                                        } else {
                                            let created = {
                                                let mut a = accounts.lock().await;
                                                if a.identity_linked_to_other_account(
                                                    "google", sub, &uname,
                                                ) {
                                                    false
                                                } else {
                                                    a.by_name.insert(
                                                        uname.clone(),
                                                        AccountRec {
                                                            name: uname.clone(),
                                                            pw_hash: None,
                                                            auth_identities: vec![
                                                                AccountAuthIdentity {
                                                                    method: "google".to_string(),
                                                                    sub: sub.to_string(),
                                                                    email: email.clone(),
                                                                },
                                                            ],
                                                            google_sub: None,
                                                            google_email: None,
                                                            oidc_sub: None,
                                                            oidc_email: None,
                                                            caps: None,
                                                            is_bot: None,
                                                            race: None,
                                                            class: None,
                                                            sex: None,
                                                            pronouns: None,
                                                            public_ack_version: 0,
                                                            coc_ack_version: 0,
                                                            email: None,
                                                            created_unix: now_unix,
                                                        },
                                                    );
                                                    a.save()?;
                                                    true
                                                }
                                            };
                                            if !created {
                                                let _ = write_tx
                                                    .send(Bytes::from_static(
                                                        b"web_auth: google identity already linked to another account\r\nname: ",
                                                    ))
                                                    .await;
                                                false
                                            } else {
                                                account_created = true;
                                                google_sub = Some(sub.to_string());
                                                google_email = email.clone();
                                                auth_method = Some("google".to_string());
                                                auth_blob = Some(make_shard_auth_blob(
                                                    &uname,
                                                    "google",
                                                    Some(sub),
                                                    google_email.as_deref(),
                                                    None,
                                                    None,
                                                    req.caps.as_deref(),
                                                ));
                                                name = Some(uname.clone());
                                                true
                                            }
                                        }
                                    }
                                }
                                ("create", "oidc")
                                | ("login", "oidc")
                                | ("auto", "oidc")
                                | ("link", "oidc") => {
                                    let sub = req.oidc_sub.as_deref().unwrap_or("").trim();
                                    if sub.is_empty() {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"web_auth: missing oidc_sub\r\nname: ",
                                            ))
                                            .await;
                                        false
                                    } else {
                                        let email = req.oidc_email.clone();
                                        let now_unix = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();
                                        let exists = {
                                            let a = accounts.lock().await;
                                            a.by_name.get(&uname).cloned()
                                        };

                                        if action == "link" {
                                            let pw =
                                                req.password.as_deref().unwrap_or("").as_bytes();
                                            if pw.is_empty() {
                                                let _ = write_tx
                                                    .send(Bytes::from_static(
                                                        b"web_auth: missing password for link\r\nname: ",
                                                    ))
                                                    .await;
                                                false
                                            } else {
                                                let mut a = accounts.lock().await;
                                                if a.identity_linked_to_other_account(
                                                    "oidc", sub, &uname,
                                                ) {
                                                    let _ = write_tx
                                                        .send(Bytes::from_static(
                                                            b"web_auth: oidc identity already linked to another account\r\nname: ",
                                                        ))
                                                        .await;
                                                    false
                                                } else {
                                                    let linked_email = match a
                                                        .by_name
                                                        .get_mut(&uname)
                                                    {
                                                        None => {
                                                            let _ = write_tx
                                                                .send(Bytes::from_static(
                                                                    b"web_auth: account not found\r\nname: ",
                                                                ))
                                                                .await;
                                                            None
                                                        }
                                                        Some(r) => match r.pw_hash.as_deref() {
                                                            None => {
                                                                let _ = write_tx
                                                                    .send(Bytes::from_static(
                                                                        b"web_auth: account has no password set\r\nname: ",
                                                                    ))
                                                                    .await;
                                                                None
                                                            }
                                                            Some(hash) => {
                                                                let ok = if let Ok(ph) =
                                                                    PasswordHash::new(hash)
                                                                {
                                                                    Argon2::default()
                                                                        .verify_password(pw, &ph)
                                                                        .is_ok()
                                                                } else {
                                                                    false
                                                                };
                                                                if !ok {
                                                                    let _ = write_tx
                                                                        .send(Bytes::from_static(
                                                                            b"web_auth: bad password\r\nname: ",
                                                                        ))
                                                                        .await;
                                                                    None
                                                                } else {
                                                                    let _ = r.link_auth_identity(
                                                                        "oidc",
                                                                        sub,
                                                                        email.clone(),
                                                                    );
                                                                    r.auth_email_for_identity(
                                                                        "oidc", sub,
                                                                    )
                                                                    .or(email.clone())
                                                                }
                                                            }
                                                        },
                                                    };
                                                    if let Some(linked_email) = linked_email {
                                                        a.save()?;
                                                        oidc_sub = Some(sub.to_string());
                                                        oidc_email = Some(linked_email);
                                                        auth_method = Some("oidc".to_string());
                                                        auth_blob = Some(make_shard_auth_blob(
                                                            &uname,
                                                            "oidc",
                                                            None,
                                                            None,
                                                            Some(sub),
                                                            oidc_email.as_deref(),
                                                            req.caps.as_deref(),
                                                        ));
                                                        name = Some(uname.clone());
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                }
                                            }
                                        } else if action == "login" || action == "auto" {
                                            match exists {
                                                None => {
                                                    let _ = write_tx
                                                        .send(Bytes::from_static(
                                                            b"web_auth: account not found\r\nname: ",
                                                        ))
                                                        .await;
                                                    false
                                                }
                                                Some(r) => {
                                                    if !r.has_auth_identity("oidc", sub) {
                                                        let _ = write_tx
                                                            .send(Bytes::from_static(
                                                                b"web_auth: account not linked to oidc\r\nname: ",
                                                            ))
                                                            .await;
                                                        false
                                                    } else {
                                                        oidc_sub = Some(sub.to_string());
                                                        oidc_email = r
                                                            .auth_email_for_identity("oidc", sub)
                                                            .or(email.clone());
                                                        auth_method = Some("oidc".to_string());
                                                        auth_blob = Some(make_shard_auth_blob(
                                                            &uname,
                                                            "oidc",
                                                            None,
                                                            None,
                                                            Some(sub),
                                                            oidc_email.as_deref(),
                                                            req.caps.as_deref(),
                                                        ));
                                                        name = Some(uname.clone());
                                                        true
                                                    }
                                                }
                                            }
                                        } else if exists.is_some() {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"web_auth: name already taken\r\nname: ",
                                                ))
                                                .await;
                                            false
                                        } else {
                                            let created = {
                                                let mut a = accounts.lock().await;
                                                if a.identity_linked_to_other_account(
                                                    "oidc", sub, &uname,
                                                ) {
                                                    false
                                                } else {
                                                    a.by_name.insert(
                                                        uname.clone(),
                                                        AccountRec {
                                                            name: uname.clone(),
                                                            pw_hash: None,
                                                            auth_identities: vec![
                                                                AccountAuthIdentity {
                                                                    method: "oidc".to_string(),
                                                                    sub: sub.to_string(),
                                                                    email: email.clone(),
                                                                },
                                                            ],
                                                            google_sub: None,
                                                            google_email: None,
                                                            oidc_sub: None,
                                                            oidc_email: None,
                                                            caps: None,
                                                            is_bot: None,
                                                            race: None,
                                                            class: None,
                                                            sex: None,
                                                            pronouns: None,
                                                            public_ack_version: 0,
                                                            coc_ack_version: 0,
                                                            email: None,
                                                            created_unix: now_unix,
                                                        },
                                                    );
                                                    a.save()?;
                                                    true
                                                }
                                            };
                                            if !created {
                                                let _ = write_tx
                                                    .send(Bytes::from_static(
                                                        b"web_auth: oidc identity already linked to another account\r\nname: ",
                                                    ))
                                                    .await;
                                                false
                                            } else {
                                                account_created = true;
                                                oidc_sub = Some(sub.to_string());
                                                oidc_email = email.clone();
                                                auth_method = Some("oidc".to_string());
                                                auth_blob = Some(make_shard_auth_blob(
                                                    &uname,
                                                    "oidc",
                                                    None,
                                                    None,
                                                    Some(sub),
                                                    oidc_email.as_deref(),
                                                    req.caps.as_deref(),
                                                ));
                                                name = Some(uname.clone());
                                                true
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"web_auth: unsupported action/method\r\nname: ",
                                        ))
                                        .await;
                                    false
                                }
                            };

                            if !ok {
                                continue;
                            }
                            if account_created {
                                persist_account_profile = true;
                                public_ack_version = 0;
                                coc_ack_version = 0;
                                state = ConnState::NeedBotDisclosure;
                            } else if let Some(nm) = name.as_deref() {
                                let rec = {
                                    let a = accounts.lock().await;
                                    a.by_name.get(nm).cloned()
                                };
                                if let Some(rec) = rec.as_ref() {
                                    state = prepare_existing_account_onboarding(
                                        rec,
                                        &mut is_bot,
                                        &mut race,
                                        &mut class,
                                        &mut sex,
                                        &mut pronouns,
                                        &mut public_ack_version,
                                        &mut coc_ack_version,
                                        &mut persist_account_profile,
                                    );
                                } else {
                                    state = ConnState::NeedBotDisclosure;
                                }
                            } else {
                                state = ConnState::NeedBotDisclosure;
                            }
                            if let Some(prompt) = prompt_for_onboarding_state(state) {
                                let _ = write_tx.send(prompt).await;
                                continue;
                            }
                        }
                    }

                    let n = sanitize_name(&line);
                    if n.is_empty() {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"bad name (use letters/numbers/_/-, max 20)\r\nname: ",
                            ))
                            .await;
                        continue;
                    }

                    if let Some(b) = { bans.lock().await.is_char_banned(&n).cloned() } {
                        let ts = Utc::now().to_rfc3339();
                        let sid = session_hex(session);
                        let entry = format!(
                            "ts={} kind=reject_char_ban session={} ip={} name={} created_by={} reason={}",
                            logfmt_str(&ts),
                            logfmt_str(&sid),
                            logfmt_str(&peer_ip.to_string()),
                            logfmt_str(&n),
                            logfmt_str(&b.created_by),
                            logfmt_str(&b.reason),
                        );
                        eventlog.log_line(LogStream::All, &entry).await;

                        let _ = write_tx
                            .send(Bytes::from_static(b"banned\r\nbye\r\n"))
                            .await;
                        break 'read;
                    }

                    if let Some(pending) = pending_auto_webauth.clone() {
                        match pending {
                            PendingAutoWebAuth::Google { sub, email, caps } => {
                                let exists = {
                                    let a = accounts.lock().await;
                                    a.by_name.get(&n).cloned()
                                };
                                match exists {
                                    Some(r) => {
                                        if !r.has_auth_identity("google", sub.as_str()) {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"name already taken\r\nname: ",
                                                ))
                                                .await;
                                            continue;
                                        }
                                        google_sub = Some(sub.clone());
                                        google_email = r
                                            .auth_email_for_identity("google", sub.as_str())
                                            .or(email.clone());
                                        auth_method = Some("google".to_string());
                                        auth_blob = Some(make_shard_auth_blob(
                                            &n,
                                            "google",
                                            Some(sub.as_str()),
                                            google_email.as_deref(),
                                            None,
                                            None,
                                            caps.as_deref(),
                                        ));
                                        name = Some(n.clone());
                                        pending_auto_webauth = None;
                                        state = prepare_existing_account_onboarding(
                                            &r,
                                            &mut is_bot,
                                            &mut race,
                                            &mut class,
                                            &mut sex,
                                            &mut pronouns,
                                            &mut public_ack_version,
                                            &mut coc_ack_version,
                                            &mut persist_account_profile,
                                        );
                                        if let Some(prompt) = prompt_for_onboarding_state(state) {
                                            let _ = write_tx.send(prompt).await;
                                            continue;
                                        }
                                    }
                                    None => {
                                        let now_unix = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();
                                        {
                                            let mut a = accounts.lock().await;
                                            a.by_name.insert(
                                                n.clone(),
                                                AccountRec {
                                                    name: n.clone(),
                                                    pw_hash: None,
                                                    auth_identities: vec![AccountAuthIdentity {
                                                        method: "google".to_string(),
                                                        sub: sub.clone(),
                                                        email: email.clone(),
                                                    }],
                                                    google_sub: None,
                                                    google_email: None,
                                                    oidc_sub: None,
                                                    oidc_email: None,
                                                    caps: None,
                                                    is_bot: None,
                                                    race: None,
                                                    class: None,
                                                    sex: None,
                                                    pronouns: None,
                                                    public_ack_version: 0,
                                                    coc_ack_version: 0,
                                                    email: None,
                                                    created_unix: now_unix,
                                                },
                                            );
                                            a.save()?;
                                        }
                                        google_sub = Some(sub.clone());
                                        google_email = email.clone();
                                        auth_method = Some("google".to_string());
                                        auth_blob = Some(make_shard_auth_blob(
                                            &n,
                                            "google",
                                            Some(sub.as_str()),
                                            google_email.as_deref(),
                                            None,
                                            None,
                                            caps.as_deref(),
                                        ));
                                        name = Some(n.clone());
                                        pending_auto_webauth = None;
                                        state = ConnState::NeedBotDisclosure;
                                        let _ = write_tx.send(prompt_bot_disclosure()).await;
                                        continue;
                                    }
                                }
                            }
                            PendingAutoWebAuth::Oidc { sub, email, caps } => {
                                let exists = {
                                    let a = accounts.lock().await;
                                    a.by_name.get(&n).cloned()
                                };
                                match exists {
                                    Some(r) => {
                                        if !r.has_auth_identity("oidc", sub.as_str()) {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"name already taken\r\nname: ",
                                                ))
                                                .await;
                                            continue;
                                        }
                                        oidc_sub = Some(sub.clone());
                                        oidc_email = r
                                            .auth_email_for_identity("oidc", sub.as_str())
                                            .or(email.clone());
                                        auth_method = Some("oidc".to_string());
                                        auth_blob = Some(make_shard_auth_blob(
                                            &n,
                                            "oidc",
                                            None,
                                            None,
                                            Some(sub.as_str()),
                                            oidc_email.as_deref(),
                                            caps.as_deref(),
                                        ));
                                        name = Some(n.clone());
                                        pending_auto_webauth = None;
                                        state = prepare_existing_account_onboarding(
                                            &r,
                                            &mut is_bot,
                                            &mut race,
                                            &mut class,
                                            &mut sex,
                                            &mut pronouns,
                                            &mut public_ack_version,
                                            &mut coc_ack_version,
                                            &mut persist_account_profile,
                                        );
                                        if let Some(prompt) = prompt_for_onboarding_state(state) {
                                            let _ = write_tx.send(prompt).await;
                                            continue;
                                        }
                                    }
                                    None => {
                                        let now_unix = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();
                                        {
                                            let mut a = accounts.lock().await;
                                            a.by_name.insert(
                                                n.clone(),
                                                AccountRec {
                                                    name: n.clone(),
                                                    pw_hash: None,
                                                    auth_identities: vec![AccountAuthIdentity {
                                                        method: "oidc".to_string(),
                                                        sub: sub.clone(),
                                                        email: email.clone(),
                                                    }],
                                                    google_sub: None,
                                                    google_email: None,
                                                    oidc_sub: None,
                                                    oidc_email: None,
                                                    caps: None,
                                                    is_bot: None,
                                                    race: None,
                                                    class: None,
                                                    sex: None,
                                                    pronouns: None,
                                                    public_ack_version: 0,
                                                    coc_ack_version: 0,
                                                    email: None,
                                                    created_unix: now_unix,
                                                },
                                            );
                                            a.save()?;
                                        }
                                        oidc_sub = Some(sub.clone());
                                        oidc_email = email.clone();
                                        auth_method = Some("oidc".to_string());
                                        auth_blob = Some(make_shard_auth_blob(
                                            &n,
                                            "oidc",
                                            None,
                                            None,
                                            Some(sub.as_str()),
                                            oidc_email.as_deref(),
                                            caps.as_deref(),
                                        ));
                                        name = Some(n.clone());
                                        pending_auto_webauth = None;
                                        state = ConnState::NeedBotDisclosure;
                                        let _ = write_tx.send(prompt_bot_disclosure()).await;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    name = Some(n);
                    state = ConnState::NeedAuthMethod;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"\r\nauth method:\r\n- password\r\n- google\r\ntype: password | google\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedAuthMethod => {
                    let line_raw = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    let line = line_raw.to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }

                    if trusted_proxy_peer {
                        let webauth_rest = line_raw
                            .strip_prefix("WEB_AUTH ")
                            .or_else(|| line_raw.strip_prefix("GMCP Slopmud.WebAuth "));
                        if let Some(rest) = webauth_rest {
                            let req: WebAuthReq = match serde_json::from_str(rest) {
                                Ok(v) => v,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"web_auth: bad json\r\nplease type: password | google\r\n> ",
                                        ))
                                        .await;
                                    continue;
                                }
                            };
                            if let Err(msg) = verify_webauth_jwt(&cfg, &req) {
                                let out = format!(
                                    "web_auth: {msg}\r\nplease type: password | google\r\n> "
                                );
                                let _ = write_tx.send(Bytes::from(out)).await;
                                continue;
                            }

                            let action = req.action.trim().to_ascii_lowercase();
                            let method = req.method.trim().to_ascii_lowercase();
                            if action == "auto" && (method == "google" || method == "oidc") {
                                let sub = if method == "google" {
                                    req.google_sub.as_deref().unwrap_or("").trim()
                                } else {
                                    req.oidc_sub.as_deref().unwrap_or("").trim()
                                };
                                if sub.is_empty() {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"web_auth: missing provider sub\r\nplease type: password | google\r\n> ",
                                        ))
                                        .await;
                                    continue;
                                }

                                let mut uname = name.clone().unwrap_or_default();
                                if uname.is_empty() {
                                    uname = sanitize_name(&req.name);
                                }
                                if uname.is_empty() {
                                    let linked_names = {
                                        let a = accounts.lock().await;
                                        a.linked_names_for_identity(&method, sub)
                                    };
                                    if linked_names.len() == 1 {
                                        uname = linked_names[0].clone();
                                    }
                                }
                                if uname.is_empty() {
                                    pending_auto_webauth = Some(if method == "google" {
                                        PendingAutoWebAuth::Google {
                                            sub: sub.to_string(),
                                            email: req.google_email.clone(),
                                            caps: req.caps.clone(),
                                        }
                                    } else {
                                        PendingAutoWebAuth::Oidc {
                                            sub: sub.to_string(),
                                            email: req.oidc_email.clone(),
                                            caps: req.caps.clone(),
                                        }
                                    });
                                    state = ConnState::NeedName;
                                    let _ = write_tx.send(Bytes::from_static(b"name: ")).await;
                                    continue;
                                }

                                let email = if method == "google" {
                                    req.google_email.clone()
                                } else {
                                    req.oidc_email.clone()
                                };
                                let now_unix = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                let mut account_created = false;
                                {
                                    let mut a = accounts.lock().await;
                                    if let Some(r) = a.by_name.get(&uname) {
                                        if !r.has_auth_identity(&method, sub) {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"name already taken\r\nplease type: password | google\r\n> ",
                                                ))
                                                .await;
                                            continue;
                                        }
                                    } else {
                                        a.by_name.insert(
                                            uname.clone(),
                                            AccountRec {
                                                name: uname.clone(),
                                                pw_hash: None,
                                                auth_identities: vec![AccountAuthIdentity {
                                                    method: method.clone(),
                                                    sub: sub.to_string(),
                                                    email: email.clone(),
                                                }],
                                                google_sub: None,
                                                google_email: None,
                                                oidc_sub: None,
                                                oidc_email: None,
                                                caps: None,
                                                is_bot: None,
                                                race: None,
                                                class: None,
                                                sex: None,
                                                pronouns: None,
                                                public_ack_version: 0,
                                                coc_ack_version: 0,
                                                email: None,
                                                created_unix: now_unix,
                                            },
                                        );
                                        a.save()?;
                                        account_created = true;
                                    }
                                }

                                if method == "google" {
                                    google_sub = Some(sub.to_string());
                                    google_email = email.clone();
                                    auth_method = Some("google".to_string());
                                    auth_blob = Some(make_shard_auth_blob(
                                        &uname,
                                        "google",
                                        Some(sub),
                                        email.as_deref(),
                                        None,
                                        None,
                                        req.caps.as_deref(),
                                    ));
                                } else {
                                    oidc_sub = Some(sub.to_string());
                                    oidc_email = email.clone();
                                    auth_method = Some("oidc".to_string());
                                    auth_blob = Some(make_shard_auth_blob(
                                        &uname,
                                        "oidc",
                                        None,
                                        None,
                                        Some(sub),
                                        email.as_deref(),
                                        req.caps.as_deref(),
                                    ));
                                }

                                name = Some(uname.clone());
                                pending_auto_webauth = None;
                                if account_created {
                                    persist_account_profile = true;
                                    public_ack_version = 0;
                                    coc_ack_version = 0;
                                    state = ConnState::NeedBotDisclosure;
                                } else {
                                    let rec = {
                                        let a = accounts.lock().await;
                                        a.by_name.get(&uname).cloned()
                                    };
                                    if let Some(rec) = rec.as_ref() {
                                        state = prepare_existing_account_onboarding(
                                            rec,
                                            &mut is_bot,
                                            &mut race,
                                            &mut class,
                                            &mut sex,
                                            &mut pronouns,
                                            &mut public_ack_version,
                                            &mut coc_ack_version,
                                            &mut persist_account_profile,
                                        );
                                    } else {
                                        state = ConnState::NeedBotDisclosure;
                                    }
                                }
                                if let Some(prompt) = prompt_for_onboarding_state(state) {
                                    let _ = write_tx.send(prompt).await;
                                    continue;
                                }
                            }
                        }
                    }

                    let uname = name.as_deref().expect("name set");
                    let rec = {
                        let a = accounts.lock().await;
                        a.by_name.get(uname).cloned()
                    };

                    match line.as_str() {
                        "password" => {
                            auth_method = Some("password".to_string());
                            let exists = rec.is_some();
                            if let Some(r) = rec.as_ref() {
                                if r.pw_hash.is_none() {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"account has no password; use google\r\n> ",
                                        ))
                                        .await;
                                    continue;
                                }
                            }

                            // Disable local echo for password entry (best-effort via telnet negotiation).
                            password_echo_disabled = true;
                            let mut b = Vec::new();
                            b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                            if exists {
                                state = ConnState::NeedPasswordLogin;
                                b.extend_from_slice(b"password (never logged/echoed): ");
                            } else {
                                state = ConnState::NeedPasswordCreate;
                                b.extend_from_slice(
                                    b"set password (never logged/echoed; min 8 chars): ",
                                );
                            }
                            let _ = write_tx.send(Bytes::from(b)).await;
                            continue;
                        }
                        "google" => {
                            auth_method = Some("google".to_string());
                            if let Some(r) = rec.as_ref() {
                                if !r.has_auth_method("google") {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"account not linked to google; use password\r\n> ",
                                        ))
                                        .await;
                                    continue;
                                }
                            }

                            std::fs::create_dir_all(&cfg.google_oauth_dir)?;

                            let mut code_b = [0u8; 8];
                            getrandom::getrandom(&mut code_b).expect("getrandom");
                            let code = hex_lower(&code_b);

                            let mut ver_b = [0u8; 32];
                            getrandom::getrandom(&mut ver_b).expect("getrandom");
                            let verifier = hex_lower(&ver_b);

                            let now_unix = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();

                            let pending = GoogleOAuthPending {
                                code: code.clone(),
                                verifier,
                                status: "pending".to_string(),
                                created_unix: now_unix,
                                updated_unix: None,
                                google_sub: None,
                                google_email: None,
                                error: None,
                                return_to: None,
                            };

                            let mut path = PathBuf::from(&cfg.google_oauth_dir);
                            path.push(format!("{}.json", code));
                            let tmp = path.with_extension("json.tmp");
                            std::fs::write(&tmp, serde_json::to_string_pretty(&pending)?)?;
                            std::fs::rename(&tmp, &path)?;

                            let base = cfg.google_auth_base_url.trim_end_matches('/');
                            let url = format!("{base}/auth/google?code={}", pending.code);
                            google_oauth_code = Some(pending.code);
                            state = ConnState::NeedGoogleWait;

                            let msg = format!(
                                "open this url in a browser to sign in:\r\n  {url}\r\nthen type: check\r\n(or type: cancel)\r\n> "
                            );
                            let _ = write_tx.send(Bytes::from(msg)).await;
                            continue;
                        }
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(b"please type: password | google\r\n> "))
                                .await;
                            continue;
                        }
                    }
                }
                ConnState::NeedGoogleWait => {
                    let line_raw = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line_raw.is_empty() {
                        continue;
                    }
                    let line = line_raw.to_ascii_lowercase();

                    // Allow trusted web proxy auth to complete an OAuth-waiting session without
                    // requiring a manual `check` command.
                    if trusted_proxy_peer {
                        let webauth_rest = line_raw
                            .strip_prefix("WEB_AUTH ")
                            .or_else(|| line_raw.strip_prefix("GMCP Slopmud.WebAuth "));
                        if let Some(rest) = webauth_rest {
                            let req: WebAuthReq = match serde_json::from_str(rest) {
                                Ok(v) => v,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"web_auth: bad json\r\ntype: check | cancel\r\n> ",
                                        ))
                                        .await;
                                    continue;
                                }
                            };
                            if let Err(msg) = verify_webauth_jwt(&cfg, &req) {
                                let out = format!("web_auth: {msg}\r\ntype: check | cancel\r\n> ");
                                let _ = write_tx.send(Bytes::from(out)).await;
                                continue;
                            }

                            let action = req.action.trim().to_ascii_lowercase();
                            let method = req.method.trim().to_ascii_lowercase();
                            if action != "auto" || (method != "google" && method != "oidc") {
                                let _ = write_tx
                                    .send(Bytes::from_static(b"type: check | cancel\r\n> "))
                                    .await;
                                continue;
                            }

                            let sub = if method == "google" {
                                req.google_sub.as_deref().unwrap_or("").trim()
                            } else {
                                req.oidc_sub.as_deref().unwrap_or("").trim()
                            };
                            if sub.is_empty() {
                                let msg = if method == "google" {
                                    b"web_auth: missing google_sub\r\ntype: check | cancel\r\n> "
                                        .as_slice()
                                } else {
                                    b"web_auth: missing oidc_sub\r\ntype: check | cancel\r\n> "
                                        .as_slice()
                                };
                                let _ = write_tx.send(Bytes::copy_from_slice(msg)).await;
                                continue;
                            }

                            let mut uname = name.clone().unwrap_or_default();
                            if uname.is_empty() {
                                uname = sanitize_name(&req.name);
                            }
                            if uname.is_empty() {
                                let linked_names = {
                                    let a = accounts.lock().await;
                                    a.linked_names_for_identity(&method, sub)
                                };
                                if linked_names.len() == 1 {
                                    uname = linked_names[0].clone();
                                }
                            }
                            if uname.is_empty() {
                                let _ = write_tx
                                    .send(Bytes::from_static(
                                        b"web_auth: no linked character found\r\ntype: check | cancel\r\n> ",
                                    ))
                                    .await;
                                continue;
                            }

                            let now_unix = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let email = if method == "google" {
                                req.google_email.clone()
                            } else {
                                req.oidc_email.clone()
                            };

                            let mut account_created = false;
                            {
                                let mut a = accounts.lock().await;
                                if let Some(r) = a.by_name.get(&uname) {
                                    if !r.has_auth_identity(&method, sub) {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"name already taken\r\nbye\r\n",
                                            ))
                                            .await;
                                        break 'read;
                                    }
                                } else {
                                    a.by_name.insert(
                                        uname.clone(),
                                        AccountRec {
                                            name: uname.clone(),
                                            pw_hash: None,
                                            auth_identities: vec![AccountAuthIdentity {
                                                method: method.clone(),
                                                sub: sub.to_string(),
                                                email: email.clone(),
                                            }],
                                            google_sub: None,
                                            google_email: None,
                                            oidc_sub: None,
                                            oidc_email: None,
                                            caps: None,
                                            is_bot: None,
                                            race: None,
                                            class: None,
                                            sex: None,
                                            pronouns: None,
                                            public_ack_version: 0,
                                            coc_ack_version: 0,
                                            email: None,
                                            created_unix: now_unix,
                                        },
                                    );
                                    a.save()?;
                                    account_created = true;
                                }
                            }

                            if method == "google" {
                                google_sub = Some(sub.to_string());
                                google_email = email.clone();
                                auth_method = Some("google".to_string());
                                auth_blob = Some(make_shard_auth_blob(
                                    &uname,
                                    "google",
                                    Some(sub),
                                    email.as_deref(),
                                    None,
                                    None,
                                    req.caps.as_deref(),
                                ));
                            } else {
                                oidc_sub = Some(sub.to_string());
                                oidc_email = email.clone();
                                auth_method = Some("oidc".to_string());
                                auth_blob = Some(make_shard_auth_blob(
                                    &uname,
                                    "oidc",
                                    None,
                                    None,
                                    Some(sub),
                                    email.as_deref(),
                                    req.caps.as_deref(),
                                ));
                            }
                            name = Some(uname.clone());

                            if let Some(code) = google_oauth_code.take() {
                                let mut path = PathBuf::from(&cfg.google_oauth_dir);
                                path.push(format!("{}.json", code));
                                let _ = std::fs::remove_file(&path);
                            }

                            let mut resumed_live = false;
                            if let Some(nm) = name.as_deref() {
                                let prior = {
                                    let m = sessions.lock().await;
                                    m.iter()
                                        .find(|(sid, s)| {
                                            **sid != session && s.name.eq_ignore_ascii_case(nm)
                                        })
                                        .map(|(sid, s)| (*sid, s.clone()))
                                };
                                if let Some((old_sid, old)) = prior {
                                    let _ = old.disconnect_tx.send(true);
                                    {
                                        let mut m = sessions.lock().await;
                                        m.remove(&old_sid);
                                    }
                                    let _ = shard_tx
                                        .send(ShardMsg {
                                            t: REQ_DETACH,
                                            session: old_sid,
                                            body: Bytes::new(),
                                        })
                                        .await;
                                    is_bot = Some(old.is_bot);
                                    race = Some(old.race);
                                    class = Some(old.class);
                                    sex = Some(old.sex);
                                    pronouns = Some(old.pronouns);
                                    state = ConnState::NeedSex;
                                    resumed_live = true;
                                }
                            }
                            if !resumed_live {
                                if account_created {
                                    persist_account_profile = true;
                                    public_ack_version = 0;
                                    coc_ack_version = 0;
                                    state = ConnState::NeedBotDisclosure;
                                } else {
                                    let rec = {
                                        let a = accounts.lock().await;
                                        a.by_name.get(&uname).cloned()
                                    };
                                    if let Some(rec) = rec.as_ref() {
                                        state = prepare_existing_account_onboarding(
                                            rec,
                                            &mut is_bot,
                                            &mut race,
                                            &mut class,
                                            &mut sex,
                                            &mut pronouns,
                                            &mut public_ack_version,
                                            &mut coc_ack_version,
                                            &mut persist_account_profile,
                                        );
                                    } else {
                                        state = ConnState::NeedBotDisclosure;
                                    }
                                }
                                if let Some(prompt) = prompt_for_onboarding_state(state) {
                                    let _ = write_tx.send(prompt).await;
                                    continue;
                                }
                            }
                        }
                    }

                    let Some(code) = google_oauth_code.as_deref() else {
                        state = ConnState::NeedAuthMethod;
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"oauth state lost; pick auth method\r\n> ",
                            ))
                            .await;
                        continue;
                    };

                    let mut path = PathBuf::from(&cfg.google_oauth_dir);
                    path.push(format!("{}.json", code));

                    match line.as_str() {
                        "cancel" => {
                            let _ = std::fs::remove_file(&path);
                            google_oauth_code = None;
                            state = ConnState::NeedAuthMethod;
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"cancelled\r\nauth method:\r\n- password\r\n- google\r\ntype: password | google\r\n> ",
                                ))
                                .await;
                            continue;
                        }
                        "check" => {
                            let pending_s = match std::fs::read_to_string(&path) {
                                Ok(s) => s,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(b"still waiting\r\n> "))
                                        .await;
                                    continue;
                                }
                            };
                            let pending: GoogleOAuthPending = match serde_json::from_str(&pending_s)
                            {
                                Ok(v) => v,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(b"oauth file corrupted\r\n> "))
                                        .await;
                                    continue;
                                }
                            };

                            // Expire after 15 minutes.
                            let now_unix = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            if now_unix.saturating_sub(pending.created_unix) > 15 * 60 {
                                let _ = std::fs::remove_file(&path);
                                google_oauth_code = None;
                                state = ConnState::NeedAuthMethod;
                                let _ = write_tx
                                    .send(Bytes::from_static(b"oauth expired; try again\r\n> "))
                                    .await;
                                continue;
                            }

                            if pending.status == "pending" {
                                let _ = write_tx
                                    .send(Bytes::from_static(b"still waiting\r\n> "))
                                    .await;
                                continue;
                            }

                            if pending.status == "err" {
                                let msg =
                                    pending.error.unwrap_or_else(|| "oauth failed".to_string());
                                let _ = std::fs::remove_file(&path);
                                google_oauth_code = None;
                                state = ConnState::NeedAuthMethod;
                                let _ = write_tx.send(Bytes::from(format!("{msg}\r\n> "))).await;
                                continue;
                            }

                            if pending.status != "ok" {
                                let _ = write_tx
                                    .send(Bytes::from_static(b"oauth status unknown\r\n> "))
                                    .await;
                                continue;
                            }

                            let sub = match pending.google_sub.as_deref() {
                                Some(s) if !s.is_empty() => s,
                                _ => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(b"oauth missing sub\r\n> "))
                                        .await;
                                    continue;
                                }
                            };
                            let email = pending.google_email.clone();
                            google_sub = Some(sub.to_string());
                            google_email = email.clone();

                            // Bind to account name.
                            let uname = name.as_deref().expect("name set").to_string();
                            let mut account_created = false;
                            {
                                let mut a = accounts.lock().await;
                                if let Some(r) = a.by_name.get(&uname) {
                                    if !r.has_auth_identity("google", sub) {
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"name already taken\r\nbye\r\n",
                                            ))
                                            .await;
                                        break 'read;
                                    }
                                } else {
                                    a.by_name.insert(
                                        uname.clone(),
                                        AccountRec {
                                            name: uname.clone(),
                                            pw_hash: None,
                                            auth_identities: vec![AccountAuthIdentity {
                                                method: "google".to_string(),
                                                sub: sub.to_string(),
                                                email: email.clone(),
                                            }],
                                            google_sub: None,
                                            google_email: None,
                                            oidc_sub: None,
                                            oidc_email: None,
                                            caps: None,
                                            is_bot: None,
                                            race: None,
                                            class: None,
                                            sex: None,
                                            pronouns: None,
                                            public_ack_version: 0,
                                            coc_ack_version: 0,
                                            email: None,
                                            created_unix: now_unix,
                                        },
                                    );
                                    a.save()?;
                                    account_created = true;
                                }
                            }

                            auth_blob = Some(make_shard_auth_blob(
                                &uname,
                                "google",
                                Some(sub),
                                email.as_deref(),
                                None,
                                None,
                                None,
                            ));

                            let _ = std::fs::remove_file(&path);
                            google_oauth_code = None;

                            if account_created {
                                persist_account_profile = true;
                                public_ack_version = 0;
                                coc_ack_version = 0;
                                state = ConnState::NeedBotDisclosure;
                            } else {
                                let rec = {
                                    let a = accounts.lock().await;
                                    a.by_name.get(&uname).cloned()
                                };
                                if let Some(rec) = rec.as_ref() {
                                    state = prepare_existing_account_onboarding(
                                        rec,
                                        &mut is_bot,
                                        &mut race,
                                        &mut class,
                                        &mut sex,
                                        &mut pronouns,
                                        &mut public_ack_version,
                                        &mut coc_ack_version,
                                        &mut persist_account_profile,
                                    );
                                } else {
                                    state = ConnState::NeedBotDisclosure;
                                }
                            }
                            if let Some(prompt) = prompt_for_onboarding_state(state) {
                                let _ = write_tx.send(prompt).await;
                                continue;
                            }
                        }
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(b"type: check | cancel\r\n> "))
                                .await;
                            continue;
                        }
                    }
                }
                ConnState::NeedPasswordCreate => {
                    let uname = name.as_deref().expect("name set");
                    let now = std::time::Instant::now();
                    let wait = {
                        let mut t = login_throttle.lock().await;
                        t.wait(peer_ip, uname, now)
                    };
                    if !wait.is_zero() {
                        let wait_s = wait_seconds(wait);
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\nrate limit: retry in ");
                        b.extend_from_slice(wait_s.to_string().as_bytes());
                        b.extend_from_slice(b"s\r\n");
                        if !password_echo_disabled {
                            password_echo_disabled = true;
                            b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        }
                        b.extend_from_slice(b"set password (min 8 chars): ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    // Never convert to String (avoid extra copies) and never log.
                    // Trim ASCII whitespace at the ends.
                    let pw = trim_ascii_ws(&line_bytes);

                    let ok = pw.len() >= 8;

                    // Wipe the user-provided password bytes ASAP.
                    // Note: we need pw for hashing below, so we wipe later on success/failure branches.

                    // Re-enable echo (best-effort).
                    if password_echo_disabled {
                        let _ = write_tx
                            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                            .await;
                        password_echo_disabled = false;
                    }

                    if !ok {
                        let delay = {
                            let mut t = login_throttle.lock().await;
                            t.note_failure(peer_ip, uname, now)
                        };
                        let delay_s = wait_seconds(delay);

                        // Re-disable echo for retry.
                        password_echo_disabled = true;
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\npassword too short; retry in ");
                        b.extend_from_slice(delay_s.to_string().as_bytes());
                        b.extend_from_slice(b"s\r\n");
                        b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        b.extend_from_slice(b"set password (min 8 chars): ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    let _ = write_tx.send(Bytes::from_static(b"\r\n")).await;

                    // Store only a salted hash, never the raw password.
                    let salt = SaltString::generate(&mut password_hash::rand_core::OsRng);
                    let hash = Argon2::default()
                        .hash_password(pw, &salt)
                        .map_err(|e| anyhow::anyhow!("hash_password failed: {e}"))?
                        .to_string();

                    let now_unix = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    {
                        let mut a = accounts.lock().await;
                        let uname = name.as_ref().expect("name set").clone();
                        a.by_name.insert(
                            uname.clone(),
                            AccountRec {
                                name: uname,
                                pw_hash: Some(hash),
                                auth_identities: Vec::new(),
                                google_sub: None,
                                google_email: None,
                                oidc_sub: None,
                                oidc_email: None,
                                caps: None,
                                is_bot: None,
                                race: None,
                                class: None,
                                sex: None,
                                pronouns: None,
                                public_ack_version: 0,
                                coc_ack_version: 0,
                                email: None,
                                created_unix: now_unix,
                            },
                        );
                        a.save()?;
                    }

                    {
                        let mut t = login_throttle.lock().await;
                        t.note_success(peer_ip, uname);
                    }

                    // For password auth, the shard principal is acct:<name> (via the auth blob).
                    auth_blob = Some(make_shard_auth_blob(
                        name.as_deref().unwrap_or(""),
                        "password",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ));

                    line_bytes.zeroize();
                    let mut resumed_live = false;
                    if let Some(nm) = name.as_deref() {
                        let prior = {
                            let m = sessions.lock().await;
                            m.iter()
                                .find(|(sid, s)| {
                                    **sid != session && s.name.eq_ignore_ascii_case(nm)
                                })
                                .map(|(sid, s)| (*sid, s.clone()))
                        };
                        if let Some((old_sid, old)) = prior {
                            let _ = old.disconnect_tx.send(true);
                            {
                                let mut m = sessions.lock().await;
                                m.remove(&old_sid);
                            }
                            let _ = shard_tx
                                .send(ShardMsg {
                                    t: REQ_DETACH,
                                    session: old_sid,
                                    body: Bytes::new(),
                                })
                                .await;
                            is_bot = Some(old.is_bot);
                            race = Some(old.race);
                            class = Some(old.class);
                            sex = Some(old.sex);
                            pronouns = Some(old.pronouns);
                            state = ConnState::NeedSex;
                            resumed_live = true;
                        }
                    }
                    if !resumed_live {
                        persist_account_profile = true;
                        public_ack_version = 0;
                        coc_ack_version = 0;
                        state = ConnState::NeedBotDisclosure;
                        let _ = write_tx.send(prompt_bot_disclosure()).await;
                        continue;
                    }
                }
                ConnState::NeedPasswordLogin => {
                    let uname = name.as_deref().expect("name set");
                    let now = std::time::Instant::now();
                    let wait = {
                        let mut t = login_throttle.lock().await;
                        t.wait(peer_ip, uname, now)
                    };
                    if !wait.is_zero() {
                        let wait_s = wait_seconds(wait);
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\nrate limit: retry in ");
                        b.extend_from_slice(wait_s.to_string().as_bytes());
                        b.extend_from_slice(b"s\r\n");
                        if !password_echo_disabled {
                            password_echo_disabled = true;
                            b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        }
                        b.extend_from_slice(b"password: ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    let pw = trim_ascii_ws(&line_bytes);
                    let rec = {
                        let a = accounts.lock().await;
                        a.by_name.get(uname).cloned()
                    };
                    let (hash, caps) = match rec.as_ref() {
                        Some(r) => (r.pw_hash.clone(), r.caps.clone()),
                        None => (None, None),
                    };

                    if hash.as_deref().is_none() {
                        // Re-enable echo (best-effort).
                        if password_echo_disabled {
                            let _ = write_tx
                                .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                                .await;
                            password_echo_disabled = false;
                        }
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"\r\naccount has no password set\r\nbye\r\n",
                            ))
                            .await;
                        line_bytes.zeroize();
                        break 'read;
                    }

                    let ok = if let Some(hash) = hash {
                        if let Ok(ph) = PasswordHash::new(&hash) {
                            Argon2::default().verify_password(pw, &ph).is_ok()
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    // Re-enable echo (best-effort).
                    if password_echo_disabled {
                        let _ = write_tx
                            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
                            .await;
                        password_echo_disabled = false;
                    }

                    if !ok {
                        let delay = {
                            let mut t = login_throttle.lock().await;
                            t.note_failure(peer_ip, uname, now)
                        };
                        let delay_s = wait_seconds(delay);

                        password_echo_disabled = true;
                        let mut b = Vec::new();
                        b.extend_from_slice(b"\r\nbad password; retry in ");
                        b.extend_from_slice(delay_s.to_string().as_bytes());
                        b.extend_from_slice(b"s\r\n");
                        b.extend_from_slice(telnet_will(TELNET_OPT_ECHO).as_slice());
                        b.extend_from_slice(b"password: ");
                        let _ = write_tx.send(Bytes::from(b)).await;
                        line_bytes.zeroize();
                        continue;
                    }

                    let _ = write_tx.send(Bytes::from_static(b"\r\n")).await;

                    {
                        let mut t = login_throttle.lock().await;
                        t.note_success(peer_ip, uname);
                    }

                    // For password auth, the shard principal is acct:<name> (via the auth blob).
                    auth_blob = Some(make_shard_auth_blob(
                        name.as_deref().unwrap_or(""),
                        "password",
                        None,
                        None,
                        None,
                        None,
                        caps.as_deref(),
                    ));

                    line_bytes.zeroize();
                    let mut resumed_live = false;
                    if let Some(nm) = name.as_deref() {
                        let prior = {
                            let m = sessions.lock().await;
                            m.iter()
                                .find(|(sid, s)| {
                                    **sid != session && s.name.eq_ignore_ascii_case(nm)
                                })
                                .map(|(sid, s)| (*sid, s.clone()))
                        };
                        if let Some((old_sid, old)) = prior {
                            let _ = old.disconnect_tx.send(true);
                            {
                                let mut m = sessions.lock().await;
                                m.remove(&old_sid);
                            }
                            let _ = shard_tx
                                .send(ShardMsg {
                                    t: REQ_DETACH,
                                    session: old_sid,
                                    body: Bytes::new(),
                                })
                                .await;
                            is_bot = Some(old.is_bot);
                            race = Some(old.race);
                            class = Some(old.class);
                            sex = Some(old.sex);
                            pronouns = Some(old.pronouns);
                            state = ConnState::NeedSex;
                            resumed_live = true;
                        }
                    }
                    if !resumed_live {
                        if let Some(rec) = rec.as_ref() {
                            state = prepare_existing_account_onboarding(
                                rec,
                                &mut is_bot,
                                &mut race,
                                &mut class,
                                &mut sex,
                                &mut pronouns,
                                &mut public_ack_version,
                                &mut coc_ack_version,
                                &mut persist_account_profile,
                            );
                        } else {
                            state = ConnState::NeedBotDisclosure;
                        }
                        if let Some(prompt) = prompt_for_onboarding_state(state) {
                            let _ = write_tx.send(prompt).await;
                            continue;
                        }
                    }
                }
                ConnState::NeedBotDisclosure => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    let b = match v.as_str() {
                        "human" => false,
                        "bot" => true,
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(b"please type: human | bot\r\n> "))
                                .await;
                            continue;
                        }
                    };
                    is_bot = Some(b);
                    state = ConnState::NeedPublicAck;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 3/4)\r\ncontent + licensing:\r\n- anything you submit - consider it publicly licensed and publicly published\r\n- zero privacy: logs may be shared and used for training\r\n- exception: passwords are never logged/echoed; only password hashes are stored\r\ntype: agree\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedPublicAck => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    if v != "agree" {
                        let _ = write_tx
                            .send(Bytes::from_static(b"type: agree\r\n> "))
                            .await;
                        continue;
                    }
                    public_ack_version = PUBLIC_ACK_VERSION;
                    if coc_ack_version < COC_ACK_VERSION {
                        state = ConnState::NeedCocAck;
                        let _ = write_tx.send(prompt_coc_ack()).await;
                        continue;
                    }
                    if race.is_some() && class.is_some() && sex.is_some() && pronouns.is_some() {
                        state = ConnState::NeedSex;
                    } else {
                        state = ConnState::NeedRace;
                        let _ = write_tx.send(prompt_race()).await;
                        continue;
                    }
                }
                ConnState::NeedCocAck => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let v = line.to_ascii_lowercase();
                    if v != "agree" {
                        let _ = write_tx
                            .send(Bytes::from_static(b"type: agree\r\n> "))
                            .await;
                        continue;
                    }
                    coc_ack_version = COC_ACK_VERSION;
                    if race.is_some() && class.is_some() && sex.is_some() && pronouns.is_some() {
                        state = ConnState::NeedSex;
                    } else {
                        state = ConnState::NeedRace;
                        let _ = write_tx.send(prompt_race()).await;
                        continue;
                    }
                }
                ConnState::NeedRace => {
                    let line = String::from_utf8_lossy(&line_bytes)
                        .trim()
                        .to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "race list" || line == "list" {
                        let mut s = String::new();
                        s.push_str("races:\r\n");
                        for r in RACE_TOKENS {
                            s.push_str(" - ");
                            s.push_str(r);
                            s.push_str("\r\n");
                        }
                        s.push_str("> ");
                        let _ = write_tx.send(Bytes::from(s)).await;
                        continue;
                    }
                    let token = line.strip_prefix("race ").unwrap_or(line.as_str()).trim();
                    if !is_allowed_token(token, &RACE_TOKENS) {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (try: race list | race human)\r\n> ",
                            ))
                            .await;
                        continue;
                    }
                    race = Some(token.to_string());
                    state = ConnState::NeedClass;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 6/7)\r\nchoose class:\r\ntype: class list | class <name>\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedClass => {
                    let line = String::from_utf8_lossy(&line_bytes)
                        .trim()
                        .to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    if line == "class list" || line == "list" {
                        let mut s = String::new();
                        s.push_str("classes:\r\n");
                        for c in CLASS_TOKENS {
                            s.push_str(" - ");
                            s.push_str(c);
                            s.push_str("\r\n");
                        }
                        s.push_str("> ");
                        let _ = write_tx.send(Bytes::from(s)).await;
                        continue;
                    }
                    let token = line.strip_prefix("class ").unwrap_or(line.as_str()).trim();
                    if !is_allowed_token(token, &CLASS_TOKENS) {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (try: class list | class fighter)\r\n> ",
                            ))
                            .await;
                        continue;
                    }
                    class = Some(token.to_string());
                    state = ConnState::NeedSex;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 7/7)\r\nsex:\r\ntype: male | female | none | other\r\n> ",
                        ))
                        .await;
                    continue;
                }
                ConnState::NeedSex => {
                    let line = String::from_utf8_lossy(&line_bytes)
                        .trim()
                        .to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
                    }
                    match line.as_str() {
                        "male" => {
                            sex = Some("male".to_string());
                            pronouns = Some("he".to_string());
                        }
                        "female" => {
                            sex = Some("female".to_string());
                            pronouns = Some("she".to_string());
                        }
                        "none" => {
                            sex = Some("none".to_string());
                            pronouns = Some("they".to_string());
                        }
                        "other" => {
                            sex = Some("other".to_string());
                            state = ConnState::NeedPronouns;
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"pronouns (en): he | she | they\r\n(type: he)\r\n> ",
                                ))
                                .await;
                            continue;
                        }
                        _ => {
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"please type: male | female | none | other\r\n> ",
                                ))
                                .await;
                            continue;
                        }
                    }
                    // Ready to attach.
                }
                ConnState::NeedPronouns => {
                    let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
                    let Some(key) = normalize_pronouns(&cfg.locale, &line) else {
                        let _ = write_tx
                            .send(Bytes::from_static(
                                b"huh? (pronouns: he | she | they)\r\n> ",
                            ))
                            .await;
                        continue;
                    };
                    pronouns = Some(key.to_string());
                    // Ready to attach.
                }
                ConnState::InWorld => {
                    // In-world input. Some commands are handled here (connection-level).
                }
            }

            // If we just finished sex/pronouns, attach now.
            if matches!(state, ConnState::NeedSex | ConnState::NeedPronouns)
                && sex.is_some()
                && pronouns.is_some()
            {
                state = ConnState::InWorld;

                let n = name.as_ref().expect("name set").clone();
                let bot = is_bot.unwrap_or(false);
                let race_s = race.clone().unwrap_or_else(|| "human".to_string());
                let class_s = class.clone().unwrap_or_else(|| "fighter".to_string());
                let sex_s = sex.clone().unwrap_or_else(|| "none".to_string());
                let pro_s = pronouns.clone().unwrap_or_else(|| "they".to_string());
                let shard_auth = auth_blob.clone().unwrap_or_else(|| {
                    make_shard_auth_blob(
                        &n,
                        auth_method.as_deref().unwrap_or("unknown"),
                        google_sub.as_deref(),
                        google_email.as_deref(),
                        oidc_sub.as_deref(),
                        oidc_email.as_deref(),
                        None,
                    )
                });

                let held = { holds.lock().await.is_held(&n).is_some() };

                {
                    let mut a = accounts.lock().await;
                    if let Some(r) = a.by_name.get_mut(&n) {
                        let mut changed = false;
                        if persist_account_profile {
                            if r.is_bot != Some(bot)
                                || r.race.as_deref() != Some(race_s.as_str())
                                || r.class.as_deref() != Some(class_s.as_str())
                                || r.sex.as_deref() != Some(sex_s.as_str())
                                || r.pronouns.as_deref() != Some(pro_s.as_str())
                            {
                                store_account_onboarding(r, bot, &race_s, &class_s, &sex_s, &pro_s);
                                changed = true;
                            }
                        }
                        if r.public_ack_version != PUBLIC_ACK_VERSION {
                            r.public_ack_version = PUBLIC_ACK_VERSION;
                            changed = true;
                        }
                        if r.coc_ack_version != COC_ACK_VERSION {
                            r.coc_ack_version = COC_ACK_VERSION;
                            changed = true;
                        }
                        if changed {
                            a.save()?;
                        }
                    }
                }

                {
                    let mut m = sessions.lock().await;
                    m.insert(
                        session,
                        SessionInfo {
                            name: n.clone(),
                            held,
                            is_bot: bot,
                            auth: Some(shard_auth.clone()),
                            race: race_s.clone(),
                            class: class_s.clone(),
                            sex: sex_s.clone(),
                            pronouns: pro_s.clone(),
                            peer_ip,
                            write_tx: write_tx.clone(),
                            disconnect_tx: disconnect_tx.clone(),
                            scrollback: Arc::new(tokio::sync::Mutex::new(Scrollback::new(
                                SCROLLBACK_MAX_LINES,
                            ))),
                            next_cmd_id: 1,
                        },
                    );
                }

                {
                    let ts = Utc::now().to_rfc3339();
                    let sid = session_hex(session);
                    let entry = format!(
                        "ts={} kind=login session={} ip={} name={} bot={} auth_method={}",
                        logfmt_str(&ts),
                        logfmt_str(&sid),
                        logfmt_str(&peer_ip.to_string()),
                        logfmt_str(&n),
                        logfmt_str(&(if bot { "1" } else { "0" }).to_string()),
                        logfmt_str(auth_method.as_deref().unwrap_or("unknown")),
                    );
                    eventlog.log_line(LogStream::All, &entry).await;
                    eventlog.log_line(LogStream::Character(&n), &entry).await;
                    eventlog.log_line(LogStream::Login, &entry).await;
                }

                let body = attach_body(
                    bot,
                    false,
                    Some(shard_auth.as_ref()),
                    &race_s,
                    &class_s,
                    &sex_s,
                    &pro_s,
                    n.as_bytes(),
                );
                let _ = shard_tx
                    .send(ShardMsg {
                        t: REQ_ATTACH,
                        session,
                        body,
                    })
                    .await;
                continue;
            }

            // In-world command handling at the broker level.
            let line = String::from_utf8_lossy(&line_bytes).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let lc = line.to_ascii_lowercase();

            {
                let Some(nm) = name.as_deref() else {
                    continue;
                };
                let line_for_log = redact_input_for_logs(&line);
                let ts = Utc::now().to_rfc3339();
                let sid = session_hex(session);
                let entry = format!(
                    "ts={} kind=input session={} ip={} name={} text={}",
                    logfmt_str(&ts),
                    logfmt_str(&sid),
                    logfmt_str(&peer_ip.to_string()),
                    logfmt_str(nm),
                    logfmt_str(line_for_log.as_ref()),
                );
                eventlog.log_line(LogStream::All, &entry).await;
                eventlog.log_line(LogStream::Character(nm), &entry).await;
            }

            if let Some(blob_len) = parse_sayblob_len(&lc) {
                let len = match blob_len {
                    Ok(v) => v,
                    Err(msg) => {
                        let _ = write_tx.send(Bytes::from(format!("{msg}\r\n> "))).await;
                        continue;
                    }
                };
                let _ = write_tx
                    .send(Bytes::from(format!(
                        "# send {len} raw bytes for sayblob now\r\n"
                    )))
                    .await;
                match spool_blob_payload(
                    &mut rd,
                    &mut iac,
                    &mut linebuf,
                    &cfg.blob_spool_dir,
                    session,
                    len,
                )
                .await
                {
                    Ok(path) => {
                        let path_text = path.to_string_lossy().to_string();
                        match mudproto::shard::build_input_blob_body(
                            b"say",
                            path_text.as_bytes(),
                            len,
                        ) {
                            Ok(body) => {
                                let _ = shard_tx
                                    .send(ShardMsg {
                                        t: REQ_INPUT_BLOB,
                                        session,
                                        body: Bytes::from(body),
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = write_tx
                                    .send(Bytes::from(format!("sayblob: {e}\r\n> ")))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = write_tx
                            .send(Bytes::from(format!("sayblob: {e}\r\n> ")))
                            .await;
                    }
                }
                continue;
            }

            if lc == "exit" || lc == "quit" {
                let _ = write_tx.send(Bytes::from_static(b"bye\r\n")).await;
                break 'read;
            }

            if lc == "uptime" || lc == "uptime broker" || lc == "uptime session" {
                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let up_s = server_info.started_instant.elapsed().as_secs();

                let mut s = String::new();
                s.push_str("uptime:\r\n");
                s.push_str(&format!(" - broker_wall_unix: {now_unix}\r\n"));
                s.push_str(&format!(
                    " - broker_started_unix: {}\r\n",
                    server_info.started_unix
                ));
                s.push_str(&format!(" - broker_uptime_s: {up_s}\r\n"));
                s.push_str(&format!(" - broker_bind: {}\r\n", server_info.bind));
                s.push_str(&format!(" - shard_addr: {}\r\n", server_info.shard_addr));
                if server_info.shard_addrs.len() > 1 {
                    let addrs = server_info
                        .shard_addrs
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    s.push_str(&format!(" - shard_addrs: {addrs}\r\n"));
                }
                s.push_str(" - note: shard uptime/time via `uptime` (forwarded to shard)\r\n");
                let _ = write_tx.send(Bytes::from(s)).await;

                if lc != "uptime" {
                    continue;
                }
                // `uptime` (no args) also forwards to shard so the user can see shard wall time + world time.
            }

            if lc == "report" || lc.starts_with("report ") {
                let nm = name.as_deref().unwrap_or("");
                let out = handle_report_command(
                    &sessions, &holds, &nearline, &eventlog, session, peer_ip, nm, &line,
                )
                .await;
                let _ = write_tx.send(Bytes::from(out)).await;
                continue;
            }

            if lc == "accounthold" || lc.starts_with("accounthold ") {
                let nm = name.as_deref().unwrap_or("");
                let out = handle_accounthold_command(
                    &sessions,
                    &holds,
                    &cfg.sbc_admin_sock,
                    &eventlog,
                    peer_ip,
                    session,
                    nm,
                    &line,
                )
                .await;
                let _ = write_tx.send(Bytes::from(out)).await;
                continue;
            }

            if lc == "account" || lc.starts_with("account ") {
                let nm = name.as_deref().unwrap_or("");
                let out = handle_account_command(&accounts, nm, &line).await;
                let _ = write_tx.send(Bytes::from(out)).await;
                continue;
            }

            if lc == "whoami" {
                let out = handle_whoami_command(&sessions, session).await;
                let _ = write_tx.send(Bytes::from(out)).await;
                continue;
            }

            if lc == "who" {
                let out = handle_who_command(&sessions).await;
                let _ = write_tx.send(Bytes::from(out)).await;
                continue;
            }

            let cmd_id = {
                let mut m = sessions.lock().await;
                if let Some(si) = m.get_mut(&session) {
                    let id = si.next_cmd_id;
                    si.next_cmd_id = si.next_cmd_id.saturating_add(1).max(1);
                    id
                } else {
                    0
                }
            };
            let (t, body) = if cmd_id == 0 {
                (REQ_INPUT, Bytes::from(line.into_bytes()))
            } else {
                (
                    REQ_INPUT_IDEMPOTENT,
                    Bytes::from(build_input_idempotent_body(cmd_id, line.as_bytes())),
                )
            };
            let _ = shard_tx.send(ShardMsg { t, session, body }).await;
        }
    }

    // Best-effort: if we disconnected mid-password, restore echo.
    if password_echo_disabled {
        let _ = write_tx
            .send(Bytes::from(telnet_wont(TELNET_OPT_ECHO).to_vec()))
            .await;
    }

    // Disconnect cleanup.
    let removed = { sessions.lock().await.remove(&session) };
    if let Some(si) = removed {
        {
            let ts = Utc::now().to_rfc3339();
            let sid = session_hex(session);
            let entry = format!(
                "ts={} kind=logout session={} ip={} name={}",
                logfmt_str(&ts),
                logfmt_str(&sid),
                logfmt_str(&si.peer_ip.to_string()),
                logfmt_str(&si.name),
            );
            eventlog.log_line(LogStream::All, &entry).await;
            eventlog
                .log_line(LogStream::Character(&si.name), &entry)
                .await;
            eventlog.log_line(LogStream::Login, &entry).await;
        }

        let _ = shard_tx
            .send(ShardMsg {
                t: REQ_DETACH,
                session,
                body: Bytes::new(),
            })
            .await;
    } else {
        info!(peer=%peer, "disconnected before entering world");
    }

    drop(write_tx);
    let _ = writer.await;
    Ok(())
}

const TELNET_IAC: u8 = 255;
const TELNET_WILL: u8 = 251;
const TELNET_WONT: u8 = 252;
const TELNET_OPT_ECHO: u8 = 1;

fn telnet_will(opt: u8) -> [u8; 3] {
    [TELNET_IAC, TELNET_WILL, opt]
}

fn telnet_wont(opt: u8) -> [u8; 3] {
    [TELNET_IAC, TELNET_WONT, opt]
}

fn parse_proxy_line_v1(line: &str) -> Option<(IpAddr, u16)> {
    // Minimal PROXY protocol v1 parser:
    //   PROXY TCP4 203.0.113.1 192.0.2.10 12345 23\r\n
    // We only trust this when the TCP peer is loopback.
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 6 {
        return None;
    }
    if parts[0] != "PROXY" {
        return None;
    }
    let proto = parts[1];
    if proto != "TCP4" && proto != "TCP6" {
        return None;
    }
    let src_ip: IpAddr = parts[2].parse().ok()?;
    if proto == "TCP4" && !src_ip.is_ipv4() {
        return None;
    }
    if proto == "TCP6" && !src_ip.is_ipv6() {
        return None;
    }
    let src_port: u16 = parts[4].parse().ok()?;
    Some((src_ip, src_port))
}

fn trim_ascii_ws(s: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = s.len();
    while a < b && s[a].is_ascii_whitespace() {
        a += 1;
    }
    while b > a && s[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    &s[a..b]
}

fn redact_pii(s: &str) -> String {
    let s = redact_emails(s);
    redact_phones(&s)
}

fn redact_emails(s: &str) -> String {
    fn is_user(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
    }
    fn is_domain(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
    }

    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0usize;

    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'@' {
            i += 1;
            continue;
        }

        let mut l = i;
        while l > 0 && is_user(b[l - 1]) {
            l -= 1;
        }
        let user_len = i.saturating_sub(l);

        let mut r = i.saturating_add(1);
        while r < b.len() && is_domain(b[r]) {
            r += 1;
        }
        let dom_len = r.saturating_sub(i.saturating_add(1));

        if user_len == 0 || dom_len == 0 {
            i += 1;
            continue;
        }

        // ASCII-only slices are safe by byte offsets.
        let user = &s[l..i];
        let domain = &s[i + 1..r];

        // Basic validation to reduce false positives.
        if user.starts_with('.') || user.ends_with('.') || user.contains("..") {
            i += 1;
            continue;
        }
        if !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
            i += 1;
            continue;
        }
        if domain.contains("..") {
            i += 1;
            continue;
        }

        let mut ok = true;
        for label in domain.split('.') {
            if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
                ok = false;
                break;
            }
        }
        if domain.split('.').last().unwrap_or("").len() < 2 {
            ok = false;
        }
        if !ok {
            i += 1;
            continue;
        }

        out.push_str(&s[last..l]);
        out.push_str("[email]");
        last = r;
        i = r;
    }

    if last == 0 {
        return s.to_string();
    }
    out.push_str(&s[last..]);
    out
}

fn redact_phones(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0usize;

    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii_digit() && c != b'+' {
            i += 1;
            continue;
        }

        let start = i;
        let mut j = i;
        let mut digits = 0usize;
        let mut last_digit_end = start;
        while j < b.len() {
            let cj = b[j];
            if cj.is_ascii_digit() {
                digits += 1;
                j += 1;
                last_digit_end = j;
                continue;
            }
            if matches!(cj, b' ' | b'-' | b'.' | b'(' | b')') {
                j += 1;
                continue;
            }
            if cj == b'+' && j == start {
                j += 1;
                continue;
            }
            break;
        }

        if digits >= 10 {
            let end = last_digit_end;
            out.push_str(&s[last..start]);
            out.push_str("[phone]");
            last = end;
            i = end;
            continue;
        }

        i = j.max(i.saturating_add(1));
    }

    if last == 0 {
        return s.to_string();
    }
    out.push_str(&s[last..]);
    out
}

fn sanitize_name(s: &str) -> String {
    let s = s.trim();
    let mut out = String::new();
    for c in s.chars() {
        if out.len() >= 20 {
            break;
        }
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        }
    }
    out
}

fn normalize_email(s: &str) -> Option<String> {
    // "Reasonable" email validation, not full RFC parsing. We keep this strict enough to avoid
    // obvious garbage and avoid accepting local-only domains.
    let s = s.trim();
    if s.is_empty() || s.len() > 254 {
        return None;
    }
    if !s.is_ascii() {
        return None;
    }
    if s.chars()
        .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
    {
        return None;
    }

    let mut parts = s.split('@');
    let local = parts.next()?;
    let domain = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if local.is_empty() || domain.is_empty() {
        return None;
    }

    // Common sanity checks.
    if local.len() > 64 {
        return None;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return None;
    }
    const LOCAL_EXTRA: &str = ".!#$%&'*+/=?^_`{|}~-";
    if !local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || LOCAL_EXTRA.contains(c))
    {
        return None;
    }

    // Require a "real-ish" domain, not localhost.
    if domain.len() > 253 || !domain.contains('.') {
        return None;
    }
    if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return None;
    }
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return None;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
    }

    Some(format!("{local}@{domain}").to_ascii_lowercase())
}

fn try_pop_line(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    // Telnet tends to send CRLF, but can also send CRNUL.
    // Treat `\n` and `\r` as EOL; if `\r` is followed by `\n` or `\0`, consume both.
    let i_nl = memchr(b'\n', buf.as_slice());
    let i_cr = memchr(b'\r', buf.as_slice());

    let i = match (i_nl, i_cr) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }?;

    let mut line = buf.drain(0..i).collect::<Vec<u8>>();
    // Drain EOL bytes.
    if !buf.is_empty() {
        let first = buf.remove(0);
        if first == b'\r' && !buf.is_empty() && (buf[0] == b'\n' || buf[0] == 0) {
            buf.remove(0);
        }
    }

    // Trim any stray trailing \r (if we matched \n first and had \r\n).
    while line.last() == Some(&b'\r') {
        line.pop();
    }

    Some(line)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};

    use super::{
        AccountRec, COC_ACK_VERSION, ConnState, LineId, PUBLIC_ACK_VERSION, REQ_DETACH, REQ_INPUT,
        REQ_INPUT_IDEMPOTENT, Scrollback, ShardMsg, ack_inflight_for_response,
        build_input_idempotent_body, extract_scrollback_lines, normalize_email, parse_sayblob_len,
        parse_shard_addrs, prepare_existing_account_onboarding, redact_pii, requeue_inflight,
        seed_account_onboarding, shard_msg_expects_response, store_account_onboarding,
        stored_account_onboarding, trim_ascii_ws,
    };
    use bytes::Bytes;
    use mudproto::session::SessionId;
    use mudproto::shard::ShardResp;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum BrokerStateClass {
        TransportLocal,
        DerivedLocal,
        RaftRequiredDebt,
    }

    struct BrokerStateField {
        owner: &'static str,
        field: &'static str,
        class: BrokerStateClass,
        reason: &'static str,
    }

    static BROKER_STATE_MANIFEST: &[BrokerStateField] = &[
        bf(
            "SessionInfo",
            "name",
            BrokerStateClass::RaftRequiredDebt,
            "character identity is resumable across broker reconnects",
        ),
        bf(
            "SessionInfo",
            "held",
            BrokerStateClass::DerivedLocal,
            "legal hold status is derived from the compliance cache",
        ),
        bf(
            "SessionInfo",
            "is_bot",
            BrokerStateClass::RaftRequiredDebt,
            "bot/player role affects resumable shard attach state",
        ),
        bf(
            "SessionInfo",
            "auth",
            BrokerStateClass::RaftRequiredDebt,
            "auth assertion is needed when the shard connection is rebuilt",
        ),
        bf(
            "SessionInfo",
            "race",
            BrokerStateClass::RaftRequiredDebt,
            "character build state must resume through raft state",
        ),
        bf(
            "SessionInfo",
            "class",
            BrokerStateClass::RaftRequiredDebt,
            "character build state must resume through raft state",
        ),
        bf(
            "SessionInfo",
            "sex",
            BrokerStateClass::RaftRequiredDebt,
            "character profile state must resume through raft state",
        ),
        bf(
            "SessionInfo",
            "pronouns",
            BrokerStateClass::RaftRequiredDebt,
            "character profile state must resume through raft state",
        ),
        bf(
            "SessionInfo",
            "peer_ip",
            BrokerStateClass::TransportLocal,
            "current TCP peer is connection-local",
        ),
        bf(
            "SessionInfo",
            "write_tx",
            BrokerStateClass::TransportLocal,
            "socket writer channel is connection-local",
        ),
        bf(
            "SessionInfo",
            "disconnect_tx",
            BrokerStateClass::TransportLocal,
            "disconnect signal is connection-local",
        ),
        bf(
            "SessionInfo",
            "scrollback",
            BrokerStateClass::DerivedLocal,
            "scrollback is mirrored into nearline/event logs, not raft consensus state",
        ),
        bf(
            "SessionInfo",
            "next_cmd_id",
            BrokerStateClass::TransportLocal,
            "per-session input id generator is only used while this transport session is live; ids are carried on in-flight shard messages",
        ),
    ];

    const fn bf(
        owner: &'static str,
        field: &'static str,
        class: BrokerStateClass,
        reason: &'static str,
    ) -> BrokerStateField {
        BrokerStateField {
            owner,
            field,
            class,
            reason,
        }
    }

    fn struct_field_names(source: &str, owner: &str) -> BTreeSet<String> {
        let needle = format!("struct {owner} {{");
        let Some(start) = source.find(&needle) else {
            panic!("missing struct {owner}");
        };
        let body_start = start + needle.len();
        let mut depth = 1i32;
        let mut end = body_start;
        for (off, ch) in source[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = body_start + off;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(end > body_start, "could not parse struct {owner}");
        source[body_start..end]
            .lines()
            .filter_map(|line| {
                let line = line.split("//").next().unwrap_or("").trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let (name, _) = line.split_once(':')?;
                let name = name.trim().trim_start_matches("pub ").trim();
                if name.is_empty() {
                    return None;
                }
                Some(name.to_string())
            })
            .collect()
    }

    #[test]
    fn broker_session_manifest_covers_all_session_info_fields() {
        let source = include_str!("main.rs");
        let parsed = struct_field_names(source, "SessionInfo");
        let declared = BROKER_STATE_MANIFEST
            .iter()
            .filter(|f| f.owner == "SessionInfo")
            .map(|f| f.field.to_string())
            .collect::<BTreeSet<_>>();
        let missing = parsed.difference(&declared).cloned().collect::<Vec<_>>();
        let stale = declared.difference(&parsed).cloned().collect::<Vec<_>>();
        assert!(
            missing.is_empty() && stale.is_empty(),
            "broker state manifest mismatch for SessionInfo: missing={missing:?} stale={stale:?}"
        );
    }

    #[test]
    fn broker_resumable_state_is_marked_for_raft() {
        let raft_required = BROKER_STATE_MANIFEST
            .iter()
            .filter(|f| f.class == BrokerStateClass::RaftRequiredDebt)
            .collect::<Vec<_>>();
        assert!(
            !raft_required.is_empty(),
            "broker resumable state must be explicitly tracked as raft migration debt"
        );
        for field in BROKER_STATE_MANIFEST {
            assert!(
                !field.reason.trim().is_empty(),
                "broker field {}.{} needs a state-boundary reason",
                field.owner,
                field.field
            );
        }
    }

    fn bare_test_account(name: &str) -> AccountRec {
        AccountRec {
            name: name.to_string(),
            pw_hash: None,
            auth_identities: Vec::new(),
            google_sub: None,
            google_email: None,
            oidc_sub: None,
            oidc_email: None,
            caps: None,
            is_bot: None,
            race: None,
            class: None,
            sex: None,
            pronouns: None,
            public_ack_version: 0,
            coc_ack_version: 0,
            email: None,
            created_unix: 1,
        }
    }

    #[test]
    fn legacy_account_defaults_skip_current_rules_but_not_profile_persistence() {
        let rec: AccountRec = serde_json::from_str(r#"{"name":"rob","created_unix":1}"#).unwrap();
        assert_eq!(rec.public_ack_version, PUBLIC_ACK_VERSION);
        assert_eq!(rec.coc_ack_version, COC_ACK_VERSION);

        let mut is_bot = None;
        let mut race = None;
        let mut class = None;
        let mut sex = None;
        let mut pronouns = None;
        assert!(!seed_account_onboarding(
            &rec,
            &mut is_bot,
            &mut race,
            &mut class,
            &mut sex,
            &mut pronouns
        ));
        assert_eq!(is_bot, Some(false));
        assert_eq!(race.as_deref(), Some("human"));
        assert_eq!(class.as_deref(), Some("fighter"));
        assert_eq!(sex.as_deref(), Some("none"));
        assert_eq!(pronouns.as_deref(), Some("they"));
    }

    #[test]
    fn existing_account_onboarding_routes_by_ack_version() {
        let mut rec = bare_test_account("alice");
        store_account_onboarding(&mut rec, false, "elf", "wizard", "female", "she");
        rec.public_ack_version = PUBLIC_ACK_VERSION;
        rec.coc_ack_version = COC_ACK_VERSION;

        let mut is_bot = None;
        let mut race = None;
        let mut class = None;
        let mut sex = None;
        let mut pronouns = None;
        let mut public_ack = 0;
        let mut coc_ack = 0;
        let mut persist_profile = true;

        let route = prepare_existing_account_onboarding(
            &rec,
            &mut is_bot,
            &mut race,
            &mut class,
            &mut sex,
            &mut pronouns,
            &mut public_ack,
            &mut coc_ack,
            &mut persist_profile,
        );
        assert_eq!(route, ConnState::NeedSex);
        assert!(persist_profile);
        assert_eq!(
            stored_account_onboarding(&rec),
            Some((
                false,
                "elf".to_string(),
                "wizard".to_string(),
                "female".to_string(),
                "she".to_string()
            ))
        );

        rec.public_ack_version = PUBLIC_ACK_VERSION.saturating_sub(1);
        let route = prepare_existing_account_onboarding(
            &rec,
            &mut is_bot,
            &mut race,
            &mut class,
            &mut sex,
            &mut pronouns,
            &mut public_ack,
            &mut coc_ack,
            &mut persist_profile,
        );
        assert_eq!(route, ConnState::NeedPublicAck);
    }

    #[test]
    fn trim_ascii_ws_basic() {
        assert_eq!(trim_ascii_ws(b""), b"");
        assert_eq!(trim_ascii_ws(b"  x "), b"x");
        assert_eq!(trim_ascii_ws(b"\r\nx\t"), b"x");
        assert_eq!(trim_ascii_ws(b"   "), b"");
    }

    #[test]
    fn normalize_email_basic() {
        assert_eq!(
            normalize_email("Alice+ok@Example.com"),
            Some("alice+ok@example.com".to_string())
        );

        assert!(normalize_email("").is_none());
        assert!(normalize_email("no-at").is_none());
        assert!(normalize_email("a@b").is_none()); // require dot
        assert!(normalize_email("a@b..c.com").is_none());
        assert!(normalize_email(".a@example.com").is_none());
        assert!(normalize_email("a.@example.com").is_none());
        assert!(normalize_email("a@-example.com").is_none());
        assert!(normalize_email("a@example-.com").is_none());
        assert!(normalize_email("a@exa_mple.com").is_none());
        assert!(normalize_email("a b@example.com").is_none());
    }

    #[test]
    fn scrollback_splits_and_skips_prompt() {
        let lines = extract_scrollback_lines(b"hello\r\n> \r\nworld\n\n");
        assert_eq!(lines, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn scrollback_search_case_insensitive_newest_first() {
        let mut sb = Scrollback::new(10);
        sb.push_line(LineId(1), 1, "alpha".to_string());
        sb.push_line(LineId(2), 2, "Beta".to_string());
        sb.push_line(LineId(3), 3, "ALPHA again".to_string());

        let hits = sb.search("alpha", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text, "ALPHA again");
        assert_eq!(hits[1].text, "alpha");
    }

    #[test]
    fn scrollback_context_includes_neighbors() {
        let mut sb = Scrollback::new(10);
        sb.push_line(LineId(1), 1, "one".to_string());
        sb.push_line(LineId(2), 2, "two".to_string());
        sb.push_line(LineId(3), 3, "three".to_string());

        let (target, ctx) = sb.find_with_context(LineId(2), 1).expect("context");
        assert_eq!(target.text, "two");
        assert_eq!(ctx.len(), 3);
        assert_eq!(ctx[0].text, "one");
        assert_eq!(ctx[1].text, "two");
        assert_eq!(ctx[2].text, "three");
    }

    #[test]
    fn redact_pii_emails_and_phones() {
        assert_eq!(
            redact_pii("email alice@example.com ok"),
            "email [email] ok".to_string()
        );
        assert_eq!(
            redact_pii("call +1 (770) 235-3571 now"),
            "call [phone] now".to_string()
        );
        assert_eq!(redact_pii("no pii here"), "no pii here".to_string());
    }

    #[test]
    fn sayblob_len_parser_is_strict() {
        assert_eq!(parse_sayblob_len("look"), None);
        assert_eq!(parse_sayblob_len("sayblob 1024"), Some(Ok(1024)));
        assert!(matches!(parse_sayblob_len("sayblob"), Some(Err(_))));
        assert!(matches!(parse_sayblob_len("sayblob 0"), Some(Err(_))));
        assert!(matches!(parse_sayblob_len("sayblob nope"), Some(Err(_))));
        assert!(matches!(
            parse_sayblob_len("sayblob 10 extra"),
            Some(Err(_))
        ));
    }

    #[test]
    fn shard_addr_parser_preserves_dns_targets() {
        assert_eq!(
            parse_shard_addrs(
                "dev-raft-n0.slopmud.com:5000",
                Some(
                    "dev-raft-n0.slopmud.com:5000, dev-raft-n1.slopmud.com:5000,dev-raft-n2.slopmud.com:5000"
                )
            ),
            vec![
                "dev-raft-n0.slopmud.com:5000".to_string(),
                "dev-raft-n1.slopmud.com:5000".to_string(),
                "dev-raft-n2.slopmud.com:5000".to_string(),
            ]
        );
        assert_eq!(
            parse_shard_addrs("127.0.0.1:5000", Some("127.0.0.1:5000,127.0.0.1:5000")),
            vec!["127.0.0.1:5000".to_string()]
        );
        assert_eq!(
            parse_shard_addrs("dev-raft-n0.slopmud.com:5000", None),
            vec!["dev-raft-n0.slopmud.com:5000".to_string()]
        );
    }

    #[test]
    fn shard_failover_requeues_only_unacked_requests() {
        let alice = SessionId(1);
        let bob = SessionId(2);
        let req_a = ShardMsg {
            t: REQ_INPUT_IDEMPOTENT,
            session: alice,
            body: Bytes::from(build_input_idempotent_body(10, b"quest get trio.probe")),
        };
        let req_b = ShardMsg {
            t: REQ_INPUT,
            session: bob,
            body: Bytes::from_static(b"look"),
        };
        let req_c = ShardMsg {
            t: REQ_INPUT,
            session: alice,
            body: Bytes::from_static(b"quest get trio.step"),
        };
        let detach = ShardMsg {
            t: REQ_DETACH,
            session: alice,
            body: Bytes::new(),
        };

        assert!(shard_msg_expects_response(&req_a));
        assert!(!shard_msg_expects_response(&detach));

        let mut pending = VecDeque::new();
        pending.push_back(req_c.clone());
        let mut inflight = VecDeque::new();
        inflight.push_back(req_a.clone());
        inflight.push_back(req_b.clone());

        ack_inflight_for_response(
            &mut inflight,
            &ShardResp::Output {
                session: alice,
                line: Bytes::from_static(b"quest: trio.probe=alive\r\n"),
            },
        );
        assert_eq!(inflight.len(), 1);
        assert_eq!(inflight[0].session, bob);

        requeue_inflight(&mut pending, &mut inflight);
        assert!(inflight.is_empty());
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].session, bob);
        assert_eq!(pending[1].body, req_c.body);
        assert_eq!(pending[0].body, req_b.body);
    }
}
