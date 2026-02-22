#!/usr/bin/env bash
set -euo pipefail

env_file="${1:-env/prd.env}"
if [[ ! -f "$env_file" ]]; then
  echo "ERROR: env file not found: $env_file" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

: "${HOST:?missing HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
: "${REMOTE_BIN:?missing REMOTE_BIN in env file}"
: "${REMOTE_WEB:?missing REMOTE_WEB in env file}"
: "${DOMAIN:?missing DOMAIN in env file}"
: "${HTTP_BIND:?missing HTTP_BIND in env file}"

# Optional HTTPS (served directly by slopmud_web via rustls)
HTTPS_BIND="${HTTPS_BIND:-}"
TLS_CERT="${TLS_CERT:-}"
TLS_KEY="${TLS_KEY:-}"
SESSION_TCP_ADDR="${SESSION_TCP_ADDR:-}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$REMOTE_BIN")"

echo "Building slopmud_web (release)"
./scripts/build_bookworm_release.sh slopmud_web

bin_src="target/release/slopmud_web"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi

echo "Provisioning remote directories + system user"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y rsync ca-certificates awscli; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install rsync ca-certificates awscli; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  if ! id -u slopmud >/dev/null 2>&1; then \
    sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
  fi; \
  sudo mkdir -p \"${REMOTE_ROOT}\" \"${remote_bin_dir}\" \"${REMOTE_WEB}\" \"${REMOTE_ROOT}/env\"; \
  sudo chown -R slopmud:slopmud \"${REMOTE_ROOT}\" \
"

echo "Uploading web_homepage -> ${SSH_USER}@${HOST}:${REMOTE_WEB}"
rsync -rz --delete --exclude README.md --rsync-path="sudo rsync" -e "ssh ${ssh_opts[*]} ${ssh_port_opt[*]}" web_homepage/ "${SSH_USER}@${HOST}:${REMOTE_WEB}/"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo chown -R slopmud:slopmud \"${REMOTE_WEB}\" \
"

echo "Uploading binary -> ${SSH_USER}@${HOST}:${REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/slopmud_web"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/slopmud_web \"${REMOTE_BIN}\"; \
  sudo rm -f /tmp/slopmud_web \
"

tmp_unit="$(mktemp)"
trap 'rm -f "$tmp_unit"' EXIT
cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud static web server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=BIND=${HTTP_BIND}
Environment=STATIC_DIR=${REMOTE_WEB}
Environment=HTTPS_BIND=${HTTPS_BIND}
Environment=TLS_CERT=${TLS_CERT}
Environment=TLS_KEY=${TLS_KEY}
EOF

if [[ -n "${SESSION_TCP_ADDR}" ]]; then
  # /ws (web client) -> session broker port. Keep envs isolated on a shared host.
  echo "Environment=SESSION_TCP_ADDR=${SESSION_TCP_ADDR}" >>"$tmp_unit"
fi

# Optional: Google SSO (requires HTTPS + correct redirect URI registered with Google).
if [[ -n "${SLOPMUD_GOOGLE_OAUTH_DIR:-}" ]]; then
  echo "Environment=SLOPMUD_GOOGLE_OAUTH_DIR=${SLOPMUD_GOOGLE_OAUTH_DIR}" >>"$tmp_unit"
fi
if [[ -n "${GOOGLE_OAUTH_CLIENT_ID:-}" ]]; then
  echo "Environment=GOOGLE_OAUTH_CLIENT_ID=${GOOGLE_OAUTH_CLIENT_ID}" >>"$tmp_unit"
fi
if [[ -n "${GOOGLE_OAUTH_CLIENT_SECRET:-}" ]]; then
  echo "Environment=GOOGLE_OAUTH_CLIENT_SECRET=${GOOGLE_OAUTH_CLIENT_SECRET}" >>"$tmp_unit"
fi
if [[ -n "${GOOGLE_OAUTH_REDIRECT_URI:-}" ]]; then
  echo "Environment=GOOGLE_OAUTH_REDIRECT_URI=${GOOGLE_OAUTH_REDIRECT_URI}" >>"$tmp_unit"
fi

# Optional: custom OIDC SSO (slopsso).
if [[ -n "${SLOPMUD_OIDC_SSO_AUTH_URL:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_AUTH_URL=${SLOPMUD_OIDC_SSO_AUTH_URL}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_TOKEN_URL:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_TOKEN_URL=${SLOPMUD_OIDC_SSO_TOKEN_URL}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_USERINFO_URL:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_USERINFO_URL=${SLOPMUD_OIDC_SSO_USERINFO_URL}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_CLIENT_ID:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_CLIENT_ID=${SLOPMUD_OIDC_SSO_CLIENT_ID}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_CLIENT_SECRET:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_CLIENT_SECRET=${SLOPMUD_OIDC_SSO_CLIENT_SECRET}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_REDIRECT_URI:-}" ]]; then
  echo "Environment=SLOPMUD_OIDC_SSO_REDIRECT_URI=${SLOPMUD_OIDC_SSO_REDIRECT_URI}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_OIDC_SSO_SCOPE:-}" ]]; then
  echo "Environment=\"SLOPMUD_OIDC_SSO_SCOPE=${SLOPMUD_OIDC_SSO_SCOPE}\"" >>"$tmp_unit"
fi

# If the env file references SSM parameter names, fetch them at service start on the instance using its IAM role.
if [[ -n "${GOOGLE_OAUTH_CLIENT_ID_SSM:-}" ]]; then
  echo "Environment=GOOGLE_OAUTH_CLIENT_ID_SSM=${GOOGLE_OAUTH_CLIENT_ID_SSM}" >>"$tmp_unit"
fi
if [[ -n "${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}" ]]; then
  echo "Environment=GOOGLE_OAUTH_CLIENT_SECRET_SSM=${GOOGLE_OAUTH_CLIENT_SECRET_SSM}" >>"$tmp_unit"
fi

# Optional: Compliance portal (access-key login, log downloads, bans, transparency log).
if [[ -n "${SLOPMUD_COMPLIANCE_ENABLED:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_ENABLED=${SLOPMUD_COMPLIANCE_ENABLED}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM=${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON=${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_EMAIL_MODE:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_EMAIL_MODE=${SLOPMUD_COMPLIANCE_EMAIL_MODE}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_EMAIL_FROM:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_EMAIL_FROM=${SLOPMUD_COMPLIANCE_EMAIL_FROM}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SMTP_HOST:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SMTP_HOST=${SLOPMUD_COMPLIANCE_SMTP_HOST}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PORT:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PORT=${SLOPMUD_COMPLIANCE_SMTP_PORT}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SMTP_USERNAME:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SMTP_USERNAME=${SLOPMUD_COMPLIANCE_SMTP_USERNAME}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PASSWORD=${SLOPMUD_COMPLIANCE_SMTP_PASSWORD}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM=${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED=${SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL=${SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_SESSION_TTL_S:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_SESSION_TTL_S=${SLOPMUD_COMPLIANCE_SESSION_TTL_S}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_PRESIGN_TTL_S:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_PRESIGN_TTL_S=${SLOPMUD_COMPLIANCE_PRESIGN_TTL_S}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_COMPLIANCE_LOOKBACK_DAYS:-}" ]]; then
  echo "Environment=SLOPMUD_COMPLIANCE_LOOKBACK_DAYS=${SLOPMUD_COMPLIANCE_LOOKBACK_DAYS}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_EVENTLOG_S3_BUCKET:-}" ]]; then
  echo "Environment=SLOPMUD_EVENTLOG_S3_BUCKET=${SLOPMUD_EVENTLOG_S3_BUCKET}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_EVENTLOG_S3_PREFIX:-}" ]]; then
  echo "Environment=SLOPMUD_EVENTLOG_S3_PREFIX=${SLOPMUD_EVENTLOG_S3_PREFIX}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_ACCOUNTS_PATH:-}" ]]; then
  echo "Environment=SLOPMUD_ACCOUNTS_PATH=${SLOPMUD_ACCOUNTS_PATH}" >>"$tmp_unit"
fi
if [[ -n "${SLOPMUD_ADMIN_ADDR:-}" ]]; then
  echo "Environment=SLOPMUD_ADMIN_ADDR=${SLOPMUD_ADMIN_ADDR}" >>"$tmp_unit"
fi

exec_start="${REMOTE_BIN}"
if [[ -n "${GOOGLE_OAUTH_CLIENT_ID_SSM:-}" || -n "${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}" ]]; then
  exec_start="/bin/bash -ceu ' \
    if [[ -n \"\${GOOGLE_OAUTH_CLIENT_ID_SSM}\" ]]; then \
      export GOOGLE_OAUTH_CLIENT_ID=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_ID_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM}\" ]]; then \
      export GOOGLE_OAUTH_CLIENT_SECRET=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}\" ]]; then \
      export SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}\" ]]; then \
      export SLOPMUD_COMPLIANCE_SMTP_PASSWORD=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    exec \"${REMOTE_BIN}\"; \
  '"
fi

cat >>"$tmp_unit" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

service_name="${WEB_SERVICE_NAME:-slopmud-web}"
unit_name="${service_name}.service"

echo "Installing systemd unit (${service_name}) + stopping nginx if present"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_unit" "${SSH_USER}@${HOST}:/tmp/${unit_name}"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo mv /tmp/${unit_name} /etc/systemd/system/${unit_name}; \
  sudo systemctl daemon-reload; \
  sudo systemctl disable --now nginx 2>/dev/null || true; \
  sudo systemctl enable --now ${service_name}; \
  sudo systemctl restart ${service_name}; \
  sudo systemctl --no-pager --full status ${service_name} || true \
"

http_port="${HTTP_BIND##*:}"
echo "Waiting for health check (http) ..."
for _ in {1..40}; do
  if curl -fsS -H "Host: ${DOMAIN}" "http://${HOST}:${http_port}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done

echo "Smoke test (direct IP, Host header = ${DOMAIN}, port = ${http_port})"
curl -fsSL -H "Host: ${DOMAIN}" "http://${HOST}:${http_port}/" | sed -n '1,25p'

echo "Health check (http)"
curl -fsSL -H "Host: ${DOMAIN}" "http://${HOST}:${http_port}/healthz" || true
