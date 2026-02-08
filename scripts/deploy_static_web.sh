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

# Optional HTTPS (served directly by static_web via rustls)
HTTPS_BIND="${HTTPS_BIND:-}"
TLS_CERT="${TLS_CERT:-}"
TLS_KEY="${TLS_KEY:-}"
SESSION_TCP_ADDR="${SESSION_TCP_ADDR:-}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$REMOTE_BIN")"

echo "Building static_web (release)"
./scripts/build_bookworm_release.sh static_web

bin_src="target/release/static_web"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi

echo "Provisioning remote directories + system user"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y rsync ca-certificates; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install rsync ca-certificates; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  if ! id -u slopmud >/dev/null 2>&1; then \
    sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
  fi; \
  sudo mkdir -p \"${REMOTE_ROOT}\" \"${remote_bin_dir}\" \"${REMOTE_WEB}\"; \
  sudo chown -R slopmud:slopmud \"${REMOTE_ROOT}\" \
"

echo "Uploading web_homepage -> ${SSH_USER}@${HOST}:${REMOTE_WEB}"
rsync -rz --delete --exclude README.md --rsync-path="sudo rsync" -e "ssh ${ssh_opts[*]} ${ssh_port_opt[*]}" web_homepage/ "${SSH_USER}@${HOST}:${REMOTE_WEB}/"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo chown -R slopmud:slopmud \"${REMOTE_WEB}\" \
"

echo "Uploading binary -> ${SSH_USER}@${HOST}:${REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/static_web"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/static_web \"${REMOTE_BIN}\"; \
  sudo rm -f /tmp/static_web \
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

cat >>"$tmp_unit" <<EOF
ExecStart=${REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

echo "Installing systemd unit (slopmud-web) + stopping nginx if present"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_unit" "${SSH_USER}@${HOST}:/tmp/slopmud-web.service"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo mv /tmp/slopmud-web.service /etc/systemd/system/slopmud-web.service; \
  sudo systemctl daemon-reload; \
  sudo systemctl disable --now nginx 2>/dev/null || true; \
  sudo systemctl enable --now slopmud-web; \
  sudo systemctl restart slopmud-web; \
  sudo systemctl --no-pager --full status slopmud-web || true \
"

echo "Smoke test (direct IP, Host header = ${DOMAIN})"
curl -fsSL -H "Host: ${DOMAIN}" "http://${HOST}/" | sed -n '1,25p'

echo "Health check"
curl -fsSL -H "Host: ${DOMAIN}" "http://${HOST}/healthz" || true
