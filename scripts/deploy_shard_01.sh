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
: "${SHARD_APP_NAME:?missing SHARD_APP_NAME in env file}"
: "${SHARD_REMOTE_BIN:?missing SHARD_REMOTE_BIN in env file}"
: "${SHARD_BIND:?missing SHARD_BIND in env file}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$SHARD_REMOTE_BIN")"

echo "Building shard_01 (release)"
./scripts/build_bookworm_release.sh shard_01

bin_src="target/release/shard_01"
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

echo "Uploading binary -> ${SSH_USER}@${HOST}:${SHARD_REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/shard_01"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/shard_01 \"${SHARD_REMOTE_BIN}\"; \
  sudo rm -f /tmp/shard_01 \
"

tmp_unit="$(mktemp)"
trap 'rm -f "$tmp_unit"' EXIT
cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud shard_01 (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=shard_01=info
Environment=SHARD_BIND=${SHARD_BIND}
ExecStart=${SHARD_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

unit_name="${SHARD_APP_NAME}.service"

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

port="${SHARD_BIND##*:}"
echo "Listening check (port ${port})"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo ss -lntp | grep -n \":${port}\\\\b\" || { echo 'not listening'; exit 1; } \
"

