use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use argon2::Argon2;
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use compliance::LogStream;
use memchr::memchr;
use mudproto::session::SessionId;
use mudproto::shard::{REQ_ATTACH, REQ_DETACH, REQ_INPUT, ShardResp};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use slopio::frame::{FrameReader, FrameWriter};
use slopio::telnet::IacParser;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tracing::{Level, info, warn};
use zeroize::Zeroize;

mod ban;
mod email;
mod eventlog;
mod hold;
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
    shard_addr: SocketAddr,
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
ENV:\n  SLOPMUD_BIND               default 0.0.0.0:4000\n  SHARD_ADDR                 default 127.0.0.1:5000\n  NODE_ID                    optional (for logs only)\n  SLOPMUD_ACCOUNTS_PATH       optional; default accounts.json (in WorkingDirectory)\n  SLOPMUD_LOCALE              optional; default en\n  SLOPMUD_ADMIN_BIND          optional; default 127.0.0.1:4011 (local admin JSON)\n  SLOPMUD_BANS_PATH           optional; default locks/bans.json\n  SBC_ADMIN_SOCK              optional; default /run/slopmud/sbc-admin.sock\n  SBC_EVENTS_SOCK             optional; default /run/slopmud/sbc-events.sock\n  SLOPMUD_EMAIL_MODE          optional; default disabled (disabled | ses | smtp | file)\n  SLOPMUD_EMAIL_FROM          required for ses/smtp; optional for file\n  SLOPMUD_SMTP_HOST           required for smtp\n  SLOPMUD_SMTP_PORT           optional; default 587\n  SLOPMUD_SMTP_USERNAME       optional\n  SLOPMUD_SMTP_PASSWORD       optional\n  SLOPMUD_EMAIL_FILE_DIR      optional; default /tmp/slopmud_email_outbox\n  SLOPMUD_EVENTLOG_ENABLED    optional; default 0\n  SLOPMUD_EVENTLOG_SPOOL_DIR  optional; default locks/eventlog\n  SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S optional; default 60\n  SLOPMUD_EVENTLOG_S3_BUCKET  optional; if set, uploads target this bucket\n  SLOPMUD_EVENTLOG_S3_PREFIX  optional; default slopmud/eventlog\n  SLOPMUD_EVENTLOG_UPLOAD_ENABLED optional; default 0\n  SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL optional; default 1\n  SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S optional; default 600\n  SLOPMUD_NEARLINE_ENABLED    optional; default 1\n  SLOPMUD_NEARLINE_DIR        optional; default locks/nearline_scrollback\n  SLOPMUD_NEARLINE_MAX_SEGMENTS optional; default 12\n  SLOPMUD_NEARLINE_SEGMENT_MAX_BYTES optional; default 2000000\n  SLOPMUD_GOOGLE_OAUTH_DIR    optional; default locks/google_oauth (shared with static_web)\n  SLOPMUD_GOOGLE_AUTH_BASE_URL optional; default http://127.0.0.1:8080 (where to open OAuth in browser)\n  SLOPMUD_OIDC_TOKEN_URL      optional; if set, mint a session token at login\n  SLOPMUD_OIDC_CLIENT_ID      required if token url set\n  SLOPMUD_OIDC_CLIENT_SECRET  required if token url set\n  SLOPMUD_OIDC_SCOPE          optional; default slopmud:session\n"
    );
    std::process::exit(2);
}

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    shard_addr: SocketAddr,
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
    locale: String,

    admin_bind: SocketAddr,
    bans_path: PathBuf,
    sbc_admin_sock: PathBuf,
    sbc_events_sock: PathBuf,
    #[allow(dead_code)]
    email: email::EmailConfig,
    eventlog: eventlog::EventLogConfig,
    nearline: nearline::NearlineConfig,
}

fn parse_args() -> Config {
    let mut bind: SocketAddr = std::env::var("SLOPMUD_BIND")
        .unwrap_or_else(|_| "0.0.0.0:4000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

    let mut shard_addr: SocketAddr = std::env::var("SHARD_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5000".to_string())
        .parse()
        .unwrap_or_else(|_| usage_and_exit());

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

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                bind = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "--shard-addr" => {
                let v = it.next().unwrap_or_else(|| usage_and_exit());
                shard_addr = v.parse().unwrap_or_else(|_| usage_and_exit());
            }
            "-h" | "--help" => usage_and_exit(),
            _ => usage_and_exit(),
        }
    }

    Config {
        bind,
        shard_addr,
        node_id,
        accounts_path,
        google_oauth_dir,
        google_auth_base_url,
        oidc_token_url,
        oidc_client_id,
        oidc_client_secret,
        oidc_scope,
        locale,
        admin_bind,
        bans_path,
        sbc_admin_sock,
        sbc_events_sock,
        email,
        eventlog,
        nearline,
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
                            Some(r) => (r.email.clone(), r.google_email.clone()),
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
    write_tx: tokio::sync::mpsc::Sender<Bytes>,
    disconnect_tx: tokio::sync::watch::Sender<bool>,
    scrollback: Arc<tokio::sync::Mutex<Scrollback>>,
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
struct AccountRec {
    name: String,
    #[serde(default)]
    pw_hash: Option<String>,
    #[serde(default)]
    google_sub: Option<String>,
    #[serde(default)]
    google_email: Option<String>,
    #[serde(default)]
    oidc_sub: Option<String>,
    #[serde(default)]
    oidc_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    caps: Option<Vec<String>>,
    // User-configured email address for notifications. Not used for auth.
    #[serde(default)]
    email: Option<String>,
    created_unix: u64,
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

#[derive(Debug, Clone, Deserialize)]
struct WebAuthReq {
    action: String, // login | create | auto
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
                for a in v {
                    by_name.insert(a.name.clone(), a);
                }
            }
        }
        Self { path, by_name }
    }

    fn save(&self) -> anyhow::Result<()> {
        let mut v = self.by_name.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        let s = serde_json::to_string_pretty(&v)?;
        let tmp = format!("{}.tmp", self.path);
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
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
        shard_addr: cfg.shard_addr,
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
        cfg.shard_addr,
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

    tokio::spawn(sbc_holds_events_task(
        cfg.sbc_events_sock.clone(),
        holds.clone(),
        sessions.clone(),
    ));

    info!(
        bind = %cfg.bind,
        shard_addr = %cfg.shard_addr,
        node_id = %cfg.node_id.as_deref().unwrap_or("-"),
        admin_bind = %cfg.admin_bind,
        bans_path = %cfg.bans_path.display(),
        sbc_admin_sock = %cfg.sbc_admin_sock.display(),
        sbc_events_sock = %cfg.sbc_events_sock.display(),
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
    shard_addr: SocketAddr,
    sessions: Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    line_ids: Arc<tokio::sync::Mutex<LineIdGen>>,
    nearline: Arc<nearline::NearlineRing>,
    eventlog: Arc<eventlog::EventLog>,
    mut rx: tokio::sync::mpsc::Receiver<ShardMsg>,
) {
    let mut announced_down = false;

    loop {
        match TcpStream::connect(shard_addr).await {
            Ok(stream) => {
                announced_down = false;
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
                for (sid, is_bot, auth, race, class, sex, pronouns, name) in snapshot {
                    let body = attach_body(
                        is_bot,
                        auth.as_deref(),
                        &race,
                        &class,
                        &sex,
                        &pronouns,
                        name.as_bytes(),
                    );
                    let _ = write_req(&mut fw, REQ_ATTACH, sid, &body).await;
                }
                let _ = fw.flush().await;

                // Connection loop.
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            let Some(msg) = msg else {
                                return;
                            };
                            let _ = write_req(&mut fw, msg.t, msg.session, &msg.body).await;
                        }
                        res = fr.read_frame() => {
                            let frame = match res {
                                Ok(Some(f)) => f,
                                Ok(None) => break,
                                Err(_) => break,
                            };
                            match mudproto::shard::parse_resp(frame) {
                                Ok(resp) => {
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
                warn!(shard_addr = %shard_addr, "shard disconnected; reconnecting");
                notify_all(&sessions, b"# shard disconnected; reconnecting...\r\n").await;
            }
            Err(e) => {
                if !announced_down {
                    announced_down = true;
                    warn!(shard_addr = %shard_addr, err=%e, "shard offline; retrying");
                    notify_all(&sessions, b"# shard offline; retrying...\r\n").await;
                }

                // Don't let the outbound queue grow unbounded while offline.
                while let Ok(msg) = rx.try_recv() {
                    if msg.t == REQ_INPUT {
                        notify_one(
                            &sessions,
                            msg.session,
                            b"# shard offline; input dropped\r\n",
                        )
                        .await;
                    }
                }

                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
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
                                google_sub: None,
                                google_email: None,
                                oidc_sub: None,
                                oidc_email: None,
                                caps,
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

async fn notify_all(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    msg: &'static [u8],
) {
    let txs = {
        let m = sessions.lock().await;
        m.values().map(|s| s.write_tx.clone()).collect::<Vec<_>>()
    };
    for tx in txs {
        let _ = tx.send(Bytes::from_static(msg)).await;
    }
}

async fn notify_one(
    sessions: &Arc<tokio::sync::Mutex<HashMap<SessionId, SessionInfo>>>,
    session: SessionId,
    msg: &'static [u8],
) {
    let tx = {
        let m = sessions.lock().await;
        m.get(&session).map(|s| s.write_tx.clone())
    };
    if let Some(tx) = tx {
        let _ = tx.send(Bytes::from_static(msg)).await;
    }
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
    let (mut rd, mut wr) = stream.into_split();

    let (disconnect_tx, mut disconnect_rx) = tokio::sync::watch::channel(false);

    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<Bytes>(128);
    let writer = tokio::spawn(async move {
        while let Some(b) = write_rx.recv().await {
            if wr.write_all(&b[..]).await.is_err() {
                break;
            }
        }
    });

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
                        if let Some(rest) = line.strip_prefix("WEB_AUTH ") {
                            let req: WebAuthReq = match serde_json::from_str(rest) {
                                Ok(v) => v,
                                Err(_) => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(b"web_auth: bad json\r\nname: "))
                                        .await;
                                    continue;
                                }
                            };

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
                                    a.by_name
                                        .values()
                                        .filter_map(|r| {
                                            let linked = if method == "google" {
                                                r.google_sub.as_deref() == Some(sub)
                                            } else {
                                                r.oidc_sub.as_deref() == Some(sub)
                                            };
                                            if linked { Some(r.name.clone()) } else { None }
                                        })
                                        .collect::<Vec<_>>()
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
                                                    google_sub: None,
                                                    google_email: None,
                                                    oidc_sub: None,
                                                    oidc_email: None,
                                                    caps: None,
                                                    email: None,
                                                    created_unix: now_unix,
                                                },
                                            );
                                            a.save()?;
                                        }

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
                                ("create", "google") | ("login", "google") | ("auto", "google") => {
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

                                        if action == "login" || action == "auto" {
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
                                                    if r.google_sub.as_deref() != Some(sub) {
                                                        let _ = write_tx
                                                            .send(Bytes::from_static(
                                                                b"web_auth: account not linked to google\r\nname: ",
                                                            ))
                                                            .await;
                                                        false
                                                    } else {
                                                        google_sub = Some(sub.to_string());
                                                        google_email = r
                                                            .google_email
                                                            .clone()
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
                                            {
                                                let mut a = accounts.lock().await;
                                                a.by_name.insert(
                                                    uname.clone(),
                                                    AccountRec {
                                                        name: uname.clone(),
                                                        pw_hash: None,
                                                        google_sub: Some(sub.to_string()),
                                                        google_email: email.clone(),
                                                        oidc_sub: None,
                                                        oidc_email: None,
                                                        caps: None,
                                                        email: None,
                                                        created_unix: now_unix,
                                                    },
                                                );
                                                a.save()?;
                                            }

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
                                ("create", "oidc") | ("login", "oidc") | ("auto", "oidc") => {
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

                                        if action == "login" || action == "auto" {
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
                                                    if r.oidc_sub.as_deref() != Some(sub) {
                                                        let _ = write_tx
                                                            .send(Bytes::from_static(
                                                                b"web_auth: account not linked to oidc\r\nname: ",
                                                            ))
                                                            .await;
                                                        false
                                                    } else {
                                                        oidc_sub = Some(sub.to_string());
                                                        oidc_email =
                                                            r.oidc_email.clone().or(email.clone());
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
                                            {
                                                let mut a = accounts.lock().await;
                                                a.by_name.insert(
                                                    uname.clone(),
                                                    AccountRec {
                                                        name: uname.clone(),
                                                        pw_hash: None,
                                                        google_sub: None,
                                                        google_email: None,
                                                        oidc_sub: Some(sub.to_string()),
                                                        oidc_email: email.clone(),
                                                        caps: None,
                                                        email: None,
                                                        created_unix: now_unix,
                                                    },
                                                );
                                                a.save()?;
                                            }

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
                                _ => {
                                    let _ = write_tx
                                        .send(Bytes::from_static(
                                            b"web_auth: unsupported action/method\r\nname: ",
                                        ))
                                        .await;
                                    false
                                }
                            };

                            if ok {
                                state = ConnState::NeedBotDisclosure;
                                let _ = write_tx
                                    .send(Bytes::from_static(
                                        b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                    ))
                                    .await;
                            }
                            continue;
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
                                        if r.google_sub.as_deref() != Some(sub.as_str()) {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"name already taken\r\nname: ",
                                                ))
                                                .await;
                                            continue;
                                        }
                                        google_sub = Some(sub.clone());
                                        google_email = r.google_email.clone().or(email.clone());
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
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                            ))
                                            .await;
                                        continue;
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
                                                    google_sub: Some(sub.clone()),
                                                    google_email: email.clone(),
                                                    oidc_sub: None,
                                                    oidc_email: None,
                                                    caps: None,
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
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                            ))
                                            .await;
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
                                        if r.oidc_sub.as_deref() != Some(sub.as_str()) {
                                            let _ = write_tx
                                                .send(Bytes::from_static(
                                                    b"name already taken\r\nname: ",
                                                ))
                                                .await;
                                            continue;
                                        }
                                        oidc_sub = Some(sub.clone());
                                        oidc_email = r.oidc_email.clone().or(email.clone());
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
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                            ))
                                            .await;
                                        continue;
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
                                                    google_sub: None,
                                                    google_email: None,
                                                    oidc_sub: Some(sub.clone()),
                                                    oidc_email: email.clone(),
                                                    caps: None,
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
                                        let _ = write_tx
                                            .send(Bytes::from_static(
                                                b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                            ))
                                            .await;
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
                    let line = String::from_utf8_lossy(&line_bytes)
                        .trim()
                        .to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
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
                                if r.google_sub.is_none() {
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
                    let line = String::from_utf8_lossy(&line_bytes)
                        .trim()
                        .to_ascii_lowercase();
                    if line.is_empty() {
                        continue;
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
                            {
                                let mut a = accounts.lock().await;
                                if let Some(r) = a.by_name.get(&uname) {
                                    if r.google_sub.as_deref() != Some(sub) {
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
                                            google_sub: Some(sub.to_string()),
                                            google_email: email.clone(),
                                            oidc_sub: None,
                                            oidc_email: None,
                                            caps: None,
                                            email: None,
                                            created_unix: now_unix,
                                        },
                                    );
                                    a.save()?;
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

                            state = ConnState::NeedBotDisclosure;
                            let _ = write_tx
                                .send(Bytes::from_static(
                                    b"\r\ncharacter creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                                ))
                                .await;
                            continue;
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
                                google_sub: None,
                                google_email: None,
                                oidc_sub: None,
                                oidc_email: None,
                                caps: None,
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
                    state = ConnState::NeedBotDisclosure;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                        ))
                        .await;
                    continue;
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
                    let (hash, caps) = match rec {
                        Some(r) => (r.pw_hash, r.caps),
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
                    state = ConnState::NeedBotDisclosure;
                    let _ = write_tx
                        .send(Bytes::from_static(
                            b"character creation (step 2/4)\r\nare you using automation?\r\ntype: human | bot\r\n> ",
                        ))
                        .await;
                    continue;
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
                    state = ConnState::NeedCocAck;
                    let mut b = Vec::new();
                    b.extend_from_slice(b"character creation (step 4/4)\r\ncode of conduct:\r\n");
                    for li in COC_LINE_ITEMS {
                        b.extend_from_slice(li.as_bytes());
                        b.extend_from_slice(b"\r\n");
                    }
                    b.extend_from_slice(b"type: agree\r\n> ");
                    let _ = write_tx.send(Bytes::from(b)).await;
                    continue;
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
                    state = ConnState::NeedRace;
                    let mut s = String::new();
                    s.push_str("character creation (step 5/7)\r\nchoose race:\r\n");
                    s.push_str("type: race list | race <name>\r\n> ");
                    let _ = write_tx.send(Bytes::from(s)).await;
                    continue;
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

            let _ = shard_tx
                .send(ShardMsg {
                    t: REQ_INPUT,
                    session,
                    body: Bytes::from(line.into_bytes()),
                })
                .await;
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
    use super::{
        LineId, Scrollback, extract_scrollback_lines, normalize_email, redact_pii, trim_ascii_ws,
    };

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
}
