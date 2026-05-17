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
  SLOPMUD_ROLLING_TRANSFER_LEADER default 1
  SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART default 1
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

unit_name_for() {
  local i="$1"
  printf '%s-%s' "${SHARD_APP_NAME:-shard-01-prd}" "${node_ids[$i]}"
}

ssh_node() {
  local i="$1"
  shift
  local target="${raft_ssh_user}@${node_hosts[$i]}"
  ssh "${ssh_opts[@]}" "$target" "$@"
}

json_field() {
  local json="$1"
  local field="$2"
  python3 - "$json" "$field" <<'PY'
import json
import sys

try:
    data = json.loads(sys.argv[1])
except Exception:
    sys.exit(1)
value = data.get(sys.argv[2])
if value is None:
    print("")
elif isinstance(value, bool):
    print("true" if value else "false")
else:
    print(value)
PY
}

normalize_host() {
  local raw="$1"
  raw="${raw#[}"
  raw="${raw%]}"
  raw="${raw%:${shard_port}}"
  raw="${raw%:${raft_port}}"
  printf '%s' "$raw"
}

node_index_for_host() {
  local host
  host="$(normalize_host "$1")"
  local i
  for i in 0 1 2; do
    if [[ "${node_hosts[$i]}" == "$host" ]]; then
      printf '%s\n' "$i"
      return 0
    fi
  done
  return 1
}

raft_rpc_node() {
  local i="$1"
  local payload="$2"
  local payload_b64
  payload_b64="$(printf '%s' "$payload" | base64 | tr -d '\n')"
  ssh_node "$i" "PAYLOAD_B64='${payload_b64}' RAFT_PORT='${raft_port}' timeout 6 bash -lc 'set -euo pipefail; payload=\$(printf %s \"\$PAYLOAD_B64\" | base64 -d); exec 3<>/dev/tcp/127.0.0.1/\$RAFT_PORT; printf \"%s\n\" \"\$payload\" >&3; IFS= read -r -t 5 line <&3; printf \"%s\n\" \"\$line\"'"
}

raft_status_node() {
  local i="$1"
  raft_rpc_node "$i" '{"t":"StatusReq"}'
}

current_leader_index() {
  local i resp kind role
  for i in 0 1 2; do
    resp="$(raft_status_node "$i" 2>/dev/null || true)"
    kind="$(json_field "$resp" t 2>/dev/null || true)"
    role="$(json_field "$resp" role 2>/dev/null || true)"
    if [[ "$kind" == "StatusResp" && "$role" == "Leader" ]]; then
      printf '%s\n' "$i"
      return 0
    fi
  done
  return 1
}

status_supported_count() {
  local i resp kind count
  count=0
  for i in 0 1 2; do
    resp="$(raft_status_node "$i" 2>/dev/null || true)"
    kind="$(json_field "$resp" t 2>/dev/null || true)"
    if [[ "$kind" == "StatusResp" ]]; then
      count=$((count + 1))
    fi
  done
  printf '%s\n' "$count"
}

gateway_active_shard_host() {
  local peer
  peer="$(ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "sudo ss -Htnp 2>/dev/null | awk '\$5 ~ /:${shard_port}\$/ {print \$5; exit}'" || true)"
  if [[ -z "$peer" ]]; then
    return 1
  fi
  normalize_host "$peer"
}

wait_node_ports() {
  local i="$1"
  local port
  for port in "$shard_port" "$raft_port"; do
    ssh_node "$i" "\
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
}

wait_for_leader() {
  local expected="${1:-}"
  local deadline=$((SECONDS + 40))
  local leader
  while (( SECONDS < deadline )); do
    leader="$(current_leader_index || true)"
    if [[ -n "$leader" && ( -z "$expected" || "$leader" == "$expected" ) ]]; then
      echo "Raft leader is ${node_ids[$leader]} (${node_hosts[$leader]})"
      return 0
    fi
    sleep 0.5
  done
  echo "ERROR: timed out waiting for Raft leader ${expected:-any}" >&2
  for i in 0 1 2; do
    echo "status ${node_ids[$i]}: $(raft_status_node "$i" 2>/dev/null || true)" >&2
  done
  return 1
}

wait_cluster_ready() {
  local deadline=$((SECONDS + 40))
  local leader active_host active_i status_seen i resp kind role
  while (( SECONDS < deadline )); do
    leader=""
    status_seen=0
    for i in 0 1 2; do
      resp="$(raft_status_node "$i" 2>/dev/null || true)"
      kind="$(json_field "$resp" t 2>/dev/null || true)"
      role="$(json_field "$resp" role 2>/dev/null || true)"
      if [[ "$kind" == "StatusResp" ]]; then
        status_seen=$((status_seen + 1))
        if [[ "$role" == "Leader" ]]; then
          leader="$i"
        fi
      fi
    done
    active_host="$(gateway_active_shard_host || true)"
    active_i=""
    if [[ -n "$active_host" ]]; then
      active_i="$(node_index_for_host "$active_host" 2>/dev/null || true)"
    fi
    if [[ -n "$active_i" && -z "$leader" && "$status_seen" -lt 3 ]]; then
      echo "Gateway is connected to ${node_ids[$active_i]} (${node_hosts[$active_i]}); leader status unavailable"
      return 0
    fi
    if [[ -n "$leader" && ( -z "$active_i" || "$active_i" == "$leader" ) ]]; then
      if [[ -n "$active_i" ]]; then
        echo "Gateway is connected to Raft leader ${node_ids[$leader]} (${node_hosts[$leader]})"
      else
        echo "Raft leader is ${node_ids[$leader]} (${node_hosts[$leader]}); no gateway shard socket observed"
      fi
      return 0
    fi
    sleep 0.5
  done
  echo "ERROR: timed out waiting for gateway to connect to current Raft leader" >&2
  echo "leader index: $(current_leader_index 2>/dev/null || true)" >&2
  echo "gateway active host: $(gateway_active_shard_host 2>/dev/null || true)" >&2
  return 1
}

restart_node() {
  local i="$1"
  local unit_name
  unit_name="$(unit_name_for "$i")"
  echo "Restarting ${node_ids[$i]} (${node_hosts[$i]})"
  ssh_node "$i" "\
    set -euo pipefail; \
    sudo systemctl restart '${unit_name}'; \
    sudo systemctl --no-pager --full status '${unit_name}' || true \
  "
  wait_node_ports "$i"
}

try_transfer_leader() {
  local from_i="$1"
  local target_i="$2"
  local target_id="${node_ids[$target_i]}"
  local resp kind accepted reason leader_id
  echo "Requesting Raft leadership transfer ${node_ids[$from_i]} -> ${target_id}"
  resp="$(raft_rpc_node "$from_i" "{\"t\":\"TransferLeaderReq\",\"target_id\":\"${target_id}\"}" 2>/dev/null || true)"
  kind="$(json_field "$resp" t 2>/dev/null || true)"
  accepted="$(json_field "$resp" accepted 2>/dev/null || true)"
  reason="$(json_field "$resp" reason 2>/dev/null || true)"
  leader_id="$(json_field "$resp" leader_id 2>/dev/null || true)"
  if [[ "$kind" == "TransferLeaderResp" && "$accepted" == "true" ]]; then
    echo "Leadership transfer accepted; leader=${leader_id:-unknown}"
    return 0
  fi
  echo "Leadership transfer unavailable: ${reason:-${resp:-no response}}" >&2
  return 1
}

proxy_jump="${SSH_USER}@${gateway_host}"
if [[ "$SSH_PORT" != "22" ]]; then
  proxy_jump="${proxy_jump}:${SSH_PORT}"
fi

ssh_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -p "$raft_ssh_port")
scp_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -P "$raft_ssh_port")
gateway_ssh_opts=(-o StrictHostKeyChecking=accept-new -p "$SSH_PORT")

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
  unit_name="$(unit_name_for "$i")"
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
    sudo systemctl enable '${unit_name}'; \
    if ! sudo systemctl is-active --quiet '${unit_name}'; then sudo systemctl start '${unit_name}'; fi; \
    sudo systemctl --no-pager --full status '${unit_name}' || true \
  "
done

echo "Waiting for private shard and Raft ports"
for i in 0 1 2; do
  wait_node_ports "$i"
done

echo "Planning rolling restart order"
active_host="$(gateway_active_shard_host || true)"
active_i=""
if [[ -n "$active_host" ]]; then
  active_i="$(node_index_for_host "$active_host" 2>/dev/null || true)"
fi
leader_i="$(current_leader_index || true)"
if [[ -z "$active_i" && -n "$leader_i" ]]; then
  active_i="$leader_i"
fi
if [[ -z "$active_i" ]]; then
  active_i=0
fi
echo "Impact-sensitive node: ${node_ids[$active_i]} (${node_hosts[$active_i]})"

restart_order=()
for i in 0 1 2; do
  if [[ "$i" != "$active_i" ]]; then
    restart_order+=("$i")
  fi
done
restart_order+=("$active_i")

for i in "${restart_order[@]}"; do
  if [[ "$i" == "$active_i" ]]; then
    leader_i="$(current_leader_index || true)"
    if [[ -z "$leader_i" ]]; then
      if [[ "$(status_supported_count)" == "3" ]]; then
        wait_for_leader ""
        leader_i="$(current_leader_index || true)"
      fi
    fi
    if [[ "$leader_i" == "$active_i" ]]; then
      target_i=$(((active_i + 1) % 3))
      if [[ "${SLOPMUD_ROLLING_TRANSFER_LEADER:-1}" != "0" ]]; then
        if try_transfer_leader "$active_i" "$target_i"; then
          wait_for_leader "$target_i"
        else
          if [[ "${SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART:-1}" == "0" ]]; then
            echo "ERROR: refusing to restart active leader without transfer" >&2
            exit 1
          fi
          echo "WARN: restarting active node without graceful transfer; this should only happen during the first rollout from older binaries" >&2
        fi
      fi
    elif [[ -n "$leader_i" ]]; then
      echo "Leadership is already on ${node_ids[$leader_i]} (${node_hosts[$leader_i]}); restarting old gateway-connected node ${node_ids[$active_i]}"
    else
      echo "WARN: no Raft leader visible before active-node restart" >&2
    fi
  fi
  restart_node "$i"
  wait_cluster_ready
done

shard_addrs_list=()
for host in "${node_hosts[@]}"; do
  shard_addrs_list+=("${host}:${shard_port}")
done
shard_addrs="$(IFS=','; echo "${shard_addrs_list[*]}")"
echo "OK: split Raft trio deployed"
echo "SHARD_ADDRS=${shard_addrs}"
