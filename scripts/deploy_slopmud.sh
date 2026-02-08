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
: "${SLOPMUD_APP_NAME:?missing SLOPMUD_APP_NAME in env file}"
: "${SLOPMUD_REMOTE_BIN:?missing SLOPMUD_REMOTE_BIN in env file}"
: "${SLOPMUD_BIND:?missing SLOPMUD_BIND in env file}"
: "${NODE_ID:?missing NODE_ID in env file}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$SLOPMUD_REMOTE_BIN")"

echo "Building slopmud (release)"
./scripts/build_bookworm_release.sh slopmud

bin_src="target/release/slopmud"
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

echo "Uploading binary -> ${SSH_USER}@${HOST}:${SLOPMUD_REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/slopmud"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/slopmud \"${SLOPMUD_REMOTE_BIN}\"; \
  sudo rm -f /tmp/slopmud \
"

tmp_unit="$(mktemp)"
trap 'rm -f "$tmp_unit"' EXIT
cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud service (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=slopmud=info
Environment=NODE_ID=${NODE_ID}
Environment=SLOPMUD_BIND=${SLOPMUD_BIND}
EOF

# Broker -> shard wiring.
# Prefer explicit SHARD_ADDR, otherwise fall back to SHARD_BIND if present in the env file.
if [[ -n "${SHARD_ADDR:-}" ]]; then
  echo "Environment=SHARD_ADDR=${SHARD_ADDR}" >>"$tmp_unit"
elif [[ -n "${SHARD_BIND:-}" ]]; then
  echo "Environment=SHARD_ADDR=${SHARD_BIND}" >>"$tmp_unit"
fi

# Optional: internal OIDC token minting. Secrets should contain no spaces.
if [[ -n "${SLOPMUD_OIDC_TOKEN_URL:-}" ]]; then
  {
    echo "Environment=SLOPMUD_OIDC_TOKEN_URL=${SLOPMUD_OIDC_TOKEN_URL}"
    [[ -n "${SLOPMUD_OIDC_CLIENT_ID:-}" ]] && echo "Environment=SLOPMUD_OIDC_CLIENT_ID=${SLOPMUD_OIDC_CLIENT_ID}"
    [[ -n "${SLOPMUD_OIDC_CLIENT_SECRET:-}" ]] && echo "Environment=SLOPMUD_OIDC_CLIENT_SECRET=${SLOPMUD_OIDC_CLIENT_SECRET}"
    [[ -n "${SLOPMUD_OIDC_SCOPE:-}" ]] && echo "Environment=SLOPMUD_OIDC_SCOPE=${SLOPMUD_OIDC_SCOPE}"
  } >>"$tmp_unit"
fi

cat >>"$tmp_unit" <<EOF
ExecStart=${SLOPMUD_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

unit_name="${SLOPMUD_APP_NAME}.service"

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

port="${SLOPMUD_BIND##*:}"
echo "Listening check (port ${port})"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo ss -lntp | grep -n \":${port}\\\\b\" || { echo 'not listening'; exit 1; } \
"
