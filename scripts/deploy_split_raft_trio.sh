#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  deploy_split_raft_trio.sh /path/to/env/split-prd.env

Deploys shard_01 to three private Raft nodes through the public gateway using
SSH ProxyJump. The Raft nodes do not need public IPv4 addresses.

Required env:
  GATEWAY_HOST or HOST
  SSH_USER
  SSH_PORT
  REMOTE_ROOT
  SHARD_REMOTE_BIN
  SHARD_NODE_HOSTS      comma-separated private IP/DNS names for n0,n1,n2

Optional env:
  RAFT_SSH_USER         default SSH_USER
  RAFT_SSH_PORT         default 22
  SHARD_RAFT_NODE_IDS   default n0,n1,n2
  SHARD_PORT            default 5000
  SHARD_RAFT_PORT       default 5100
  SHARD_RAFT_LOG        default REMOTE_ROOT/var/shard_01_groups_raft.jsonl
EOF
}

env_file="${1:-}"
if [[ -z "$env_file" ]]; then
  usage
  exit 2
fi
if [[ ! -f "$env_file" ]]; then
  echo "ERROR: env file not found: $env_file" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

gateway_host="${GATEWAY_HOST:-${HOST:-}}"
: "${gateway_host:?missing GATEWAY_HOST or HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
: "${SHARD_REMOTE_BIN:?missing SHARD_REMOTE_BIN in env file}"
: "${SHARD_NODE_HOSTS:?missing SHARD_NODE_HOSTS in env file}"

raft_ssh_user="${RAFT_SSH_USER:-$SSH_USER}"
raft_ssh_port="${RAFT_SSH_PORT:-22}"
shard_port="${SHARD_PORT:-5000}"
raft_port="${SHARD_RAFT_PORT:-5100}"
node_ids_csv="${SHARD_RAFT_NODE_IDS:-${SHARD_NODE_IDS:-n0,n1,n2}}"
base_log="${SHARD_RAFT_LOG:-${REMOTE_ROOT}/var/shard_01_groups_raft.jsonl}"

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  IFS=',' read -r -a out_ref <<<"$raw"
  local i
  for i in "${!out_ref[@]}"; do
    out_ref[$i]="$(printf '%s' "${out_ref[$i]}" | xargs)"
  done
}

require_three() {
  local label="$1"
  local -n arr_ref="$2"
  if [[ "${#arr_ref[@]}" != "3" ]]; then
    echo "ERROR: ${label} must contain exactly 3 comma-separated values" >&2
    exit 2
  fi
}

suffix_log_path() {
  local path="$1"
  local suffix="$2"
  if [[ "$path" == *.jsonl ]]; then
    printf '%s_%s.jsonl' "${path%.jsonl}" "$suffix"
  else
    printf '%s_%s' "$path" "$suffix"
  fi
}

split_csv "$SHARD_NODE_HOSTS" node_hosts
split_csv "$node_ids_csv" node_ids
require_three "SHARD_NODE_HOSTS" node_hosts
require_three "SHARD_RAFT_NODE_IDS" node_ids

raft_addrs=()
for i in 0 1 2; do
  raft_addrs+=("${node_hosts[$i]}:${raft_port}")
done

default_logs="${base_log},$(suffix_log_path "$base_log" n1),$(suffix_log_path "$base_log" n2)"
split_csv "${SHARD_TRIO_RAFT_LOGS:-$default_logs}" raft_logs
require_three "SHARD_TRIO_RAFT_LOGS" raft_logs

proxy_jump="${SSH_USER}@${gateway_host}"
if [[ "$SSH_PORT" != "22" ]]; then
  proxy_jump="${proxy_jump}:${SSH_PORT}"
fi

ssh_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -p "$raft_ssh_port")
scp_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -P "$raft_ssh_port")

remote_bin_dir="$(dirname "$SHARD_REMOTE_BIN")"

echo "Building shard_01 (release)"
./scripts/build_bookworm_release.sh shard_01

bin_src="target/release/shard_01"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

for i in 0 1 2; do
  host="${node_hosts[$i]}"
  node_id="${node_ids[$i]}"
  unit_name="${SHARD_APP_NAME:-shard-01-prd}-${node_id}"
  target="${raft_ssh_user}@${host}"

  peers=()
  for j in 0 1 2; do
    if [[ "$i" == "$j" ]]; then
      continue
    fi
    peers+=("${node_ids[$j]}@${raft_addrs[$j]}")
  done
  peers_csv="$(IFS=','; echo "${peers[*]}")"

  tmp_unit="${tmp_dir}/${unit_name}.service"
  cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud shard_01 split Raft node ${node_id} (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=shard_01=info
Environment=NODE_ID=${node_id}
Environment=SHARD_BIND=0.0.0.0:${shard_port}
Environment=SHARD_RAFT_NODE_ID=${node_id}
Environment=SHARD_RAFT_BIND=0.0.0.0:${raft_port}
Environment=SHARD_RAFT_PEERS=${peers_csv}
Environment=SHARD_RAFT_LOG=${raft_logs[$i]}
EOF

  if [[ -n "${SHARD_RAFT_ELECTION_MS:-}" ]]; then
    echo "Environment=SHARD_RAFT_ELECTION_MS=${SHARD_RAFT_ELECTION_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_RAFT_HEARTBEAT_MS:-}" ]]; then
    echo "Environment=SHARD_RAFT_HEARTBEAT_MS=${SHARD_RAFT_HEARTBEAT_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${WORLD_TICK_MS:-}" ]]; then
    echo "Environment=WORLD_TICK_MS=${WORLD_TICK_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${WORLD_TIME_SCALE_PPM:-}" ]]; then
    echo "Environment=WORLD_TIME_SCALE_PPM=${WORLD_TIME_SCALE_PPM}" >>"$tmp_unit"
  fi
  if [[ -n "${BARTENDER_EMOTE_MS:-}" ]]; then
    echo "Environment=BARTENDER_EMOTE_MS=${BARTENDER_EMOTE_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${MOB_WANDER_MS:-}" ]]; then
    echo "Environment=MOB_WANDER_MS=${MOB_WANDER_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_BOOTSTRAP_ADMINS:-}" ]]; then
    echo "Environment=SHARD_BOOTSTRAP_ADMINS=${SHARD_BOOTSTRAP_ADMINS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_BOOTSTRAP_ADMIN_SSO:-}" ]]; then
    echo "Environment=SHARD_BOOTSTRAP_ADMIN_SSO=${SHARD_BOOTSTRAP_ADMIN_SSO}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_API_BASE:-}" ]]; then
    echo "Environment=OPENAI_API_BASE=${OPENAI_API_BASE}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_PING_MODEL:-}" ]]; then
    echo "Environment=OPENAI_PING_MODEL=${OPENAI_PING_MODEL}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_API_KEY:-}" ]]; then
    echo "Environment=OPENAI_API_KEY=${OPENAI_API_KEY}" >>"$tmp_unit"
  fi

  cat >>"$tmp_unit" <<EOF
ExecStart=${SHARD_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

  echo "Provisioning ${node_id} (${host})"
  ssh "${ssh_opts[@]}" "$target" "\
    set -euo pipefail; \
    if ! id -u slopmud >/dev/null 2>&1; then sudo useradd --system --home '${REMOTE_ROOT}' --create-home --shell /usr/sbin/nologin slopmud; fi; \
    sudo mkdir -p '${REMOTE_ROOT}' '${remote_bin_dir}' '${REMOTE_ROOT}/var'; \
    sudo chown -R slopmud:slopmud '${REMOTE_ROOT}' \
  "

  echo "Uploading shard_01 -> ${target}:${SHARD_REMOTE_BIN}"
  scp "${scp_opts[@]}" "$bin_src" "${target}:/tmp/shard_01"
  ssh "${ssh_opts[@]}" "$target" "\
    set -euo pipefail; \
    sudo install -m 0755 -o root -g root /tmp/shard_01 '${SHARD_REMOTE_BIN}'; \
    sudo rm -f /tmp/shard_01 \
  "

  echo "Installing ${unit_name}.service"
  scp "${scp_opts[@]}" "$tmp_unit" "${target}:/tmp/${unit_name}.service"
  ssh "${ssh_opts[@]}" "$target" "\
    set -euo pipefail; \
    sudo mv '/tmp/${unit_name}.service' '/etc/systemd/system/${unit_name}.service'; \
    sudo systemctl daemon-reload; \
    sudo systemctl enable --now '${unit_name}'; \
    sudo systemctl restart '${unit_name}'; \
    sudo systemctl --no-pager --full status '${unit_name}' || true \
  "
done

echo "Waiting for private shard and Raft ports"
for i in 0 1 2; do
  target="${raft_ssh_user}@${node_hosts[$i]}"
  for port in "$shard_port" "$raft_port"; do
    ssh "${ssh_opts[@]}" "$target" "\
      set -euo pipefail; \
      for _ in {1..80}; do \
        if sudo ss -lntp | grep -q ':${port}\\b'; then exit 0; fi; \
        sleep 0.25; \
      done; \
      sudo ss -lntp | grep -n ':${port}\\b' || true; \
      echo 'not listening on ${node_hosts[$i]}:${port}'; \
      exit 1 \
    "
  done
done

shard_addrs_list=()
for host in "${node_hosts[@]}"; do
  shard_addrs_list+=("${host}:${shard_port}")
done
shard_addrs="$(IFS=','; echo "${shard_addrs_list[*]}")"
echo "OK: split Raft trio deployed"
echo "SHARD_ADDRS=${shard_addrs}"
