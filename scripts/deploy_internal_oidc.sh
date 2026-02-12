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
: "${OIDC_APP_NAME:?missing OIDC_APP_NAME in env file}"
: "${OIDC_REMOTE_BIN:?missing OIDC_REMOTE_BIN in env file}"
: "${OIDC_BIND:?missing OIDC_BIND in env file}"
: "${OIDC_ISSUER:?missing OIDC_ISSUER in env file}"
: "${OIDC_CLIENT_ID:?missing OIDC_CLIENT_ID in env file}"
: "${OIDC_CLIENT_SECRET:?missing OIDC_CLIENT_SECRET in env file}"
: "${OIDC_ED25519_SEED_B64:?missing OIDC_ED25519_SEED_B64 in env file}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$OIDC_REMOTE_BIN")"

echo "Building internal_oidc (release)"
./scripts/build_bookworm_release.sh internal_oidc

bin_src="target/release/internal_oidc"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi

echo "Provisioning remote directories + system user"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install ca-certificates; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  if ! id -u slopmud >/dev/null 2>&1; then \
    sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
  fi; \
  sudo mkdir -p \"${REMOTE_ROOT}\" \"${remote_bin_dir}\"; \
  sudo chown -R slopmud:slopmud \"${REMOTE_ROOT}\" \
"

echo "Uploading binary -> ${SSH_USER}@${HOST}:${OIDC_REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/internal_oidc"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/internal_oidc \"${OIDC_REMOTE_BIN}\"; \
  sudo rm -f /tmp/internal_oidc \
"

tmp_unit="$(mktemp)"
trap 'rm -f "$tmp_unit"' EXIT
cat >"$tmp_unit" <<EOF
[Unit]
Description=internal_oidc (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=internal_oidc=info
Environment=OIDC_BIND=${OIDC_BIND}
Environment=OIDC_ISSUER=${OIDC_ISSUER}
Environment=OIDC_CLIENT_ID=${OIDC_CLIENT_ID}
Environment=OIDC_CLIENT_SECRET=${OIDC_CLIENT_SECRET}
Environment=OIDC_ED25519_SEED_B64=${OIDC_ED25519_SEED_B64}
Environment=OIDC_TOKEN_TTL_S=${OIDC_TOKEN_TTL_S:-}
Environment=OIDC_AUTH_CODE_TTL_S=${OIDC_AUTH_CODE_TTL_S:-}
Environment=OIDC_USERS_PATH=${OIDC_USERS_PATH:-}
Environment=OIDC_ALLOWED_REDIRECT_URIS=${OIDC_ALLOWED_REDIRECT_URIS:-}
Environment=OIDC_ALLOW_PLAINTEXT_PASSWORDS=${OIDC_ALLOW_PLAINTEXT_PASSWORDS:-}
ExecStart=${OIDC_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

unit_name="${OIDC_APP_NAME}.service"

echo "Installing systemd unit (${unit_name})"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_unit" "${SSH_USER}@${HOST}:/tmp/${unit_name}"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo mv \"/tmp/${unit_name}\" \"/etc/systemd/system/${unit_name}\"; \
  sudo systemctl daemon-reload; \
  sudo systemctl enable --now \"${unit_name}\"; \
  sudo systemctl restart \"${unit_name}\"; \
  sudo systemctl --no-pager --full status \"${unit_name}\" || true \
"

port="${OIDC_BIND##*:}"
echo "Listening check (port ${port})"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo ss -lntp | grep -n \":${port}\\\\b\" || { echo 'not listening'; exit 1; } \
"
