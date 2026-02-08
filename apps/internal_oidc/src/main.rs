use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use ed25519_dalek::SigningKey;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use pkcs8::{EncodePrivateKey, LineEnding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{Level, info, warn};

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    issuer: String,
    client_id: String,
    client_secret: String,
    token_ttl_s: u64,
    // PKCS8 PEM, because jsonwebtoken wants PEM for EdDSA.
    signing_key_pem: String,
    jwk: Jwk,
}

#[derive(Clone, Debug, Serialize)]
struct OidcDiscovery {
    issuer: String,
    token_endpoint: String,
    jwks_uri: String,
    response_types_supported: Vec<String>,
    subject_types_supported: Vec<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Vec<String>,
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
    grant_type: String,
    // Internal extension for this service:
    // the broker can request a token for a specific session/name without sending any password.
    sub: Option<String>,
    sid: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenResp {
    access_token: String,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
    // Custom claims.
    sid: Option<String>,
    scope: Option<String>,
    client_id: String,
}

fn usage_and_exit() -> ! {
    eprintln!(
        "internal_oidc (minimal internal OIDC token issuer)\n\n\
USAGE:\n  internal_oidc\n\n\
ENV:\n  OIDC_BIND              default 127.0.0.1:9000\n  OIDC_ISSUER            default http://127.0.0.1:9000\n  OIDC_CLIENT_ID         required\n  OIDC_CLIENT_SECRET     required\n  OIDC_TOKEN_TTL_S       default 3600\n  OIDC_ED25519_SEED_B64  optional; 32 bytes seed (base64). If omitted, generates an ephemeral key.\n"
    );
    std::process::exit(2);
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

async fn discovery(State(cfg): State<Arc<Config>>) -> impl IntoResponse {
    let d = OidcDiscovery {
        issuer: cfg.issuer.clone(),
        token_endpoint: format!("{}/token", cfg.issuer),
        jwks_uri: format!("{}/jwks.json", cfg.issuer),
        response_types_supported: vec!["token".to_string()],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec!["EdDSA".to_string()],
        token_endpoint_auth_methods_supported: vec!["client_secret_basic".to_string()],
    };
    Json(d)
}

async fn jwks(State(cfg): State<Arc<Config>>) -> impl IntoResponse {
    Json(Jwks {
        keys: vec![cfg.jwk.clone()],
    })
}

async fn token(
    State(cfg): State<Arc<Config>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some((cid, csec)) = basic_auth(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing basic auth\n").into_response();
    };
    if cid != cfg.client_id || csec != cfg.client_secret {
        return (StatusCode::UNAUTHORIZED, "bad client credentials\n").into_response();
    }

    let tf = TokenForm {
        grant_type: form.get("grant_type").cloned().unwrap_or_default(),
        sub: form.get("sub").cloned(),
        sid: form.get("sid").cloned(),
        scope: form.get("scope").cloned(),
    };

    if tf.grant_type != "client_credentials" {
        return (StatusCode::BAD_REQUEST, "unsupported grant_type\n").into_response();
    }

    let sub = tf.sub.unwrap_or_else(|| "anonymous".to_string());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
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
    };

    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some(cfg.jwk.kid.clone());
    let key = match EncodingKey::from_ed_pem(cfg.signing_key_pem.as_bytes()) {
        Ok(k) => k,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "bad signing key\n").into_response(),
    };

    let jwt = match jsonwebtoken::encode(&header, &claims, &key) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "token encode failed\n").into_response();
        }
    };

    Json(TokenResp {
        access_token: jwt,
        token_type: "Bearer".to_string(),
        expires_in: cfg.token_ttl_s,
        scope: tf.scope,
    })
    .into_response()
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

    let cfg = match parse_cfg() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e:#}");
            usage_and_exit();
        }
    };

    let cfg = Arc::new(cfg);
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks.json", get(jwks))
        .route("/token", post(token))
        .with_state(cfg.clone());

    info!(bind = %cfg.bind, issuer = %cfg.issuer, "internal oidc listening");

    let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
