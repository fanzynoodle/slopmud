#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  restore_onebox_stack.sh --from-s3 s3://BUCKET/KEY --env-prefix prd
  restore_onebox_stack.sh --bucket BUCKET --track prod --env-prefix prd [--sha SHA]

Restores the one-box stack from a CI artifact:
- installs slopmud, shard_01, internal_oidc, static_web, and slopmud_web
- restores the bundled env files into /opt/slopmud/env
- issues/syncs TLS certs for enabled HTTPS web envs
- writes/starts systemd units for the broker, shard, OIDC, and enabled web services

The artifact must contain:
- bin/
- env/
- web_homepage/
EOF
}

if [[ "${EUID:-$(id -u)}" != "0" ]]; then
  echo "ERROR: must run as root" >&2
  exit 2
fi

from_s3=""
bucket=""
track=""
sha=""
env_prefix=""
install_root="/opt/slopmud"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from-s3)
      from_s3="${2:-}"; shift 2 ;;
    --bucket)
      bucket="${2:-}"; shift 2 ;;
    --track)
      track="${2:-}"; shift 2 ;;
    --sha)
      sha="${2:-}"; shift 2 ;;
    --env-prefix)
      env_prefix="${2:-}"; shift 2 ;;
    --install-root)
      install_root="${2:-}"; shift 2 ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown arg: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$env_prefix" ]]; then
  echo "ERROR: --env-prefix is required" >&2
  usage
  exit 2
fi

if [[ -n "$from_s3" && ( -n "$bucket" || -n "$track" || -n "$sha" ) ]]; then
  echo "ERROR: use either --from-s3 or --bucket/--track[/--sha]" >&2
  exit 2
fi

if [[ -z "$from_s3" ]]; then
  if [[ -z "$bucket" || -z "$track" ]]; then
    echo "ERROR: --bucket and --track are required when --from-s3 is not provided" >&2
    usage
    exit 2
  fi
fi

wait_for_listen() {
  local port="$1"
  local label="$2"
  local i
  for i in $(seq 1 80); do
    if ss -lnt | grep -qE ":${port}\\b"; then
      return 0
    fi
    sleep 0.25
  done
  echo "not listening on ${label}" >&2
  return 1
}

append_unit_env() {
  local unit="$1"
  local var="$2"
  local value="${!var-}"

  if [[ -z "${value}" ]]; then
    return 0
  fi

  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf 'Environment="%s=%s"\n' "$var" "$value" >>"$unit"
}

ensure_packages() {
  if command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -y
    apt-get install -y awscli ca-certificates certbot python3-certbot-dns-route53 rsync curl
  elif command -v dnf >/dev/null 2>&1; then
    dnf -y install awscli ca-certificates certbot python3-certbot-dns-route53 rsync curl || {
      dnf -y install awscli ca-certificates certbot python3-pip rsync curl
      python3 -m pip install certbot-dns-route53
    }
  else
    echo "ERROR: unsupported OS (need apt-get or dnf)" >&2
    exit 2
  fi
}

ensure_slopmud_user() {
  if ! id -u slopmud >/dev/null 2>&1; then
    useradd --system --home "$install_root" --create-home --shell /usr/sbin/nologin slopmud
  fi

  install -d -m 0755 \
    "$install_root" \
    "$install_root/bin" \
    "$install_root/assets" \
    "$install_root/env" \
    "$install_root/locks"
  chown -R slopmud:slopmud "$install_root"
}

latest_s3_uri() {
  local latest_key
  latest_key="$(aws s3api list-objects-v2 \
    --bucket "$bucket" \
    --prefix "${track}/" \
    --query 'reverse(sort_by(Contents,&LastModified))[?ends_with(Key, `artifact.tgz`)].Key | [0]' \
    --output text)"
  if [[ -z "$latest_key" || "$latest_key" == "None" ]]; then
    echo "ERROR: no artifact found under s3://${bucket}/${track}/" >&2
    exit 1
  fi
  printf 's3://%s/%s\n' "$bucket" "$latest_key"
}

resolve_s3_uri() {
  if [[ -n "$from_s3" ]]; then
    printf '%s\n' "$from_s3"
    return 0
  fi

  if [[ -n "$sha" ]]; then
    printf 's3://%s/%s/%s/artifact.tgz\n' "$bucket" "$track" "$sha"
    return 0
  fi

  latest_s3_uri
}

extract_sha_from_s3_uri() {
  local s3_uri="$1"
  local tmp="${s3_uri#s3://}"
  local key="${tmp#*/}"
  basename "$(dirname "$key")"
}

install_binary() {
  local src="$1"
  local dst="$2"
  install -d -m 0755 "$(dirname "$dst")"
  install -m 0755 -o root -g root "$src" "$dst"
}

install_web_root() {
  local dst="$1"
  install -d -m 0755 "$dst"
  rsync -a --delete "${assets_dir}/web_homepage/" "${dst}/"
  chown -R slopmud:slopmud "$dst"
}

sync_env_bundle() {
  if [[ ! -d "${assets_dir}/env" ]]; then
    echo "ERROR: artifact is missing env/" >&2
    exit 2
  fi

  rsync -a --delete "${assets_dir}/env/" "${install_root}/env/"
  chown -R slopmud:slopmud "${install_root}/env"
}

ensure_hook_binary() {
  install -d -m 0755 /usr/local/bin /etc/letsencrypt/renewal-hooks/deploy
  cat >/usr/local/bin/slopmud-certbot-sync <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail

: "${CERTBOT_CERT_NAME:?missing CERTBOT_CERT_NAME}"
: "${TLS_DST_DIR:?missing TLS_DST_DIR}"
: "${WEB_SERVICE_NAME:?missing WEB_SERVICE_NAME}"

lineage="/etc/letsencrypt/live/${CERTBOT_CERT_NAME}"
install -d -o slopmud -g slopmud -m 0750 "${TLS_DST_DIR}"
install -o slopmud -g slopmud -m 0640 "${lineage}/fullchain.pem" "${TLS_DST_DIR}/fullchain.pem"
install -o slopmud -g slopmud -m 0640 "${lineage}/privkey.pem" "${TLS_DST_DIR}/privkey.pem"
systemctl restart "${WEB_SERVICE_NAME}" 2>/dev/null || true
SCRIPT
  chmod 0755 /usr/local/bin/slopmud-certbot-sync
}

cert_domains_args() {
  local domains="$1"
  local out=()
  local d

  for d in $domains; do
    out+=("-d" "$d")
  done

  printf '%s\n' "${out[@]}"
}

issue_or_sync_cert_for_env() {
  local env_file="$1"
  local cert_name=""
  local cert_domains=""
  local dst_dir=""
  local service_name=""
  local marker_key=""

  # shellcheck disable=SC1090
  source "$env_file"

  if [[ "${ENABLED:-1}" != "1" || -z "${HTTPS_BIND:-}" ]]; then
    return 0
  fi

  cert_name="${CERTBOT_CERT_NAME:-${DOMAIN:-}}"
  cert_domains="${CERTBOT_DOMAINS:-${DOMAIN:-}}"
  dst_dir="${TLS_DST_DIR:-/etc/slopmud/tls}"
  service_name="${WEB_SERVICE_NAME:-slopmud-web}"

  if [[ -z "$cert_name" || -z "$cert_domains" ]]; then
    echo "WARN: ${env_file} has HTTPS enabled but no CERTBOT_CERT_NAME/CERTBOT_DOMAINS; skipping cert setup" >&2
    return 0
  fi

  marker_key="cert:${cert_name}"
  if [[ -z "${issued_certs[$marker_key]:-}" ]]; then
    mapfile -t cert_args < <(cert_domains_args "$cert_domains")
    certbot certonly \
      --dns-route53 \
      --non-interactive \
      --agree-tos \
      --register-unsafely-without-email \
      --cert-name "$cert_name" \
      --keep-until-expiring \
      "${cert_args[@]}"
    issued_certs[$marker_key]="1"
  fi

  install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
  cat >"/etc/letsencrypt/renewal-hooks/deploy/${service_name}.sh" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export CERTBOT_CERT_NAME=${cert_name}
export TLS_DST_DIR=${dst_dir}
export WEB_SERVICE_NAME=${service_name}
exec /usr/local/bin/slopmud-certbot-sync
EOF
  chmod 0755 "/etc/letsencrypt/renewal-hooks/deploy/${service_name}.sh"

  CERTBOT_CERT_NAME="$cert_name" TLS_DST_DIR="$dst_dir" WEB_SERVICE_NAME="$service_name" /usr/local/bin/slopmud-certbot-sync
}

write_shard_unit() {
  local env_file="$1"
  local unit_path=""
  local exec_start=""

  # shellcheck disable=SC1090
  source "$env_file"

  : "${SHARD_APP_NAME:?missing SHARD_APP_NAME in ${env_file}}"
  : "${SHARD_REMOTE_BIN:?missing SHARD_REMOTE_BIN in ${env_file}}"
  : "${SHARD_BIND:?missing SHARD_BIND in ${env_file}}"

  install_binary "${assets_dir}/bin/shard_01" "$SHARD_REMOTE_BIN"
  unit_path="/etc/systemd/system/${SHARD_APP_NAME}.service"

  cat >"$unit_path" <<EOF
[Unit]
Description=slopmud shard_01 (restored from ${artifact_sha})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${install_root}
Environment=RUST_LOG=shard_01=info
EOF

  append_unit_env "$unit_path" "SHARD_BIND"
  append_unit_env "$unit_path" "OPENAI_API_BASE"
  append_unit_env "$unit_path" "OPENAI_PING_MODEL"
  append_unit_env "$unit_path" "OPENAI_API_KEY_SSM"

  exec_start="$SHARD_REMOTE_BIN"
  if [[ -n "${OPENAI_API_KEY_SSM:-}" ]]; then
    exec_start="/bin/bash -ceu 'export OPENAI_API_KEY=\"\$(aws ssm get-parameter --region us-east-1 --name \"\${OPENAI_API_KEY_SSM}\" --with-decryption --query Parameter.Value --output text)\"; exec \"${SHARD_REMOTE_BIN}\";'"
  fi

  cat >>"$unit_path" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

  shard_service_name="$SHARD_APP_NAME"
  shard_port="${SHARD_BIND##*:}"
}

write_broker_unit() {
  local env_file="$1"
  local unit_path=""

  # shellcheck disable=SC1090
  source "$env_file"

  : "${SLOPMUD_APP_NAME:?missing SLOPMUD_APP_NAME in ${env_file}}"
  : "${SLOPMUD_REMOTE_BIN:?missing SLOPMUD_REMOTE_BIN in ${env_file}}"
  : "${SLOPMUD_BIND:?missing SLOPMUD_BIND in ${env_file}}"
  : "${NODE_ID:?missing NODE_ID in ${env_file}}"

  install_binary "${assets_dir}/bin/slopmud" "$SLOPMUD_REMOTE_BIN"
  unit_path="/etc/systemd/system/${SLOPMUD_APP_NAME}.service"

  cat >"$unit_path" <<EOF
[Unit]
Description=slopmud broker (restored from ${artifact_sha})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${install_root}
Environment=RUST_LOG=slopmud=info
EOF

  append_unit_env "$unit_path" "NODE_ID"
  append_unit_env "$unit_path" "SLOPMUD_BIND"
  if [[ -n "${SHARD_ADDR:-}" ]]; then
    append_unit_env "$unit_path" "SHARD_ADDR"
  elif [[ -n "${SHARD_BIND:-}" ]]; then
    local saved_shard_addr="${SHARD_BIND}"
    SHARD_ADDR="$saved_shard_addr"
    append_unit_env "$unit_path" "SHARD_ADDR"
    unset SHARD_ADDR
  fi
  append_unit_env "$unit_path" "SLOPMUD_OIDC_TOKEN_URL"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_CLIENT_ID"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_CLIENT_SECRET"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SCOPE"
  append_unit_env "$unit_path" "SLOPMUD_GOOGLE_OAUTH_DIR"
  append_unit_env "$unit_path" "SLOPMUD_GOOGLE_AUTH_BASE_URL"
  append_unit_env "$unit_path" "SLOPMUD_ACCOUNTS_PATH"
  append_unit_env "$unit_path" "SLOPMUD_LOCALE"
  append_unit_env "$unit_path" "SLOPMUD_ADMIN_BIND"
  append_unit_env "$unit_path" "SLOPMUD_BANS_PATH"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_ENABLED"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_SPOOL_DIR"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_S3_BUCKET"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_S3_PREFIX"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_UPLOAD_ENABLED"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S"

  cat >>"$unit_path" <<EOF
ExecStart=${SLOPMUD_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

  broker_service_name="$SLOPMUD_APP_NAME"
  broker_port="${SLOPMUD_BIND##*:}"
}

write_oidc_unit() {
  local env_file="$1"
  local unit_path=""

  # shellcheck disable=SC1090
  source "$env_file"

  if [[ -z "${OIDC_APP_NAME:-}" || -z "${OIDC_REMOTE_BIN:-}" || -z "${OIDC_BIND:-}" ]]; then
    return 0
  fi

  install_binary "${assets_dir}/bin/internal_oidc" "$OIDC_REMOTE_BIN"
  unit_path="/etc/systemd/system/${OIDC_APP_NAME}.service"

  cat >"$unit_path" <<EOF
[Unit]
Description=internal_oidc (restored from ${artifact_sha})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${install_root}
Environment=RUST_LOG=internal_oidc=info
EOF

  append_unit_env "$unit_path" "OIDC_BIND"
  append_unit_env "$unit_path" "OIDC_HTTPS_BIND"
  append_unit_env "$unit_path" "OIDC_TLS_CERT"
  append_unit_env "$unit_path" "OIDC_TLS_KEY"
  append_unit_env "$unit_path" "OIDC_ISSUER"
  append_unit_env "$unit_path" "OIDC_CLIENT_ID"
  append_unit_env "$unit_path" "OIDC_CLIENT_SECRET"
  append_unit_env "$unit_path" "OIDC_ED25519_SEED_B64"
  append_unit_env "$unit_path" "OIDC_TOKEN_TTL_S"
  append_unit_env "$unit_path" "OIDC_AUTH_CODE_TTL_S"
  append_unit_env "$unit_path" "OIDC_USERS_PATH"
  append_unit_env "$unit_path" "OIDC_ALLOWED_REDIRECT_URIS"
  append_unit_env "$unit_path" "OIDC_ALLOW_PLAINTEXT_PASSWORDS"
  append_unit_env "$unit_path" "OIDC_ALLOW_REGISTRATION"
  append_unit_env "$unit_path" "OIDC_ALLOW_PASSWORD_RESET"

  cat >>"$unit_path" <<EOF
ExecStart=${OIDC_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

  oidc_service_name="$OIDC_APP_NAME"
}

write_web_unit() {
  local env_file="$1"
  local unit_path=""
  local bin_src=""
  local service_name=""
  local exec_start=""
  local service_port=""

  # shellcheck disable=SC1090
  source "$env_file"

  if [[ "${ENABLED:-1}" != "1" ]]; then
    return 0
  fi

  : "${REMOTE_BIN:?missing REMOTE_BIN in ${env_file}}"
  : "${REMOTE_WEB:?missing REMOTE_WEB in ${env_file}}"
  : "${HTTP_BIND:?missing HTTP_BIND in ${env_file}}"

  case "$(basename "$REMOTE_BIN")" in
    slopmud_web)
      bin_src="${assets_dir}/bin/slopmud_web"
      ;;
    *)
      bin_src="${assets_dir}/bin/static_web"
      ;;
  esac

  install_binary "$bin_src" "$REMOTE_BIN"
  install_web_root "$REMOTE_WEB"

  service_name="${WEB_SERVICE_NAME:-slopmud-web}"
  unit_path="/etc/systemd/system/${service_name}.service"

  cat >"$unit_path" <<EOF
[Unit]
Description=slopmud web service (restored from ${artifact_sha})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${install_root}
EOF

  BIND="$HTTP_BIND"
  append_unit_env "$unit_path" "BIND"
  unset BIND
  STATIC_DIR="$REMOTE_WEB"
  append_unit_env "$unit_path" "STATIC_DIR"
  unset STATIC_DIR
  append_unit_env "$unit_path" "HTTPS_BIND"
  append_unit_env "$unit_path" "TLS_CERT"
  append_unit_env "$unit_path" "TLS_KEY"
  append_unit_env "$unit_path" "SESSION_TCP_ADDR"
  append_unit_env "$unit_path" "SLOPMUD_GOOGLE_OAUTH_DIR"
  append_unit_env "$unit_path" "GOOGLE_OAUTH_CLIENT_ID"
  append_unit_env "$unit_path" "GOOGLE_OAUTH_CLIENT_SECRET"
  append_unit_env "$unit_path" "GOOGLE_OAUTH_REDIRECT_URI"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_AUTH_URL"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_TOKEN_URL"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_USERINFO_URL"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_CLIENT_ID"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_CLIENT_SECRET"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_REDIRECT_URI"
  append_unit_env "$unit_path" "SLOPMUD_OIDC_SSO_SCOPE"
  append_unit_env "$unit_path" "GOOGLE_OAUTH_CLIENT_ID_SSM"
  append_unit_env "$unit_path" "GOOGLE_OAUTH_CLIENT_SECRET_SSM"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_ENABLED"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_EMAIL_MODE"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_EMAIL_FROM"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SMTP_HOST"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SMTP_PORT"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SMTP_USERNAME"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SMTP_PASSWORD"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_SESSION_TTL_S"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_PRESIGN_TTL_S"
  append_unit_env "$unit_path" "SLOPMUD_COMPLIANCE_LOOKBACK_DAYS"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_S3_BUCKET"
  append_unit_env "$unit_path" "SLOPMUD_EVENTLOG_S3_PREFIX"
  append_unit_env "$unit_path" "SLOPMUD_ACCOUNTS_PATH"
  append_unit_env "$unit_path" "SLOPMUD_ADMIN_ADDR"

  exec_start="$REMOTE_BIN"
  if [[ -n "${GOOGLE_OAUTH_CLIENT_ID_SSM:-}" || -n "${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}" ]]; then
    exec_start="/bin/bash -ceu ' \
      if [[ -n \"\${GOOGLE_OAUTH_CLIENT_ID_SSM:-}\" ]]; then \
        export GOOGLE_OAUTH_CLIENT_ID=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_ID_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
      fi; \
      if [[ -n \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}\" ]]; then \
        export GOOGLE_OAUTH_CLIENT_SECRET=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
      fi; \
      if [[ -n \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}\" ]]; then \
        export SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
      fi; \
      if [[ -n \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}\" ]]; then \
        export SLOPMUD_COMPLIANCE_SMTP_PASSWORD=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
      fi; \
      exec \"${REMOTE_BIN}\"; \
    '"
  fi

  cat >>"$unit_path" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

  service_port="${HTTP_BIND##*:}"
  web_services+=("${service_name}:${service_port}")
}

resolve_s3_uri_value="$(resolve_s3_uri)"
artifact_sha="$(extract_sha_from_s3_uri "$resolve_s3_uri_value")"
assets_dir="${install_root}/assets/${track:-bootstrap}/${artifact_sha}"

ensure_packages
ensure_slopmud_user
ensure_hook_binary

install -d -m 0755 "$assets_dir"
aws s3 cp "$resolve_s3_uri_value" "${assets_dir}/artifact.tgz"
tar -xzf "${assets_dir}/artifact.tgz" -C "$assets_dir"
sync_env_bundle

primary_env="${install_root}/env/${env_prefix}.env"
if [[ ! -f "$primary_env" ]]; then
  echo "ERROR: expected primary env bundle at ${primary_env}" >&2
  exit 2
fi

declare -A issued_certs=()
declare -a web_services=()
broker_service_name=""
broker_port=""
shard_service_name=""
shard_port=""
oidc_service_name=""

write_shard_unit "$primary_env"
write_broker_unit "$primary_env"
write_oidc_unit "$primary_env"

env_files=()
if [[ -f "${install_root}/env/${env_prefix}.env" ]]; then
  env_files+=("${install_root}/env/${env_prefix}.env")
fi
shopt -s nullglob
for env_file in "${install_root}/env/${env_prefix}-"*.env; do
  env_files+=("$env_file")
done
shopt -u nullglob

if [[ "${#env_files[@]}" -eq 0 ]]; then
  echo "ERROR: no env files matched ${install_root}/env/${env_prefix}.env or ${install_root}/env/${env_prefix}-*.env" >&2
  exit 2
fi

for env_file in "${env_files[@]}"; do
  issue_or_sync_cert_for_env "$env_file"
done

for env_file in "${env_files[@]}"; do
  write_web_unit "$env_file"
done

systemctl daemon-reload

if [[ -n "$shard_service_name" ]]; then
  systemctl enable --now "${shard_service_name}.service"
  systemctl restart "${shard_service_name}.service"
  wait_for_listen "$shard_port" "$shard_service_name"
fi

if [[ -n "$broker_service_name" ]]; then
  systemctl enable --now "${broker_service_name}.service"
  systemctl restart "${broker_service_name}.service"
  wait_for_listen "$broker_port" "$broker_service_name"
fi

if [[ -n "$oidc_service_name" ]]; then
  systemctl enable --now "${oidc_service_name}.service"
  systemctl restart "${oidc_service_name}.service"
fi

for entry in "${web_services[@]}"; do
  service_name="${entry%%:*}"
  service_port="${entry##*:}"
  systemctl enable --now "${service_name}.service"
  systemctl restart "${service_name}.service"
  wait_for_listen "$service_port" "$service_name"
done

echo "OK: restored one-box stack from ${resolve_s3_uri_value}"
