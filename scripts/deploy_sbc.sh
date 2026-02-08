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

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="${REMOTE_ROOT}/bin"

echo "Building SBC binaries (release)"
for pkg in sbc_raftd sbc_enforcerd sbc_metricsd sbc_deciderd; do
  ./scripts/build_bookworm_release.sh "$pkg"
done

echo "Provisioning remote directories + system user"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install ca-certificates curl; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  if ! id -u slopmud >/dev/null 2>&1; then \
    sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
  fi; \
  sudo mkdir -p \"${REMOTE_ROOT}\" \"${remote_bin_dir}\"; \
  sudo chown -R slopmud:slopmud \"${REMOTE_ROOT}\" \
"

echo "Stopping any existing SBC processes/units (best-effort)"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo systemctl disable --now sbc-enforcerd sbc-raftd sbc-metricsd sbc-deciderd 2>/dev/null || true; \
  sudo pkill -x sbc_enforcerd 2>/dev/null || true; \
  sudo pkill -x sbc_raftd 2>/dev/null || true; \
  sudo pkill -x sbc_metricsd 2>/dev/null || true; \
  sudo pkill -x sbc_deciderd 2>/dev/null || true; \
"

echo "Uploading SBC binaries -> ${SSH_USER}@${HOST}:${remote_bin_dir}"
for bin in sbc_raftd sbc_enforcerd sbc_metricsd sbc_deciderd; do
  src="target/release/${bin}"
  if [[ ! -x "$src" ]]; then
    echo "ERROR: expected binary at $src" >&2
    exit 2
  fi
  scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$src" "${SSH_USER}@${HOST}:/tmp/${bin}"
  ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
    set -euo pipefail; \
    sudo install -m 0755 -o root -g root \"/tmp/${bin}\" \"${remote_bin_dir}/${bin}\"; \
    sudo rm -f \"/tmp/${bin}\" \
  "
done

echo "Uploading exempt prefixes -> ${SSH_USER}@${HOST}:${REMOTE_ROOT}/sbc_exempt_prefixes.txt"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" reference/sbc_exempt_prefixes.txt "${SSH_USER}@${HOST}:/tmp/sbc_exempt_prefixes.txt"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0644 -o slopmud -g slopmud /tmp/sbc_exempt_prefixes.txt \"${REMOTE_ROOT}/sbc_exempt_prefixes.txt\"; \
  sudo rm -f /tmp/sbc_exempt_prefixes.txt \
"

unit_dir_local="reference/systemd"

tmp_raft="$(mktemp)"
tmp_enf="$(mktemp)"
tmp_met="$(mktemp)"
tmp_dec="$(mktemp)"
trap 'rm -f "$tmp_raft" "$tmp_enf" "$tmp_met" "$tmp_dec"' EXIT

# Generate systemd units with env-overrides (kept minimal; use defaults unless the env file defines them).
SBC_ENABLE_DNS_NAME="${SBC_ENABLE_DNS_NAME:-sbc-anti-lockout-prd.slopmud.com}"
SBC_ENABLE_DNS_IP="${SBC_ENABLE_DNS_IP:-192.0.2.1}"
SBC_ENABLE_DNS_INTERVAL_S="${SBC_ENABLE_DNS_INTERVAL_S:-60}"
SBC_APPLY_SNAPSHOT="${SBC_APPLY_SNAPSHOT:-0}"
SBC_STATUS_HTTP="${SBC_STATUS_HTTP:-127.0.0.1:9911}"
SBC_STATSD_BIND="${SBC_STATSD_BIND:-0.0.0.0:8125}"
SBC_METRICS_HTTP="${SBC_METRICS_HTTP:-127.0.0.1:9912}"

cp "${unit_dir_local}/sbc-raftd.service" "$tmp_raft"
cp "${unit_dir_local}/sbc-metricsd.service" "$tmp_met"
cp "${unit_dir_local}/sbc-deciderd.service" "$tmp_dec"

cat >"$tmp_enf" <<EOF
[Unit]
Description=slopmud SBC enforcer (DNS-gated)
After=network-online.target sbc-raftd.service
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}

Environment=SBC_NODE_ID=%H
Environment=SBC_ADMIN_SOCK=/run/slopmud/sbc-admin.sock
Environment=SBC_EVENTS_SOCK=/run/slopmud/sbc-events.sock
Environment=SBC_STATUS_HTTP=${SBC_STATUS_HTTP}

# DNS A-record value => enforcement enabled.
Environment=SBC_ENABLE_DNS_NAME=${SBC_ENABLE_DNS_NAME}
Environment=SBC_ENABLE_DNS_IP=${SBC_ENABLE_DNS_IP}
Environment=SBC_ENABLE_DNS_INTERVAL_S=${SBC_ENABLE_DNS_INTERVAL_S}

# Snapshot subscription controls whether bans are re-applied on restart.
Environment=SBC_APPLY_SNAPSHOT=${SBC_APPLY_SNAPSHOT}

# Exempt prefix list (block-level enforcement disabled).
Environment=SBC_EXEMPT_PREFIXES_PATH=${REMOTE_ROOT}/sbc_exempt_prefixes.txt

ExecStart=${remote_bin_dir}/sbc_enforcerd

# Always detach enforcement on stop (noop today; required once XDP is implemented).
ExecStopPost=${remote_bin_dir}/sbc_enforcerd --detach-xdp --iface eth0

Restart=always
RestartSec=2

RuntimeDirectory=slopmud
RuntimeDirectoryMode=0755

AmbientCapabilities=CAP_NET_ADMIN CAP_BPF CAP_SYS_ADMIN
CapabilityBoundingSet=CAP_NET_ADMIN CAP_BPF CAP_SYS_ADMIN
NoNewPrivileges=true

PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/run/slopmud

[Install]
WantedBy=multi-user.target
EOF

# Patch metrics binds in metrics unit if caller overrides.
sed -i \
  -e "s#^Environment=SBC_STATSD_BIND=.*#Environment=SBC_STATSD_BIND=${SBC_STATSD_BIND}#" \
  -e "s#^Environment=SBC_METRICS_HTTP=.*#Environment=SBC_METRICS_HTTP=${SBC_METRICS_HTTP}#" \
  "$tmp_met"

echo "Installing SBC systemd units"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_raft" "${SSH_USER}@${HOST}:/tmp/sbc-raftd.service"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_met" "${SSH_USER}@${HOST}:/tmp/sbc-metricsd.service"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_dec" "${SSH_USER}@${HOST}:/tmp/sbc-deciderd.service"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_enf" "${SSH_USER}@${HOST}:/tmp/sbc-enforcerd.service"

ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo mv /tmp/sbc-raftd.service /etc/systemd/system/sbc-raftd.service; \
  sudo mv /tmp/sbc-metricsd.service /etc/systemd/system/sbc-metricsd.service; \
  sudo mv /tmp/sbc-deciderd.service /etc/systemd/system/sbc-deciderd.service; \
  sudo mv /tmp/sbc-enforcerd.service /etc/systemd/system/sbc-enforcerd.service; \
  sudo systemctl daemon-reload; \
  sudo systemctl enable --now sbc-raftd; \
  sudo systemctl enable --now sbc-metricsd; \
  sudo systemctl enable --now sbc-enforcerd; \
  sudo systemctl disable --now sbc-deciderd 2>/dev/null || true; \
  sudo systemctl restart sbc-raftd sbc-metricsd sbc-enforcerd; \
  sudo systemctl --no-pager --full status sbc-enforcerd || true \
"

echo "SBC enforcer status"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "curl -fsSL http://${SBC_STATUS_HTTP}/status | sed -n '1,160p'"
