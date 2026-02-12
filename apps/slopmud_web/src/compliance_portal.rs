use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_sesv2::Client as SesClient;
use aws_sdk_sesv2::types::{Body, Content, Destination, EmailContent, Message};
use axum::Router;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Datelike, Duration as ChronoDuration, TimeZone, Utc};
use compliance::{CompliancePortalConfig, EmailDomainRule, LogStream, s3_key};
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message as LettreMessage, Tokio1Executor};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::warn;

use crate::AppState;

const COOKIE_NAME: &str = "slopmud_compliance";

#[derive(Clone)]
pub enum EmailSender {
    Ses {
        client: SesClient,
        from: String,
    },
    Smtp {
        transport: Arc<AsyncSmtpTransport<Tokio1Executor>>,
        from: String,
    },
    Disabled,
}

impl EmailSender {
    async fn send(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        match self {
            EmailSender::Disabled => anyhow::bail!("email sender disabled"),
            EmailSender::Ses { client, from } => {
                let dest = Destination::builder().to_addresses(to).build();
                let subj = Content::builder().data(subject).charset("UTF-8").build()?;
                let body = Content::builder().data(body).charset("UTF-8").build()?;
                let msg = Message::builder()
                    .subject(subj)
                    .body(Body::builder().text(body).build())
                    .build();
                let content = EmailContent::builder().simple(msg).build();
                client
                    .send_email()
                    .from_email_address(from)
                    .destination(dest)
                    .content(content)
                    .send()
                    .await?;
                Ok(())
            }
            EmailSender::Smtp { transport, from } => {
                let msg = LettreMessage::builder()
                    .from(from.parse::<Mailbox>()?)
                    .to(to.parse::<Mailbox>()?)
                    .subject(subject)
                    .body(body.to_string())?;
                transport.send(msg).await.map_err(|e| anyhow::anyhow!(e))?;
                Ok(())
            }
        }
    }
}

#[derive(Clone)]
pub struct ComplianceState {
    pub enabled: bool,

    pub portal_cfg: Arc<CompliancePortalConfig>,

    pub cookie_secure: bool,

    pub keys_path: PathBuf,
    pub audit_log_path: PathBuf,
    pub public_log_path: PathBuf,

    pub public_log_enabled: bool,
    pub public_log_redact_email: bool,

    pub session_ttl_s: u64,
    pub presign_ttl_s: u64,
    pub lookback_days: i64,

    pub accounts_path: PathBuf,
    pub broker_admin_addr: SocketAddr,

    pub s3_bucket: Option<String>,
    pub s3_prefix: String,
    pub s3: Option<S3Client>,

    pub email: EmailSender,

    sessions: Arc<tokio::sync::Mutex<HashMap<String, ComplianceSession>>>,
    pending: Arc<tokio::sync::Mutex<HashMap<String, PendingAction>>>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // Authoring/debug metadata; not currently read by the portal.
struct ComplianceSession {
    email: String,
    created_unix: u64,
    expires_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActionKind {
    Login,
    ViewCharacters,
    DownloadAll,
    DownloadLogin,
    DownloadCharacter { name: String },
    BanCharacter { name: String },
    BanIpPrefix { cidr: String },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReasonCode {
    Bullying,
    ThreatsViolence,
    FraudImpersonation,
    LawEnforcement,
    OtherwiseRequiredByLaw,
}

impl ReasonCode {
    fn all() -> &'static [ReasonCode] {
        &[
            ReasonCode::Bullying,
            ReasonCode::ThreatsViolence,
            ReasonCode::FraudImpersonation,
            ReasonCode::LawEnforcement,
            ReasonCode::OtherwiseRequiredByLaw,
        ]
    }

    fn as_key(self) -> &'static str {
        match self {
            ReasonCode::Bullying => "bullying",
            ReasonCode::ThreatsViolence => "threats_violence",
            ReasonCode::FraudImpersonation => "fraud_impersonation",
            ReasonCode::LawEnforcement => "law_enforcement",
            ReasonCode::OtherwiseRequiredByLaw => "otherwise_required_by_law",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ReasonCode::Bullying => "Bullying / harassment",
            ReasonCode::ThreatsViolence => "Threats / violence",
            ReasonCode::FraudImpersonation => "Fraud / impersonation",
            ReasonCode::LawEnforcement => "Law enforcement request",
            ReasonCode::OtherwiseRequiredByLaw => "Otherwise required by law",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "bullying" => Some(ReasonCode::Bullying),
            "threats_violence" => Some(ReasonCode::ThreatsViolence),
            "fraud_impersonation" => Some(ReasonCode::FraudImpersonation),
            "law_enforcement" => Some(ReasonCode::LawEnforcement),
            "otherwise_required_by_law" => Some(ReasonCode::OtherwiseRequiredByLaw),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct PendingAction {
    id: String,
    email: String,
    requester_ip: IpAddr,
    created_unix: u64,
    kind: ActionKind,
    reason: Option<ReasonCode>,
    key_hash_hex: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KeyDb {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    keys: Vec<KeyRec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KeyRec {
    key_hash_hex: String,
    email: String,
    issued_unix: u64,
    expires_unix: u64,
    #[serde(default)]
    used_unix: Option<u64>,
}

impl ComplianceState {
    pub async fn from_env() -> anyhow::Result<Self> {
        let enabled = std::env::var("SLOPMUD_COMPLIANCE_ENABLED")
            .ok()
            .is_some_and(|v| v == "1");

        let cookie_secure = std::env::var("HTTPS_BIND")
            .ok()
            .is_some_and(|v| !v.trim().is_empty());

        let mut portal_cfg = CompliancePortalConfig {
            email_domain_allowlist: vec![
                EmailDomainRule {
                    suffix: "gov".to_string(),
                    advertised: true,
                    country: Some("United States".to_string()),
                },
                EmailDomainRule {
                    suffix: "mil".to_string(),
                    advertised: true,
                    country: Some("United States".to_string()),
                },
            ],
        };
        if let Ok(s) = std::env::var("SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON")
            && !s.trim().is_empty()
        {
            portal_cfg =
                serde_json::from_str(&s).context("parse SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON")?;
        }

        let keys_path: PathBuf = std::env::var("SLOPMUD_COMPLIANCE_KEYS_PATH")
            .unwrap_or_else(|_| "locks/compliance/keys.json".to_string())
            .into();
        let audit_log_path: PathBuf = std::env::var("SLOPMUD_COMPLIANCE_AUDIT_LOG_PATH")
            .unwrap_or_else(|_| "locks/compliance/audit.logfmt".to_string())
            .into();
        let public_log_path: PathBuf = std::env::var("SLOPMUD_COMPLIANCE_PUBLIC_LOG_PATH")
            .unwrap_or_else(|_| "locks/compliance/public.logfmt".to_string())
            .into();

        let public_log_enabled = std::env::var("SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED")
            .ok()
            .map(|v| v != "0")
            .unwrap_or(true);

        // Optional: redact public emails (defaults to full email in public log).
        let public_log_redact_email = std::env::var("SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL")
            .ok()
            .is_some_and(|v| v == "1");

        let session_ttl_s = std::env::var("SLOPMUD_COMPLIANCE_SESSION_TTL_S")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);
        let presign_ttl_s = std::env::var("SLOPMUD_COMPLIANCE_PRESIGN_TTL_S")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);
        let lookback_days = std::env::var("SLOPMUD_COMPLIANCE_LOOKBACK_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30)
            .clamp(1, 90);

        let accounts_path: PathBuf = std::env::var("SLOPMUD_ACCOUNTS_PATH")
            .unwrap_or_else(|_| "accounts.json".to_string())
            .into();

        let broker_admin_addr: SocketAddr = std::env::var("SLOPMUD_ADMIN_ADDR")
            .or_else(|_| std::env::var("SLOPMUD_ADMIN_BIND"))
            .unwrap_or_else(|_| "127.0.0.1:4011".to_string())
            .parse()
            .context("parse SLOPMUD_ADMIN_ADDR")?;

        let s3_bucket = std::env::var("SLOPMUD_EVENTLOG_S3_BUCKET")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let s3_prefix = std::env::var("SLOPMUD_EVENTLOG_S3_PREFIX")
            .unwrap_or_else(|_| "slopmud/eventlog".to_string());

        let s3 = if enabled && s3_bucket.is_some() {
            let aws_cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            Some(S3Client::new(&aws_cfg))
        } else {
            None
        };

        let email_mode = std::env::var("SLOPMUD_COMPLIANCE_EMAIL_MODE")
            .unwrap_or_else(|_| "disabled".to_string())
            .to_ascii_lowercase();
        let email_from = std::env::var("SLOPMUD_COMPLIANCE_EMAIL_FROM").ok();

        let email = match email_mode.as_str() {
            "disabled" => EmailSender::Disabled,
            "ses" => {
                let Some(from) = email_from.clone() else {
                    anyhow::bail!(
                        "SLOPMUD_COMPLIANCE_EMAIL_MODE=ses but missing SLOPMUD_COMPLIANCE_EMAIL_FROM"
                    );
                };
                let aws_cfg =
                    aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                EmailSender::Ses {
                    client: SesClient::new(&aws_cfg),
                    from,
                }
            }
            "smtp" => {
                let Some(from) = email_from.clone() else {
                    anyhow::bail!(
                        "SLOPMUD_COMPLIANCE_EMAIL_MODE=smtp but missing SLOPMUD_COMPLIANCE_EMAIL_FROM"
                    );
                };
                let host = std::env::var("SLOPMUD_COMPLIANCE_SMTP_HOST")
                    .context("missing SLOPMUD_COMPLIANCE_SMTP_HOST")?;
                let port: u16 = std::env::var("SLOPMUD_COMPLIANCE_SMTP_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(587);
                let user = std::env::var("SLOPMUD_COMPLIANCE_SMTP_USERNAME").unwrap_or_default();
                let pass = std::env::var("SLOPMUD_COMPLIANCE_SMTP_PASSWORD").unwrap_or_default();

                let mut builder =
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?.port(port);
                if !user.is_empty() {
                    builder = builder.credentials(Credentials::new(user, pass));
                }
                let transport = builder.build();
                EmailSender::Smtp {
                    transport: Arc::new(transport),
                    from,
                }
            }
            _ => anyhow::bail!("unknown SLOPMUD_COMPLIANCE_EMAIL_MODE={email_mode:?}"),
        };

        Ok(Self {
            enabled,
            portal_cfg: Arc::new(portal_cfg),
            cookie_secure,
            keys_path,
            audit_log_path,
            public_log_path,
            public_log_enabled,
            public_log_redact_email,
            session_ttl_s,
            presign_ttl_s,
            lookback_days: lookback_days as i64,
            accounts_path,
            broker_admin_addr,
            s3_bucket,
            s3_prefix,
            s3,
            email,
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            pending: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }

    fn email_allowed(&self, email: &str) -> bool {
        self.portal_cfg.email_allowed(email)
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/compliance", get(compliance_home))
        .route("/compliance/request_key", post(compliance_request_key))
        .route("/compliance/login", post(compliance_login))
        .route(
            "/compliance/action/preview",
            post(compliance_action_preview),
        )
        .route(
            "/compliance/action/confirm",
            post(compliance_action_confirm),
        )
        .route("/transparency/compliance", get(transparency_page))
        .route("/transparency/compliance.log", get(transparency_log))
}

#[derive(Deserialize)]
struct RequestKeyForm {
    email: String,
}

#[derive(Deserialize)]
struct LoginForm {
    access_key: String,
}

#[derive(Deserialize)]
struct PrepareActionForm {
    action: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    character: String,
    #[serde(default)]
    cidr: String,
    #[serde(default)]
    understand: String,
    #[serde(default)]
    pending_id: String,
}

async fn get_session(st: &ComplianceState, jar: &CookieJar) -> Option<ComplianceSession> {
    let token = jar.get(COOKIE_NAME).map(|c| c.value().to_string())?;
    let now = now_unix();

    let mut sessions = st.sessions.lock().await;
    sessions.retain(|_, s| s.expires_unix > now);
    sessions.get(&token).cloned()
}

async fn compliance_home(State(st): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if !st.compliance.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(sess) = get_session(&st.compliance, &jar).await {
        return Html(render_portal_home(&st, &sess.email)).into_response();
    }

    Html(render_login_page(&st)).into_response()
}

async fn compliance_request_key(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(f): Form<RequestKeyForm>,
) -> impl IntoResponse {
    if !st.compliance.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let email = f.email.trim().to_string();
    if email.len() > 254 {
        return (StatusCode::BAD_REQUEST, "bad email\n").into_response();
    }

    // Don't log key requests. Only log first successful login.
    if !st.compliance.email_allowed(&email) {
        // Avoid enumerating allowed domains; always respond success-ish.
        return Html(render_key_sent_page()).into_response();
    }

    let key = random_key_hex(16);
    let key_hash = sha256_hex(key.as_bytes());

    let now_unix = now_unix();
    let expires_unix = now_unix.saturating_add(15 * 60);

    if let Err(e) = upsert_key_rec(
        &st.compliance.keys_path,
        KeyRec {
            key_hash_hex: key_hash.clone(),
            email: email.clone(),
            issued_unix: now_unix,
            expires_unix,
            used_unix: None,
        },
    ) {
        warn!(err=%e, "failed to store compliance key");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let subj = "slopmud compliance portal access key";
    let body = format!(
        "Your slopmud compliance portal access key:\n\n  {key}\n\nThis key expires in 15 minutes.\n\nUsing this key to log in and performing compliance actions will create entries in our public compliance transparency log.\nThe public transparency log includes your email address.\n\nWe also keep a private audit log that includes your email address and requester IP address.\n\nRequester IP: {}\n",
        peer.ip()
    );

    if let Err(e) = st.compliance.email.send(&email, subj, &body).await {
        warn!(err=%e, "failed to send compliance email");
        // Still return generic success to avoid domain enumeration.
    }

    Html(render_key_sent_page()).into_response()
}

async fn compliance_login(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(f): Form<LoginForm>,
) -> impl IntoResponse {
    if !st.compliance.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let key = f.access_key.trim();
    if key.is_empty() || key.len() > 128 {
        return (StatusCode::BAD_REQUEST, "bad key\n").into_response();
    }

    let key_hash = sha256_hex(key.as_bytes());
    let rec = match find_valid_key(&st.compliance.keys_path, &key_hash) {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::FORBIDDEN, "bad/expired key\n").into_response(),
        Err(e) => {
            warn!(err=%e, "key db error");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Require confirmation before we create a public log entry.
    let pending = PendingAction {
        id: random_key_hex(16),
        email: rec.email,
        requester_ip: peer.ip(),
        created_unix: now_unix(),
        kind: ActionKind::Login,
        reason: None,
        key_hash_hex: Some(key_hash),
    };
    st.compliance
        .pending
        .lock()
        .await
        .insert(pending.id.clone(), pending.clone());

    Html(render_preview_page(&st, &pending)).into_response()
}

async fn compliance_action_preview(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Form(f): Form<PrepareActionForm>,
) -> impl IntoResponse {
    if !st.compliance.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let Some(sess) = get_session(&st.compliance, &jar).await else {
        return Redirect::temporary("/compliance").into_response();
    };

    let reason = ReasonCode::parse(&f.reason);

    let kind = match f.action.as_str() {
        "view_characters" => ActionKind::ViewCharacters,
        "download_all" => ActionKind::DownloadAll,
        "download_login" => ActionKind::DownloadLogin,
        "download_character" => ActionKind::DownloadCharacter {
            name: f.character.trim().to_string(),
        },
        "ban_character" => ActionKind::BanCharacter {
            name: f.character.trim().to_string(),
        },
        "ban_ip_prefix" => ActionKind::BanIpPrefix {
            cidr: f.cidr.trim().to_string(),
        },
        _ => return (StatusCode::BAD_REQUEST, "bad action\n").into_response(),
    };

    // Require a reason for all actions (no free-form).
    if reason.is_none() {
        return (StatusCode::BAD_REQUEST, "missing reason\n").into_response();
    }

    // Basic validation.
    match &kind {
        ActionKind::DownloadCharacter { name } | ActionKind::BanCharacter { name } => {
            if name.trim().is_empty() || name.len() > 20 {
                return (StatusCode::BAD_REQUEST, "bad character\n").into_response();
            }
        }
        ActionKind::BanIpPrefix { cidr } => {
            if cidr.trim().is_empty() || cidr.len() > 64 {
                return (StatusCode::BAD_REQUEST, "bad cidr\n").into_response();
            }
        }
        _ => {}
    }

    let pending = PendingAction {
        id: random_key_hex(16),
        email: sess.email,
        requester_ip: peer.ip(),
        created_unix: now_unix(),
        kind,
        reason,
        key_hash_hex: None,
    };
    st.compliance
        .pending
        .lock()
        .await
        .insert(pending.id.clone(), pending.clone());

    Html(render_preview_page(&st, &pending)).into_response()
}

async fn compliance_action_confirm(
    State(st): State<AppState>,
    jar: CookieJar,
    Form(f): Form<PrepareActionForm>,
) -> impl IntoResponse {
    if !st.compliance.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let pending_id = f.pending_id.trim();
    if pending_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing pending_id\n").into_response();
    }

    if f.understand != "on" {
        return (StatusCode::BAD_REQUEST, "must check understand\n").into_response();
    }

    let pending = {
        let mut p = st.compliance.pending.lock().await;
        p.remove(pending_id)
    };
    let Some(pending) = pending else {
        return (StatusCode::BAD_REQUEST, "unknown pending_id\n").into_response();
    };

    if !matches!(pending.kind, ActionKind::Login) {
        let Some(sess) = get_session(&st.compliance, &jar).await else {
            return Redirect::temporary("/compliance").into_response();
        };
        if sess.email != pending.email {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    // Always log after explicit confirmation.
    //
    // For login, we only log if the one-time key is successfully consumed so the
    // public log can't claim a login that didn't happen.
    if !matches!(pending.kind, ActionKind::Login)
        && let Err(e) = write_compliance_logs(&st, &pending)
    {
        warn!(err=%e, "failed to write compliance logs");
    }

    match pending.kind {
        ActionKind::Login => {
            let Some(key_hash) = pending.key_hash_hex.as_deref() else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            let rec = match mark_key_used(&st.compliance.keys_path, key_hash) {
                Ok(Some(r)) => r,
                Ok(None) => return (StatusCode::FORBIDDEN, "bad/expired key\n").into_response(),
                Err(e) => {
                    warn!(err=%e, "key db error");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

            // Create a compliance session.
            let now = now_unix();
            let token = random_key_hex(24);
            let sess = ComplianceSession {
                email: rec.email,
                created_unix: now,
                expires_unix: now.saturating_add(st.compliance.session_ttl_s),
            };
            st.compliance
                .sessions
                .lock()
                .await
                .insert(token.clone(), sess);

            if let Err(e) = write_compliance_logs(&st, &pending) {
                warn!(err=%e, "failed to write compliance logs");
            }

            let cookie = Cookie::build((COOKIE_NAME, token))
                .path("/compliance")
                .http_only(true)
                .same_site(SameSite::Lax)
                .secure(st.compliance.cookie_secure)
                .build();

            (jar.add(cookie), Redirect::temporary("/compliance")).into_response()
        }
        ActionKind::ViewCharacters => match load_character_list(&st.compliance.accounts_path) {
            Ok(names) => Html(render_characters_page(&names)).into_response(),
            Err(e) => {
                warn!(err=%e, "failed to load character list");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        ActionKind::DownloadAll => match download_links(&st, LogStream::All, None).await {
            Ok(html) => Html(html).into_response(),
            Err(e) => {
                warn!(err=%e, "download all failed");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        },
        ActionKind::DownloadLogin => match download_links(&st, LogStream::Login, None).await {
            Ok(html) => Html(html).into_response(),
            Err(e) => {
                warn!(err=%e, "download login failed");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        },
        ActionKind::DownloadCharacter { name } => {
            match download_links(&st, LogStream::Character(&name), Some(&name)).await {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!(err=%e, "download character failed");
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                }
            }
        }
        ActionKind::BanCharacter { name } => {
            match admin_ban_character(&st, &pending.email, &name, pending.reason).await {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!(err=%e, "ban character failed");
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                }
            }
        }
        ActionKind::BanIpPrefix { cidr } => {
            match admin_ban_ip(&st, &pending.email, &cidr, pending.reason).await {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!(err=%e, "ban ip failed");
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                }
            }
        }
    }
}

async fn transparency_page(State(st): State<AppState>) -> impl IntoResponse {
    if !st.compliance.enabled || !st.compliance.public_log_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    Html(render_transparency_page(&st)).into_response()
}

async fn transparency_log(State(st): State<AppState>) -> impl IntoResponse {
    if !st.compliance.enabled || !st.compliance.public_log_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = st.compliance.public_log_path.clone();
    let f = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let stream = tokio_util::io::ReaderStream::new(f);

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "text/plain; charset=utf-8".parse().unwrap(),
    );
    (headers, axum::body::Body::from_stream(stream)).into_response()
}

fn render_login_page(st: &AppState) -> String {
    let doms_by_country = st
        .compliance
        .portal_cfg
        .advertised_domain_suffixes_by_country();

    let domains_html = if doms_by_country.is_empty() {
        "(configured allowlist is empty)".to_string()
    } else {
        let mut s = String::new();
        s.push_str("<ul>");
        for (country, doms) in doms_by_country {
            let doms_html = doms
                .iter()
                .map(|d| format!("<code>.{}</code>", html_escape(d)))
                .collect::<Vec<_>>()
                .join(" ");
            s.push_str(&format!(
                "<li><strong>{}</strong>: {}</li>",
                html_escape(&country),
                doms_html
            ));
        }
        s.push_str("</ul>");
        s
    };

    format!(
        "<h1>Compliance Portal</h1>\
<p>Request an access key via email, then log in with that key.</p>\
<p><strong>Notice:</strong> Logging in and performing actions will create entries in a <a href=\"/transparency/compliance\">public transparency log</a>. The public log includes your email address.</p>\
<p>We also keep a private audit log that includes your email address and requester IP.</p>\
<h2>Request Key</h2>\
<form method=\"post\" action=\"/compliance/request_key\">\
  <label>Email: <input name=\"email\" type=\"email\" size=\"40\" required></label>\
  <button type=\"submit\">Send Access Key</button>\
</form>\
<h3>Advertised allowlist</h3>{domains_html}<p><small>Some allowlisted domains may not be advertised.</small></p>\
<h2>Log In</h2>\
<form method=\"post\" action=\"/compliance/login\">\
  <label>Access key: <input name=\"access_key\" type=\"password\" size=\"40\" required></label>\
  <button type=\"submit\">Continue</button>\
</form>\
",
        domains_html = domains_html
    )
}

fn render_key_sent_page() -> String {
    "<h1>Check your email</h1><p>If the address is allowlisted, an access key has been sent.</p><p><a href=\"/compliance\">Back</a></p>".to_string()
}

fn render_portal_home(_st: &AppState, email: &str) -> String {
    let reasons = render_reason_select();
    format!(
        "<h1>Compliance Portal</h1>\
<p>Logged in as <code>{}</code></p>\
<p><strong>Reminder:</strong> Each action below creates an entry in the public transparency log (includes your email) and in the private audit log (includes your email + requester IP).</p>\
<h2>Actions</h2>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"view_characters\">\
  {reasons}\
  <button type=\"submit\">View Character List</button>\
</form>\
<hr>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"download_all\">\
  {reasons}\
  <button type=\"submit\">Download All Session Events (Last 30 Days)</button>\
</form>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"download_login\">\
  {reasons}\
  <button type=\"submit\">Download Login/Logout Log (Last 30 Days)</button>\
</form>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"download_character\">\
  <label>Character: <input name=\"character\" size=\"24\" required></label>\
  {reasons}\
  <button type=\"submit\">Download Character Events (Last 30 Days)</button>\
</form>\
<hr>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"ban_character\">\
  <label>Character: <input name=\"character\" size=\"24\" required></label>\
  {reasons}\
  <button type=\"submit\">Ban Character (Immediate Expulsion)</button>\
</form>\
<form method=\"post\" action=\"/compliance/action/preview\">\
  <input type=\"hidden\" name=\"action\" value=\"ban_ip_prefix\">\
  <label>IP range (CIDR): <input name=\"cidr\" size=\"24\" placeholder=\"203.0.113.0/24\" required></label>\
  {reasons}\
  <button type=\"submit\">Ban IP Range (Immediate Expulsion)</button>\
</form>\
",
        html_escape(email),
        reasons = reasons
    )
}

fn render_reason_select() -> String {
    let mut s = String::new();
    s.push_str("<label>Reason: <select name=\"reason\" required>");
    s.push_str("<option value=\"\" selected disabled>(select)</option>");
    for r in ReasonCode::all() {
        s.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            r.as_key(),
            html_escape(r.label())
        ));
    }
    s.push_str("</select></label>");
    s
}

fn render_preview_page(st: &AppState, p: &PendingAction) -> String {
    let (public_line, _audit_line) = format_compliance_log_entries(st, p);

    format!(
        "<h1>Confirm Compliance Action</h1>\
<p><strong>Notice:</strong> Confirming will append an entry to the public transparency log and will include your email address.</p>\
<h2>Public log entry preview</h2>\
<pre>{}</pre>\
<form method=\"post\" action=\"/compliance/action/confirm\">\
  <input type=\"hidden\" name=\"pending_id\" value=\"{}\">\
  <label><input type=\"checkbox\" name=\"understand\" required> I understand</label>\
  <button type=\"submit\">Confirm</button>\
</form>\
<p><a href=\"/compliance\">Cancel</a></p>\
",
        html_escape(&public_line),
        html_escape(&p.id)
    )
}

fn render_characters_page(names: &[String]) -> String {
    let mut s = String::new();
    s.push_str("<h1>Characters</h1>");
    if names.is_empty() {
        s.push_str("<p>(none)</p>");
    } else {
        s.push_str("<ul>");
        for n in names {
            s.push_str(&format!("<li><code>{}</code></li>", html_escape(n)));
        }
        s.push_str("</ul>");
    }
    s.push_str("<p><a href=\"/compliance\">Back</a></p>");
    s
}

fn render_transparency_page(st: &AppState) -> String {
    let actions = [
        "login",
        "view_characters",
        "download_all",
        "download_login",
        "download_character",
        "ban_character",
        "ban_ip_prefix",
    ];
    let mut s = String::new();
    s.push_str("<h1>Compliance Transparency Log</h1>");
    s.push_str("<p>This page is public.</p>");
    s.push_str("<h2>Actions We Log</h2><ul>");
    for a in actions {
        s.push_str(&format!("<li><code>{}</code></li>", a));
    }
    s.push_str("</ul>");
    s.push_str(
        "<p>Raw log: <a href=\"/transparency/compliance.log\">/transparency/compliance.log</a></p>",
    );

    if let Ok(tail) = tail_file_lines(&st.compliance.public_log_path, 200) {
        s.push_str("<h2>Recent Entries</h2>");
        s.push_str("<pre>");
        s.push_str(&html_escape(&tail));
        s.push_str("</pre>");
    }

    s
}

fn tail_file_lines(path: &Path, max_lines: usize) -> anyhow::Result<String> {
    let f = std::fs::File::open(path)?;
    let rd = std::io::BufReader::new(f);
    let mut ring: VecDeque<String> = VecDeque::with_capacity(max_lines.min(10_000));
    for line in std::io::BufRead::lines(rd) {
        let line = line?;
        if ring.len() == max_lines {
            ring.pop_front();
        }
        ring.push_back(line);
    }
    Ok(ring.into_iter().collect::<Vec<_>>().join("\n") + "\n")
}

fn load_character_list(path: &Path) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct AccountRec {
        name: String,
    }

    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read accounts file {}", path.display()))?;
    let v: Vec<AccountRec> = serde_json::from_str(&s)?;
    let mut out = v.into_iter().map(|r| r.name).collect::<Vec<_>>();
    out.sort();
    out.dedup();
    Ok(out)
}

async fn download_links(
    st: &AppState,
    stream: LogStream<'_>,
    label: Option<&str>,
) -> anyhow::Result<String> {
    let Some(bucket) = st.compliance.s3_bucket.as_deref() else {
        anyhow::bail!("missing S3 bucket config");
    };
    let Some(s3) = st.compliance.s3.as_ref() else {
        anyhow::bail!("missing S3 client");
    };

    let ttl = Duration::from_secs(st.compliance.presign_ttl_s.clamp(60, 24 * 3600));

    let end = Utc::now().date_naive();
    let start = end - ChronoDuration::days(st.compliance.lookback_days.saturating_sub(1));

    let mut urls = Vec::new();
    let mut day = start;
    while day <= end {
        let ts = Utc
            .with_ymd_and_hms(day.year(), day.month(), day.day(), 0, 0, 0)
            .single()
            .unwrap_or_else(Utc::now);

        let key = s3_key(&st.compliance.s3_prefix, stream, ts);

        // Only include existing objects.
        let head = s3.head_object().bucket(bucket).key(&key).send().await;
        if head.is_ok() {
            let presigned = s3
                .get_object()
                .bucket(bucket)
                .key(&key)
                .presigned(PresigningConfig::expires_in(ttl)?)
                .await?;
            urls.push((day.to_string(), presigned.uri().to_string()));
        }

        day += ChronoDuration::days(1);
    }

    let title = match label {
        Some(n) => format!("Download links for {}", html_escape(n)),
        None => "Download links".to_string(),
    };

    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>", title));
    body.push_str("<p>These are presigned URLs to S3 objects.</p>");
    if urls.is_empty() {
        body.push_str("<p>(no objects found)</p>");
    } else {
        body.push_str("<ul>");
        for (d, u) in &urls {
            body.push_str(&format!(
                "<li><code>{}</code> <a href=\"{}\">download</a></li>",
                html_escape(d),
                html_escape(u)
            ));
        }
        body.push_str("</ul>");

        body.push_str("<h2>URL manifest</h2><pre>");
        for (_d, u) in &urls {
            body.push_str(&html_escape(u));
            body.push('\n');
        }
        body.push_str("</pre>");
    }
    body.push_str("<p><a href=\"/compliance\">Back</a></p>");

    Ok(body)
}

#[derive(Serialize)]
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
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // Response payload fields are not always consumed by callers.
enum AdminResp {
    Ok {
        kicked: u64,
    },
    Err {
        message: String,
    },
    #[serde(other)]
    Other,
}

async fn admin_ban_character(
    st: &AppState,
    email: &str,
    name: &str,
    reason: Option<ReasonCode>,
) -> anyhow::Result<String> {
    let req = AdminReq::BanCharacter {
        name: name.to_string(),
        created_by: email.to_string(),
        reason: reason.map(|r| r.as_key().to_string()).unwrap_or_default(),
    };
    let resp = admin_send(st.compliance.broker_admin_addr, &req).await?;
    Ok(format!(
        "<h1>Ban character</h1><pre>{}</pre><p><a href=\"/compliance\">Back</a></p>",
        html_escape(&resp)
    ))
}

async fn admin_ban_ip(
    st: &AppState,
    email: &str,
    cidr: &str,
    reason: Option<ReasonCode>,
) -> anyhow::Result<String> {
    let req = AdminReq::BanIpPrefix {
        cidr: cidr.to_string(),
        created_by: email.to_string(),
        reason: reason.map(|r| r.as_key().to_string()).unwrap_or_default(),
    };
    let resp = admin_send(st.compliance.broker_admin_addr, &req).await?;
    Ok(format!(
        "<h1>Ban IP prefix</h1><pre>{}</pre><p><a href=\"/compliance\">Back</a></p>",
        html_escape(&resp)
    ))
}

async fn admin_send(addr: SocketAddr, req: &AdminReq) -> anyhow::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;
    let s = serde_json::to_string(req)?;
    stream.write_all(s.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let (rd, _) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    Ok(line.trim().to_string())
}

fn format_compliance_log_entries(st: &AppState, p: &PendingAction) -> (String, String) {
    // Use the time the action was previewed, so the preview line matches what we write on confirm.
    let ts = Utc
        .timestamp_opt(p.created_unix as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339();
    let action = match &p.kind {
        ActionKind::Login => "login".to_string(),
        ActionKind::ViewCharacters => "view_characters".to_string(),
        ActionKind::DownloadAll => "download_all".to_string(),
        ActionKind::DownloadLogin => "download_login".to_string(),
        ActionKind::DownloadCharacter { name } => format!("download_character:{}", name),
        ActionKind::BanCharacter { name } => format!("ban_character:{}", name),
        ActionKind::BanIpPrefix { cidr } => format!("ban_ip_prefix:{}", cidr),
    };

    let reason = p.reason.map(|r| r.as_key()).unwrap_or("");

    let email_audit = p.email.clone();
    let email_redacted = if st.compliance.public_log_redact_email {
        redact_email(&p.email)
    } else {
        p.email.clone()
    };
    let email_hash = sha256_hex(p.email.as_bytes());

    let requester_ip = p.requester_ip.to_string();

    let pub_line = format!(
        "ts={} action={} reason={} email={} email_hash={} ip={}",
        logfmt_str(&ts),
        logfmt_str(&action),
        logfmt_str(reason),
        logfmt_str(&email_redacted),
        logfmt_str(&email_hash),
        logfmt_str(&requester_ip),
    );

    let audit_line = format!(
        "ts={} action={} reason={} email={} email_hash={} ip={}",
        logfmt_str(&ts),
        logfmt_str(&action),
        logfmt_str(reason),
        logfmt_str(&email_audit),
        logfmt_str(&email_hash),
        logfmt_str(&requester_ip),
    );

    (pub_line, audit_line)
}

fn write_compliance_logs(st: &AppState, p: &PendingAction) -> anyhow::Result<()> {
    let (public_line, audit_line) = format_compliance_log_entries(st, p);

    append_line(&st.compliance.audit_log_path, &audit_line)?;
    if st.compliance.public_log_enabled {
        append_line(&st.compliance.public_log_path, &public_line)?;
    }
    Ok(())
}

fn append_line(path: &Path, line: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

fn redact_email(email: &str) -> String {
    let email = email.trim();
    let Some((local, dom)) = email.split_once('@') else {
        return "(redacted)".to_string();
    };
    let mut out = String::new();
    let mut chars = local.chars();
    if let Some(first) = chars.next() {
        out.push(first);
        out.push_str("***");
    } else {
        out.push_str("***");
    }
    out.push('@');
    out.push_str(dom);
    out
}

fn sha256_hex(b: &[u8]) -> String {
    let mut h = sha2::Sha256::new();
    h.update(b);
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for x in out {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

fn random_key_hex(nbytes: usize) -> String {
    let mut b = vec![0u8; nbytes];
    getrandom::getrandom(&mut b).expect("getrandom");
    let mut s = String::with_capacity(nbytes * 2);
    for x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn logfmt_str(s: &str) -> String {
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

fn upsert_key_rec(path: &Path, rec: KeyRec) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut db = load_key_db(path).unwrap_or(KeyDb {
        version: 1,
        keys: Vec::new(),
    });

    // Prune old.
    let now = now_unix();
    db.keys
        .retain(|k| k.used_unix.is_none() && k.expires_unix.saturating_add(3600) > now);

    db.keys.push(rec);

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&db)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn load_key_db(path: &Path) -> anyhow::Result<KeyDb> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

fn find_valid_key(path: &Path, key_hash_hex: &str) -> anyhow::Result<Option<KeyRec>> {
    if !path.exists() {
        return Ok(None);
    }

    let db = load_key_db(path)?;
    let now = now_unix();

    for k in db.keys.iter() {
        if k.key_hash_hex == key_hash_hex {
            if k.used_unix.is_some() {
                return Ok(None);
            }
            if now > k.expires_unix {
                return Ok(None);
            }
            return Ok(Some(k.clone()));
        }
    }

    Ok(None)
}

fn mark_key_used(path: &Path, key_hash_hex: &str) -> anyhow::Result<Option<KeyRec>> {
    if !path.exists() {
        return Ok(None);
    }

    let mut db = load_key_db(path)?;
    let now = now_unix();

    let mut found: Option<KeyRec> = None;
    for k in db.keys.iter_mut() {
        if k.key_hash_hex == key_hash_hex {
            if k.used_unix.is_some() {
                return Ok(None);
            }
            if now > k.expires_unix {
                return Ok(None);
            }
            k.used_unix = Some(now);
            found = Some(k.clone());
            break;
        }
    }

    // Prune old.
    db.keys
        .retain(|k| k.used_unix.is_none() && k.expires_unix.saturating_add(3600) > now);

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&db)?)?;
    std::fs::rename(&tmp, path)?;

    Ok(found)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
