use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use argon2::Argon2;
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use ed25519_dalek::SigningKey;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use pkcs8::{EncodePrivateKey, LineEnding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{Level, info, warn};

#[derive(Clone)]
struct AppState {
    cfg: Arc<Config>,
    auth_codes: Arc<tokio::sync::Mutex<HashMap<String, AuthCode>>>,
    userdb_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    issuer: String,
    client_id: String,
    client_secret: String,
    token_ttl_s: u64,
    auth_code_ttl_s: u64,
    // Enables /authorize if set.
    users_path: Option<PathBuf>,
    // Optional allowlist for /authorize redirect_uri.
    allowed_redirect_uris: Vec<String>,
    // Allow plaintext `password` fields in the user db (intended for local dev only).
    allow_plaintext_passwords: bool,
    // Allow self-serve user registration via /register (writes to OIDC_USERS_PATH).
    allow_registration: bool,
    // Allow self-serve password reset via /reset (writes to OIDC_USERS_PATH).
    allow_password_reset: bool,
    // PKCS8 PEM, because jsonwebtoken wants PEM for EdDSA.
    signing_key_pem: String,
    jwk: Jwk,
}

#[derive(Clone, Debug, Serialize)]
struct OidcDiscovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    jwks_uri: String,
    response_types_supported: Vec<String>,
    subject_types_supported: Vec<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Vec<String>,
    grant_types_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Clone, Debug, Serialize)]
struct Jwk {
    kty: String,
    crv: String,
    x: String,
    use_: String,
    alg: String,
    kid: String,
}

#[derive(Debug, Deserialize)]
struct TokenForm {
    #[serde(default)]
    grant_type: String,
    // Internal extension for this service:
    // the broker can request a token for a specific session/name without sending any password.
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    sid: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    // OAuth authorization code flow (PKCE).
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenResp {
    access_token: String,
    token_type: String,
    expires_in: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
    // Custom claims.
    #[serde(default)]
    sid: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    client_id: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    caps: Option<Vec<String>>,
}

fn usage_and_exit() -> ! {
    eprintln!(
        "internal_oidc (minimal internal OIDC provider/token issuer)\n\n\
USAGE:\n  internal_oidc\n  internal_oidc hash-password <password>\n\n\
ENV:\n  OIDC_BIND                     default 127.0.0.1:9000\n  OIDC_ISSUER                   default http://127.0.0.1:9000\n  OIDC_CLIENT_ID                required\n  OIDC_CLIENT_SECRET            required\n  OIDC_TOKEN_TTL_S              default 3600\n  OIDC_ED25519_SEED_B64         optional; 32 bytes seed (base64). If omitted, generates an ephemeral key.\n  OIDC_ALLOWED_REDIRECT_URIS    optional; comma-separated allowlist for /authorize redirect_uri\n  OIDC_USERS_PATH               optional; if set, enables /authorize using this JSON user db\n  OIDC_AUTH_CODE_TTL_S          default 300\n  OIDC_ALLOW_PLAINTEXT_PASSWORDS optional; default 0\n  OIDC_ALLOW_REGISTRATION        optional; default 0 (dev only)\n  OIDC_ALLOW_PASSWORD_RESET      optional; default 0 (dev only)\n"
    );
    std::process::exit(2);
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random_b64url(nbytes: usize) -> anyhow::Result<String> {
    let mut b = vec![0u8; nbytes];
    getrandom::getrandom(&mut b).map_err(|e| anyhow::anyhow!("getrandom: {e:?}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b))
}

fn base64url_sha256(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
}

fn join_query(base: &str, qs: &str) -> String {
    let mut out = base.trim().to_string();
    if out.contains('?') {
        if !out.ends_with('?') && !out.ends_with('&') {
            out.push('&');
        }
    } else {
        out.push('?');
    }
    out.push_str(qs);
    out
}

fn redirect_see_other(url: &str) -> axum::response::Response {
    // For form POST handlers, use 303 so user agents follow with GET.
    (StatusCode::SEE_OTHER, [(header::LOCATION, url)]).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

fn parse_cfg() -> anyhow::Result<Config> {
    let bind: SocketAddr = std::env::var("OIDC_BIND")
        .unwrap_or_else(|_| "127.0.0.1:9000".to_string())
        .parse()
        .map_err(|_| anyhow::anyhow!("bad OIDC_BIND"))?;
    let issuer = std::env::var("OIDC_ISSUER").unwrap_or_else(|_| format!("http://{bind}"));

    let client_id =
        std::env::var("OIDC_CLIENT_ID").map_err(|_| anyhow::anyhow!("missing OIDC_CLIENT_ID"))?;
    let client_secret = std::env::var("OIDC_CLIENT_SECRET")
        .map_err(|_| anyhow::anyhow!("missing OIDC_CLIENT_SECRET"))?;

    let token_ttl_s: u64 = std::env::var("OIDC_TOKEN_TTL_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    let auth_code_ttl_s: u64 = std::env::var("OIDC_AUTH_CODE_TTL_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);

    let users_path: Option<PathBuf> = std::env::var("OIDC_USERS_PATH")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Into::into);

    let allowed_redirect_uris: Vec<String> = std::env::var("OIDC_ALLOWED_REDIRECT_URIS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let allow_plaintext_passwords = std::env::var("OIDC_ALLOW_PLAINTEXT_PASSWORDS")
        .ok()
        .is_some_and(|v| v == "1");

    let allow_registration = std::env::var("OIDC_ALLOW_REGISTRATION")
        .ok()
        .is_some_and(|v| v == "1");

    let allow_password_reset = std::env::var("OIDC_ALLOW_PASSWORD_RESET")
        .ok()
        .is_some_and(|v| v == "1");

    let seed = if let Ok(b64) = std::env::var("OIDC_ED25519_SEED_B64") {
        let s = b64.trim();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(s)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s))
            .context("OIDC_ED25519_SEED_B64 base64 decode (standard or urlsafe)")?;
        if raw.len() != 32 {
            anyhow::bail!(
                "OIDC_ED25519_SEED_B64 must decode to 32 bytes, got {}",
                raw.len()
            );
        }
        let mut s = [0u8; 32];
        s.copy_from_slice(&raw);
        Some(s)
    } else {
        None
    };

    let signing = if let Some(seed) = seed {
        SigningKey::from_bytes(&seed)
    } else {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| anyhow::anyhow!("getrandom: {e:?}"))?;
        warn!(
            "OIDC_ED25519_SEED_B64 not set; using ephemeral signing key (tokens won't survive restarts)"
        );
        SigningKey::from_bytes(&seed)
    };

    let der = signing.to_pkcs8_der().context("encode pkcs8 der")?;
    let pem = der
        .to_pem("PRIVATE KEY", LineEnding::LF)
        .context("encode pkcs8 pem")?
        .to_string();

    let pub_bytes = signing.verifying_key().to_bytes();
    let kid = {
        let mut h = Sha256::new();
        h.update(pub_bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
    };
    let jwk = Jwk {
        kty: "OKP".to_string(),
        crv: "Ed25519".to_string(),
        x: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pub_bytes),
        use_: "sig".to_string(),
        alg: "EdDSA".to_string(),
        kid,
    };

    Ok(Config {
        bind,
        issuer,
        client_id,
        client_secret,
        token_ttl_s,
        auth_code_ttl_s,
        users_path,
        allowed_redirect_uris,
        allow_plaintext_passwords,
        allow_registration,
        allow_password_reset,
        signing_key_pem: pem,
        jwk,
    })
}

fn basic_auth(headers: &HeaderMap) -> Option<(String, String)> {
    let h = headers.get(axum::http::header::AUTHORIZATION)?;
    let s = h.to_str().ok()?;
    let s = s.strip_prefix("Basic ")?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .ok()?;
    let raw = String::from_utf8(raw).ok()?;
    let (u, p) = raw.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

async fn discovery(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = &st.cfg;
    let d = OidcDiscovery {
        issuer: cfg.issuer.clone(),
        authorization_endpoint: format!("{}/authorize", cfg.issuer),
        token_endpoint: format!("{}/token", cfg.issuer),
        userinfo_endpoint: format!("{}/userinfo", cfg.issuer),
        jwks_uri: format!("{}/jwks.json", cfg.issuer),
        response_types_supported: vec!["code".to_string()],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec!["EdDSA".to_string()],
        token_endpoint_auth_methods_supported: vec!["client_secret_basic".to_string()],
        grant_types_supported: vec![
            "client_credentials".to_string(),
            "authorization_code".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
    };
    Json(d)
}

async fn jwks(State(st): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = &st.cfg;
    Json(Jwks {
        keys: vec![cfg.jwk.clone()],
    })
}

#[derive(Debug)]
struct AuthCode {
    created_unix: u64,
    redirect_uri: String,
    code_challenge: String,
    scope: Option<String>,
    sub: String,
    email: Option<String>,
    caps: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AuthorizeQuery {
    #[serde(default)]
    response_type: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizeForm {
    username: String,
    password: String,
    response_type: String,
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct UserDb {
    users: Vec<UserRec>,
}

#[derive(Debug, Deserialize, Serialize)]
struct UserRec {
    username: String,
    #[serde(default)]
    password_hash: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    caps: Option<Vec<String>>,
}

fn authorize_enabled(cfg: &Config) -> bool {
    cfg.users_path.is_some()
}

fn validate_redirect_uri(cfg: &Config, redirect_uri: &str) -> bool {
    if cfg.allowed_redirect_uris.is_empty() {
        return true;
    }
    cfg.allowed_redirect_uris
        .iter()
        .any(|u| u.as_str() == redirect_uri)
}

fn validate_authorize_req(cfg: &Config, q: &AuthorizeQuery) -> Result<(), String> {
    if !authorize_enabled(cfg) {
        return Err("authorize disabled (set OIDC_USERS_PATH)".to_string());
    }
    if q.response_type.as_deref() != Some("code") {
        return Err("unsupported response_type".to_string());
    }
    if q.client_id.as_deref() != Some(cfg.client_id.as_str()) {
        return Err("bad client_id".to_string());
    }
    let Some(redirect_uri) = q.redirect_uri.as_deref() else {
        return Err("missing redirect_uri".to_string());
    };
    if !validate_redirect_uri(cfg, redirect_uri) {
        return Err("redirect_uri not allowed".to_string());
    }
    let Some(chal) = q.code_challenge.as_deref() else {
        return Err("missing code_challenge".to_string());
    };
    if chal.trim().is_empty() {
        return Err("missing code_challenge".to_string());
    }
    if q.code_challenge_method.as_deref() != Some("S256") {
        return Err("unsupported code_challenge_method".to_string());
    }
    Ok(())
}

fn build_oauth_qs(q: &AuthorizeQuery) -> String {
    // Preserve the authorize query so other pages can continue the same OAuth flow.
    let mut qs = Vec::<(String, String)>::new();
    if let Some(v) = q.response_type.as_deref() {
        qs.push(("response_type".into(), v.into()));
    }
    if let Some(v) = q.client_id.as_deref() {
        qs.push(("client_id".into(), v.into()));
    }
    if let Some(v) = q.redirect_uri.as_deref() {
        qs.push(("redirect_uri".into(), v.into()));
    }
    if let Some(v) = q.scope.as_deref() {
        qs.push(("scope".into(), v.into()));
    }
    if let Some(v) = q.state.as_deref() {
        qs.push(("state".into(), v.into()));
    }
    if let Some(v) = q.code_challenge.as_deref() {
        qs.push(("code_challenge".into(), v.into()));
    }
    if let Some(v) = q.code_challenge_method.as_deref() {
        qs.push(("code_challenge_method".into(), v.into()));
    }
    if qs.is_empty() {
        return "".to_string();
    }
    let mut out = String::from("?");
    for (i, (k, v)) in qs.into_iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&urlencoding::encode(&k));
        out.push('=');
        out.push_str(&urlencoding::encode(&v));
    }
    out
}

fn login_form(cfg: &Config, q: &AuthorizeQuery, err: Option<&str>) -> Html<String> {
    let err_html = err
        .filter(|s| !s.is_empty())
        .map(|s| format!("<p style=\"color:#b00\">{}</p>", html_escape(s)))
        .unwrap_or_default();

    let hidden = |k: &str, v: Option<&str>| -> String {
        let Some(v) = v else {
            return String::new();
        };
        format!(
            "<input type=\"hidden\" name=\"{k}\" value=\"{v}\"/>",
            k = html_escape(k),
            v = html_escape(v)
        )
    };

    let reg_link = if cfg.allow_registration {
        format!(
            "<a href=\"/register{qs}\">Create account</a>",
            qs = build_oauth_qs(q)
        )
    } else {
        "".to_string()
    };
    let reset_link = if cfg.allow_password_reset {
        format!(
            "<a href=\"/reset{qs}\">Forgot password?</a>",
            qs = build_oauth_qs(q)
        )
    } else {
        "".to_string()
    };
    let links = {
        let mut xs = Vec::<String>::new();
        if !reg_link.is_empty() {
            xs.push(reg_link);
        }
        if !reset_link.is_empty() {
            xs.push(reset_link);
        }
        if xs.is_empty() {
            "".to_string()
        } else {
            format!(
                "<p style=\"margin-top:16px; display:flex; gap:14px; flex-wrap:wrap\">{}</p>",
                xs.join(" ")
            )
        }
    };

    Html(format!(
        "<!doctype html><meta charset=\"utf-8\" />\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\
<title>Sign in</title>\
<body style=\"font:16px/1.4 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Arial\">\
<h1>Sign in</h1>\
{err_html}\
<form method=\"post\" action=\"/authorize\">\
  {h_response_type}\
  {h_client_id}\
  {h_redirect_uri}\
  {h_scope}\
  {h_state}\
  {h_code_challenge}\
  {h_code_challenge_method}\
  <label>Username<br/><input name=\"username\" autocomplete=\"username\"/></label><br/><br/>\
  <label>Password<br/><input name=\"password\" type=\"password\" autocomplete=\"current-password\"/></label><br/><br/>\
  <button type=\"submit\">Continue</button>\
</form>\
{links}\
</body>",
        err_html = err_html,
        h_response_type = hidden("response_type", q.response_type.as_deref()),
        h_client_id = hidden("client_id", q.client_id.as_deref()),
        h_redirect_uri = hidden("redirect_uri", q.redirect_uri.as_deref()),
        h_scope = hidden("scope", q.scope.as_deref()),
        h_state = hidden("state", q.state.as_deref()),
        h_code_challenge = hidden("code_challenge", q.code_challenge.as_deref()),
        h_code_challenge_method =
            hidden("code_challenge_method", q.code_challenge_method.as_deref()),
        links = links,
    ))
}

async fn authorize_get(
    State(st): State<Arc<AppState>>,
    Query(q): Query<AuthorizeQuery>,
) -> impl IntoResponse {
    let cfg = &st.cfg;
    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }
    login_form(cfg, &q, None).into_response()
}

#[derive(Debug, Deserialize)]
struct RegisterQuery {
    #[serde(default)]
    response_type: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterForm {
    username: String,
    password: String,
    password2: String,
    response_type: String,
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}

fn is_valid_username(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 || s.len() > 32 {
        return false;
    }
    s.bytes().all(|b| match b {
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' => true,
        _ => false,
    })
}

fn register_form(q: &AuthorizeQuery, err: Option<&str>) -> Html<String> {
    let err_html = err
        .filter(|s| !s.is_empty())
        .map(|s| format!("<p style=\"color:#b00\">{}</p>", html_escape(s)))
        .unwrap_or_default();

    let hidden = |k: &str, v: Option<&str>| -> String {
        let Some(v) = v else {
            return String::new();
        };
        format!(
            "<input type=\"hidden\" name=\"{k}\" value=\"{v}\"/>",
            k = html_escape(k),
            v = html_escape(v)
        )
    };

    Html(format!(
        "<!doctype html><meta charset=\"utf-8\" />\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\
<title>Create account</title>\
<body style=\"font:16px/1.4 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Arial\">\
<h1>Create account</h1>\
{err_html}\
<form method=\"post\" action=\"/register\">\
  {h_response_type}\
  {h_client_id}\
  {h_redirect_uri}\
  {h_scope}\
  {h_state}\
  {h_code_challenge}\
  {h_code_challenge_method}\
  <label>Username<br/><input name=\"username\" autocomplete=\"username\"/></label><br/><br/>\
  <label>Password<br/><input name=\"password\" type=\"password\" autocomplete=\"new-password\"/></label><br/><br/>\
  <label>Password (again)<br/><input name=\"password2\" type=\"password\" autocomplete=\"new-password\"/></label><br/><br/>\
  <button type=\"submit\">Create and continue</button>\
</form>\
<p style=\"margin-top:16px\">\
  <a href=\"/authorize{auth_qs}\">Back to sign in</a>\
</p>\
</body>",
        err_html = err_html,
        h_response_type = hidden("response_type", q.response_type.as_deref()),
        h_client_id = hidden("client_id", q.client_id.as_deref()),
        h_redirect_uri = hidden("redirect_uri", q.redirect_uri.as_deref()),
        h_scope = hidden("scope", q.scope.as_deref()),
        h_state = hidden("state", q.state.as_deref()),
        h_code_challenge = hidden("code_challenge", q.code_challenge.as_deref()),
        h_code_challenge_method =
            hidden("code_challenge_method", q.code_challenge_method.as_deref()),
        auth_qs = build_oauth_qs(q),
    ))
}

async fn register_get(
    State(st): State<Arc<AppState>>,
    Query(q): Query<RegisterQuery>,
) -> impl IntoResponse {
    let cfg = &st.cfg;
    if !cfg.allow_registration {
        return (StatusCode::NOT_FOUND, "not found\n").into_response();
    }
    let q = AuthorizeQuery {
        response_type: q.response_type,
        client_id: q.client_id,
        redirect_uri: q.redirect_uri,
        scope: q.scope,
        state: q.state,
        code_challenge: q.code_challenge,
        code_challenge_method: q.code_challenge_method,
    };
    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }
    register_form(&q, None).into_response()
}

fn hash_password_phc(password: &str) -> anyhow::Result<String> {
    if password.is_empty() {
        anyhow::bail!("empty password");
    }
    let mut salt_raw = [0u8; 16];
    getrandom::getrandom(&mut salt_raw).map_err(|e| anyhow::anyhow!("getrandom: {e:?}"))?;
    let salt =
        SaltString::encode_b64(&salt_raw).map_err(|e| anyhow::anyhow!("salt encode: {e:?}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash: {e:?}"))?
        .to_string())
}

fn load_userdb(path: &PathBuf) -> anyhow::Result<UserDb> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UserDb { users: vec![] });
        }
        Err(e) => return Err(e).with_context(|| format!("read user db {path:?}")),
    };
    Ok(serde_json::from_str::<UserDb>(&raw)?)
}

fn save_userdb_atomic(path: &PathBuf, db: &UserDb) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).ok();
    let tmp = path.with_extension("tmp");
    let raw = serde_json::to_string_pretty(db)? + "\n";
    std::fs::write(&tmp, raw.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

async fn register_post(
    State(st): State<Arc<AppState>>,
    Form(f): Form<RegisterForm>,
) -> impl IntoResponse {
    let cfg = &st.cfg;
    if !cfg.allow_registration {
        return (StatusCode::NOT_FOUND, "not found\n").into_response();
    }

    let q = AuthorizeQuery {
        response_type: Some(f.response_type.clone()),
        client_id: Some(f.client_id.clone()),
        redirect_uri: Some(f.redirect_uri.clone()),
        scope: f.scope.clone(),
        state: f.state.clone(),
        code_challenge: Some(f.code_challenge.clone()),
        code_challenge_method: Some(f.code_challenge_method.clone()),
    };

    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }

    let Some(users_path) = cfg.users_path.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "authorize disabled\n".to_string(),
        )
            .into_response();
    };

    let uname = f.username.trim();
    if !is_valid_username(uname) {
        return register_form(&q, Some("bad username (use 3-32 chars: a-z, 0-9, _, -)"))
            .into_response();
    }
    if f.password != f.password2 {
        return register_form(&q, Some("passwords do not match")).into_response();
    }
    if f.password.len() < 8 {
        return register_form(&q, Some("password too short (min 8 chars)")).into_response();
    }

    // Serialize user creation to avoid clobbering the JSON file.
    let _guard = st.userdb_lock.lock().await;

    let mut db = match load_userdb(users_path) {
        Ok(d) => d,
        Err(e) => {
            warn!(err = ?e, "failed to load user db for registration");
            return (StatusCode::INTERNAL_SERVER_ERROR, "user db load failed\n").into_response();
        }
    };

    if db.users.iter().any(|u| u.username == uname) {
        return register_form(&q, Some("username already taken")).into_response();
    }

    let phc = match tokio::task::spawn_blocking({
        let pw = f.password.clone();
        move || hash_password_phc(pw.as_str())
    })
    .await
    {
        Ok(Ok(phc)) => phc,
        Ok(Err(e)) => {
            warn!(err = ?e, "password hash failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "password hash failed\n").into_response();
        }
        Err(e) => {
            warn!(err = ?e, "password hash join failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "password hash failed\n").into_response();
        }
    };

    db.users.push(UserRec {
        username: uname.to_string(),
        password_hash: Some(phc),
        password: None,
        sub: Some(uname.to_string()),
        email: None,
        caps: None,
    });

    if let Err(e) = save_userdb_atomic(users_path, &db) {
        warn!(err = ?e, "failed to save user db");
        return (StatusCode::INTERNAL_SERVER_ERROR, "user db save failed\n").into_response();
    }

    // Immediately continue the OAuth flow as the newly-created user.
    let code = match random_b64url(32) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = ?e, "code gen failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "code gen failed\n").into_response();
        }
    };

    let ac = AuthCode {
        created_unix: now_unix(),
        redirect_uri: f.redirect_uri.clone(),
        code_challenge: f.code_challenge.clone(),
        scope: f.scope.clone(),
        sub: uname.to_string(),
        email: None,
        caps: None,
    };
    {
        let mut m = st.auth_codes.lock().await;
        m.insert(code.clone(), ac);
    }

    let mut qs = format!("code={}", urlencoding::encode(&code));
    if let Some(state) = f.state.as_deref() {
        if !state.trim().is_empty() {
            qs.push_str("&state=");
            qs.push_str(&urlencoding::encode(state));
        }
    }
    let url = join_query(&f.redirect_uri, &qs);
    redirect_see_other(&url)
}

#[derive(Debug, Deserialize)]
struct ResetQuery {
    #[serde(default)]
    response_type: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResetForm {
    username: String,
    password: String,
    password2: String,
    response_type: String,
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}

fn reset_form(q: &AuthorizeQuery, err: Option<&str>) -> Html<String> {
    let err_html = err
        .filter(|s| !s.is_empty())
        .map(|s| format!("<p style=\"color:#b00\">{}</p>", html_escape(s)))
        .unwrap_or_default();

    let hidden = |k: &str, v: Option<&str>| -> String {
        let Some(v) = v else {
            return String::new();
        };
        format!(
            "<input type=\"hidden\" name=\"{k}\" value=\"{v}\"/>",
            k = html_escape(k),
            v = html_escape(v)
        )
    };

    Html(format!(
        "<!doctype html><meta charset=\"utf-8\" />\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\
<title>Reset password</title>\
<body style=\"font:16px/1.4 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Arial\">\
<h1>Reset password</h1>\
{err_html}\
<form method=\"post\" action=\"/reset\">\
  {h_response_type}\
  {h_client_id}\
  {h_redirect_uri}\
  {h_scope}\
  {h_state}\
  {h_code_challenge}\
  {h_code_challenge_method}\
  <label>Username<br/><input name=\"username\" autocomplete=\"username\"/></label><br/><br/>\
  <label>New password<br/><input name=\"password\" type=\"password\" autocomplete=\"new-password\"/></label><br/><br/>\
  <label>New password (again)<br/><input name=\"password2\" type=\"password\" autocomplete=\"new-password\"/></label><br/><br/>\
  <button type=\"submit\">Reset</button>\
</form>\
<p style=\"margin-top:16px\">\
  <a href=\"/authorize{auth_qs}\">Back to sign in</a>\
</p>\
</body>",
        err_html = err_html,
        h_response_type = hidden("response_type", q.response_type.as_deref()),
        h_client_id = hidden("client_id", q.client_id.as_deref()),
        h_redirect_uri = hidden("redirect_uri", q.redirect_uri.as_deref()),
        h_scope = hidden("scope", q.scope.as_deref()),
        h_state = hidden("state", q.state.as_deref()),
        h_code_challenge = hidden("code_challenge", q.code_challenge.as_deref()),
        h_code_challenge_method =
            hidden("code_challenge_method", q.code_challenge_method.as_deref()),
        auth_qs = build_oauth_qs(q),
    ))
}

async fn reset_get(
    State(st): State<Arc<AppState>>,
    Query(q): Query<ResetQuery>,
) -> impl IntoResponse {
    let cfg = &st.cfg;
    if !cfg.allow_password_reset {
        return (StatusCode::NOT_FOUND, "not found\n").into_response();
    }
    let q = AuthorizeQuery {
        response_type: q.response_type,
        client_id: q.client_id,
        redirect_uri: q.redirect_uri,
        scope: q.scope,
        state: q.state,
        code_challenge: q.code_challenge,
        code_challenge_method: q.code_challenge_method,
    };
    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }
    reset_form(&q, None).into_response()
}

async fn reset_post(
    State(st): State<Arc<AppState>>,
    Form(f): Form<ResetForm>,
) -> impl IntoResponse {
    let cfg = &st.cfg;
    if !cfg.allow_password_reset {
        return (StatusCode::NOT_FOUND, "not found\n").into_response();
    }

    let q = AuthorizeQuery {
        response_type: Some(f.response_type.clone()),
        client_id: Some(f.client_id.clone()),
        redirect_uri: Some(f.redirect_uri.clone()),
        scope: f.scope.clone(),
        state: f.state.clone(),
        code_challenge: Some(f.code_challenge.clone()),
        code_challenge_method: Some(f.code_challenge_method.clone()),
    };
    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }

    let Some(users_path) = cfg.users_path.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "authorize disabled\n".to_string(),
        )
            .into_response();
    };

    let uname = f.username.trim();
    if !is_valid_username(uname) {
        return reset_form(&q, Some("bad username")).into_response();
    }
    if f.password != f.password2 {
        return reset_form(&q, Some("passwords do not match")).into_response();
    }
    if f.password.len() < 8 {
        return reset_form(&q, Some("password too short (min 8 chars)")).into_response();
    }

    let _guard = st.userdb_lock.lock().await;
    let mut db = match load_userdb(users_path) {
        Ok(d) => d,
        Err(e) => {
            warn!(err = ?e, "failed to load user db for reset");
            return (StatusCode::INTERNAL_SERVER_ERROR, "user db load failed\n").into_response();
        }
    };

    let Some(u) = db.users.iter_mut().find(|u| u.username == uname) else {
        // Don't leak existence.
        return reset_form(&q, Some("reset failed")).into_response();
    };

    let phc = match tokio::task::spawn_blocking({
        let pw = f.password.clone();
        move || hash_password_phc(pw.as_str())
    })
    .await
    {
        Ok(Ok(phc)) => phc,
        Ok(Err(e)) => {
            warn!(err = ?e, "password hash failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "password hash failed\n").into_response();
        }
        Err(e) => {
            warn!(err = ?e, "password hash join failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "password hash failed\n").into_response();
        }
    };

    u.password_hash = Some(phc);
    u.password = None;

    if let Err(e) = save_userdb_atomic(users_path, &db) {
        warn!(err = ?e, "failed to save user db after reset");
        return (StatusCode::INTERNAL_SERVER_ERROR, "user db save failed\n").into_response();
    }

    login_form(cfg, &q, Some("password reset. please sign in.")).into_response()
}

fn sanitize_caps(v: Option<Vec<String>>) -> Option<Vec<String>> {
    v.map(|xs| {
        xs.into_iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .take(32)
            .collect::<Vec<_>>()
    })
    .filter(|v| !v.is_empty())
}

fn verify_password(cfg: &Config, user: &UserRec, password: &str) -> bool {
    if let Some(phc) = user.password_hash.as_deref() {
        let Ok(parsed) = PasswordHash::new(phc) else {
            warn!(username = %user.username, "bad password hash format");
            return false;
        };
        return Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
    }
    if cfg.allow_plaintext_passwords {
        if let Some(p) = user.password.as_deref() {
            return p == password;
        }
    }
    false
}

async fn authorize_post(
    State(st): State<Arc<AppState>>,
    Form(f): Form<AuthorizeForm>,
) -> impl IntoResponse {
    let cfg = &st.cfg;

    let q = AuthorizeQuery {
        response_type: Some(f.response_type.clone()),
        client_id: Some(f.client_id.clone()),
        redirect_uri: Some(f.redirect_uri.clone()),
        scope: f.scope.clone(),
        state: f.state.clone(),
        code_challenge: Some(f.code_challenge.clone()),
        code_challenge_method: Some(f.code_challenge_method.clone()),
    };

    if let Err(e) = validate_authorize_req(cfg, &q) {
        return (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response();
    }

    let Some(users_path) = cfg.users_path.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "authorize disabled\n".to_string(),
        )
            .into_response();
    };

    let db: UserDb = match std::fs::read_to_string(users_path)
        .with_context(|| format!("read user db {users_path:?}"))
        .and_then(|s| Ok(serde_json::from_str::<UserDb>(&s)?))
    {
        Ok(v) => v,
        Err(e) => {
            warn!(err = ?e, "failed to load user db");
            return (StatusCode::INTERNAL_SERVER_ERROR, "user db load failed\n").into_response();
        }
    };

    let uname = f.username.trim();
    let pass = f.password.as_str();
    let Some(user) = db.users.iter().find(|u| u.username == uname) else {
        return login_form(cfg, &q, Some("invalid username or password")).into_response();
    };
    if !verify_password(cfg, user, pass) {
        return login_form(cfg, &q, Some("invalid username or password")).into_response();
    }

    let code = match random_b64url(32) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = ?e, "code gen failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "code gen failed\n").into_response();
        }
    };

    let caps = sanitize_caps(user.caps.clone());
    let sub = user.sub.clone().unwrap_or_else(|| user.username.clone());
    let email = user.email.clone().filter(|e| e.len() <= 254);

    let ac = AuthCode {
        created_unix: now_unix(),
        redirect_uri: f.redirect_uri.clone(),
        code_challenge: f.code_challenge.clone(),
        scope: f.scope.clone(),
        sub,
        email,
        caps,
    };

    {
        let mut m = st.auth_codes.lock().await;
        m.insert(code.clone(), ac);
    }

    let mut qs = format!("code={}", urlencoding::encode(&code));
    if let Some(state) = f.state.as_deref() {
        if !state.trim().is_empty() {
            qs.push_str("&state=");
            qs.push_str(&urlencoding::encode(state));
        }
    }

    let url = join_query(&f.redirect_uri, &qs);
    redirect_see_other(&url)
}

#[derive(Debug, Serialize)]
struct UserInfoResp {
    sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    caps: Option<Vec<String>>,
}

async fn userinfo(State(st): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    let cfg = &st.cfg;

    let Some(h) = headers.get(axum::http::header::AUTHORIZATION) else {
        return (StatusCode::UNAUTHORIZED, "missing bearer token\n").into_response();
    };
    let Ok(s) = h.to_str() else {
        return (StatusCode::UNAUTHORIZED, "bad authorization header\n").into_response();
    };
    let Some(tok) = s.strip_prefix("Bearer ") else {
        return (StatusCode::UNAUTHORIZED, "bad authorization header\n").into_response();
    };
    let tok = tok.trim();
    if tok.is_empty() {
        return (StatusCode::UNAUTHORIZED, "missing bearer token\n").into_response();
    }

    // Verify using the public key (JWK x component), not the private key PEM.
    let key = match DecodingKey::from_ed_components(cfg.jwk.x.as_str()) {
        Ok(k) => k,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "bad signing key\n").into_response();
        }
    };
    let mut v = Validation::new(Algorithm::EdDSA);
    v.set_issuer(&[cfg.issuer.clone()]);
    v.set_audience(&["slopmud"]);

    let claims = match jsonwebtoken::decode::<Claims>(tok, &key, &v) {
        Ok(d) => d.claims,
        Err(_) => return (StatusCode::UNAUTHORIZED, "invalid token\n").into_response(),
    };

    Json(UserInfoResp {
        sub: claims.sub,
        email: claims.email,
        caps: sanitize_caps(claims.caps),
    })
    .into_response()
}

fn encode_jwt(cfg: &Config, claims: &Claims) -> Result<String, axum::response::Response> {
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(cfg.jwk.kid.clone());
    let key = match EncodingKey::from_ed_pem(cfg.signing_key_pem.as_bytes()) {
        Ok(k) => k,
        Err(_) => {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "bad signing key\n").into_response());
        }
    };
    jsonwebtoken::encode(&header, claims, &key)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "token encode failed\n").into_response())
}

async fn token(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(tf): Form<TokenForm>,
) -> impl IntoResponse {
    let cfg = &st.cfg;

    let Some((cid, csec)) = basic_auth(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing basic auth\n").into_response();
    };
    if cid != cfg.client_id || csec != cfg.client_secret {
        return (StatusCode::UNAUTHORIZED, "bad client credentials\n").into_response();
    }

    match tf.grant_type.as_str() {
        "client_credentials" => {
            let sub = tf.sub.unwrap_or_else(|| "anonymous".to_string());
            let now = now_unix();
            let exp = now.saturating_add(cfg.token_ttl_s);

            let claims = Claims {
                iss: cfg.issuer.clone(),
                sub,
                aud: "slopmud".to_string(),
                iat: now,
                exp,
                sid: tf.sid,
                scope: tf.scope.clone(),
                client_id: cfg.client_id.clone(),
                email: None,
                caps: None,
            };

            let jwt = match encode_jwt(cfg, &claims) {
                Ok(s) => s,
                Err(r) => return r,
            };

            Json(TokenResp {
                access_token: jwt,
                token_type: "Bearer".to_string(),
                expires_in: cfg.token_ttl_s,
                scope: tf.scope,
                id_token: None,
            })
            .into_response()
        }
        "authorization_code" => {
            let Some(code) = tf.code.as_deref() else {
                return (StatusCode::BAD_REQUEST, "missing code\n").into_response();
            };
            let Some(redirect_uri) = tf.redirect_uri.as_deref() else {
                return (StatusCode::BAD_REQUEST, "missing redirect_uri\n").into_response();
            };
            let Some(verifier) = tf.code_verifier.as_deref() else {
                return (StatusCode::BAD_REQUEST, "missing code_verifier\n").into_response();
            };

            let ac = {
                let mut m = st.auth_codes.lock().await;
                m.remove(code)
            };
            let Some(ac) = ac else {
                return (StatusCode::BAD_REQUEST, "invalid code\n").into_response();
            };

            let now = now_unix();
            if now.saturating_sub(ac.created_unix) > cfg.auth_code_ttl_s {
                return (StatusCode::BAD_REQUEST, "code expired\n").into_response();
            }
            if ac.redirect_uri != redirect_uri {
                return (StatusCode::BAD_REQUEST, "redirect_uri mismatch\n").into_response();
            }

            let expected = ac.code_challenge;
            let actual = base64url_sha256(verifier);
            if expected != actual {
                return (StatusCode::BAD_REQUEST, "pkce verify failed\n").into_response();
            }

            let exp = now.saturating_add(cfg.token_ttl_s);
            let access_claims = Claims {
                iss: cfg.issuer.clone(),
                sub: ac.sub.clone(),
                aud: "slopmud".to_string(),
                iat: now,
                exp,
                sid: None,
                scope: ac.scope.clone(),
                client_id: cfg.client_id.clone(),
                email: ac.email.clone(),
                caps: ac.caps.clone(),
            };
            let access = match encode_jwt(cfg, &access_claims) {
                Ok(s) => s,
                Err(r) => return r,
            };

            let id_token = if access_claims
                .scope
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .any(|s| s == "openid")
            {
                let mut id_claims = access_claims.clone();
                id_claims.aud = cfg.client_id.clone();
                encode_jwt(cfg, &id_claims).ok()
            } else {
                None
            };

            Json(TokenResp {
                access_token: access,
                token_type: "Bearer".to_string(),
                expires_in: cfg.token_ttl_s,
                scope: access_claims.scope,
                id_token,
            })
            .into_response()
        }
        _ => (StatusCode::BAD_REQUEST, "unsupported grant_type\n").into_response(),
    }
}

fn hash_password_cmd(password: &str) -> anyhow::Result<()> {
    if password.is_empty() {
        anyhow::bail!("empty password");
    }
    let phc = hash_password_phc(password)?;
    println!("{phc}");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,internal_oidc=info".into()),
        )
        .with_target(false)
        .with_max_level(Level::INFO)
        .init();

    // Helper subcommand for creating a password hash for the JSON user db.
    if std::env::args().nth(1).as_deref() == Some("hash-password") {
        let pw = std::env::args().nth(2).unwrap_or_default();
        if let Err(e) = hash_password_cmd(pw.as_str()) {
            eprintln!("{e:#}");
            usage_and_exit();
        }
        return Ok(());
    }

    let cfg = match parse_cfg() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e:#}");
            usage_and_exit();
        }
    };

    if cfg.users_path.is_some() && cfg.allowed_redirect_uris.is_empty() {
        warn!(
            "OIDC_USERS_PATH set but OIDC_ALLOWED_REDIRECT_URIS is empty; /authorize will accept any redirect_uri (unsafe)"
        );
    }
    if cfg.allow_plaintext_passwords {
        warn!("OIDC_ALLOW_PLAINTEXT_PASSWORDS=1; plaintext passwords accepted (dev only)");
    }
    if cfg.allow_registration {
        warn!("OIDC_ALLOW_REGISTRATION=1; self-serve registration enabled (dev only)");
    }
    if cfg.allow_password_reset {
        warn!("OIDC_ALLOW_PASSWORD_RESET=1; self-serve password reset enabled (dev only)");
    }

    let cfg = Arc::new(cfg);
    let st = Arc::new(AppState {
        cfg: cfg.clone(),
        auth_codes: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        userdb_lock: Arc::new(tokio::sync::Mutex::new(())),
    });

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks.json", get(jwks))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/register", get(register_get).post(register_post))
        .route("/reset", get(reset_get).post(reset_post))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo))
        .with_state(st.clone());

    info!(bind = %cfg.bind, issuer = %cfg.issuer, "internal oidc listening");

    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
