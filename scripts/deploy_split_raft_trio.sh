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
  SLOPMUD_SKIP_BUILD    default 0; set 1 to reuse target/release/shard_01
  SLOPMUD_BIN_SRC       default target/release/shard_01
  SLOPMUD_RELEASE_ID    default current git sha or timestamp
  SLOPMUD_DEPLOY_FROM_S3 default 0; set 1 to upload once and have nodes pull concurrently
  SLOPMUD_RELEASE_S3_URI optional full s3:// URI for the shard_01 binary object
  SLOPMUD_RELEASE_S3_BUCKET default SLOPMUD_RELEASE_S3_BUCKET, ASSETS_BUCKET, or derived slopmud-assets bucket
  SLOPMUD_RELEASE_S3_PREFIX default split-raft/<ENV_NAME>/<release_id>
  SLOPMUD_ATOMIC_BIN_SWAP default 1; install versioned binary and atomically swap symlink
  SLOPMUD_STRICT_LIVE_UPGRADE default 0; set 1 to require visible leader/healthy gateway after each restart
  SLOPMUD_QUORUM_RESTART_GUARD default 1; require the other two voters before each restart
  SLOPMUD_RAFT_RESTART_LEASE default auto; auto|required|off cluster-owned restart lease
  SLOPMUD_RAFT_RESTART_LEASE_TTL_MS default 60000
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
    if [[ "${SLOPMUD_STRICT_LIVE_UPGRADE:-0}" != "1" && -n "$active_i" && -z "$leader" && "$status_seen" -lt 3 ]]; then
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

restart_lease_tokens=("" "" "")

acquire_restart_lease() {
  local i="$1"
  local mode="${SLOPMUD_RAFT_RESTART_LEASE:-auto}"
  local ttl_ms="${SLOPMUD_RAFT_RESTART_LEASE_TTL_MS:-60000}"
  local token deadline leader_i resp kind accepted reason holder holder_token expires
  case "$mode" in
    off|0|false|no) return 0 ;;
    auto|required) ;;
    *)
      echo "ERROR: SLOPMUD_RAFT_RESTART_LEASE must be auto, required, or off" >&2
      exit 2
      ;;
  esac
  if ! [[ "$ttl_ms" =~ ^[0-9]+$ ]]; then
    echo "ERROR: SLOPMUD_RAFT_RESTART_LEASE_TTL_MS must be an integer" >&2
    exit 2
  fi

  token="${release_id}-${node_ids[$i]}-$$-${RANDOM}-${RANDOM}"
  deadline=$((SECONDS + 90))
  while (( SECONDS < deadline )); do
    leader_i="$(current_leader_index || true)"
    if [[ -z "$leader_i" ]]; then
      sleep 0.5
      continue
    fi
    resp="$(raft_rpc_node "$leader_i" "{\"t\":\"RestartLeaseReq\",\"node_id\":\"${node_ids[$i]}\",\"token\":\"${token}\",\"ttl_ms\":${ttl_ms}}" 2>/dev/null || true)"
    kind="$(json_field "$resp" t 2>/dev/null || true)"
    accepted="$(json_field "$resp" accepted 2>/dev/null || true)"
    reason="$(json_field "$resp" reason 2>/dev/null || true)"
    holder="$(json_field "$resp" node_id 2>/dev/null || true)"
    holder_token="$(json_field "$resp" token 2>/dev/null || true)"
    expires="$(json_field "$resp" expires_in_ms 2>/dev/null || true)"
    if [[ "$kind" == "RestartLeaseResp" && "$accepted" == "true" ]]; then
      restart_lease_tokens[$i]="$token"
      echo "Raft restart lease acquired for ${node_ids[$i]} via ${node_ids[$leader_i]} token=${token}"
      return 0
    fi
    if [[ "$kind" != "RestartLeaseResp" ]]; then
      if [[ "$mode" == "auto" ]]; then
        echo "WARN: Raft restart lease unsupported by current leader; falling back to local quorum guard"
        return 0
      fi
      echo "ERROR: Raft restart lease required but unavailable from ${node_ids[$leader_i]}: ${resp:-no response}" >&2
      exit 1
    fi
    case "$reason" in
      "another restart lease is active"|"another restart lease won the race")
        echo "Waiting for active restart lease holder=${holder:-unknown} expires_in_ms=${expires:-unknown}"
        sleep 0.5
        ;;
      *)
        echo "ERROR: restart lease rejected for ${node_ids[$i]}: ${reason:-unknown} holder=${holder:-} token=${holder_token:-}" >&2
        exit 1
        ;;
    esac
  done

  echo "ERROR: timed out acquiring restart lease for ${node_ids[$i]}" >&2
  exit 1
}

release_restart_lease() {
  local i="$1"
  local token="${restart_lease_tokens[$i]:-}"
  local leader_i resp kind accepted reason
  if [[ -z "$token" ]]; then
    return 0
  fi
  leader_i="$(current_leader_index || true)"
  if [[ -z "$leader_i" ]]; then
    echo "WARN: no leader visible while releasing restart lease for ${node_ids[$i]}; lease will expire" >&2
    return 0
  fi
  resp="$(raft_rpc_node "$leader_i" "{\"t\":\"RestartLeaseReleaseReq\",\"node_id\":\"${node_ids[$i]}\",\"token\":\"${token}\"}" 2>/dev/null || true)"
  kind="$(json_field "$resp" t 2>/dev/null || true)"
  accepted="$(json_field "$resp" accepted 2>/dev/null || true)"
  reason="$(json_field "$resp" reason 2>/dev/null || true)"
  restart_lease_tokens[$i]=""
  if [[ "$kind" == "RestartLeaseReleaseResp" && "$accepted" == "true" ]]; then
    echo "Raft restart lease released for ${node_ids[$i]}"
    return 0
  fi
  echo "WARN: restart lease release for ${node_ids[$i]} was not accepted: ${reason:-${resp:-no response}}" >&2
}

guard_quorum_before_restart() {
  local candidate_i="$1"
  local status_seen other_seen leader i resp kind role
  status_seen=0
  other_seen=0
  leader=""
  for i in 0 1 2; do
    resp="$(raft_status_node "$i" 2>/dev/null || true)"
    kind="$(json_field "$resp" t 2>/dev/null || true)"
    role="$(json_field "$resp" role 2>/dev/null || true)"
    if [[ "$kind" == "StatusResp" ]]; then
      status_seen=$((status_seen + 1))
      if [[ "$i" != "$candidate_i" ]]; then
        other_seen=$((other_seen + 1))
      fi
      if [[ "$role" == "Leader" ]]; then
        leader="$i"
      fi
    fi
  done
  if (( other_seen < 2 )); then
    echo "ERROR: refusing to restart ${node_ids[$candidate_i]}: only ${other_seen}/2 remaining voters answered Raft status" >&2
    return 1
  fi
  if [[ -z "$leader" ]]; then
    echo "ERROR: refusing to restart ${node_ids[$candidate_i]}: no visible Raft leader" >&2
    return 1
  fi
  if [[ "$leader" == "$candidate_i" ]]; then
    echo "ERROR: refusing to restart ${node_ids[$candidate_i]}: it is still the Raft leader" >&2
    return 1
  fi
  echo "Quorum guard: ${node_ids[$candidate_i]} can restart; ${other_seen}/2 remaining voters visible, leader=${node_ids[$leader]}"
}

wait_jobs() {
  local label="$1"
  shift
  local pid rc
  rc=0
  for pid in "$@"; do
    if ! wait "$pid"; then
      rc=1
    fi
  done
  if [[ "$rc" != "0" ]]; then
    echo "ERROR: ${label} failed" >&2
    exit 1
  fi
}

proxy_jump="${SSH_USER}@${gateway_host}"
if [[ "$SSH_PORT" != "22" ]]; then
  proxy_jump="${proxy_jump}:${SSH_PORT}"
fi

ssh_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -p "$raft_ssh_port")
scp_opts=(-o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -P "$raft_ssh_port")
gateway_ssh_opts=(-o StrictHostKeyChecking=accept-new -p "$SSH_PORT")

remote_bin_dir="$(dirname "$SHARD_REMOTE_BIN")"
release_id="${SLOPMUD_RELEASE_ID:-$(git rev-parse --short HEAD 2>/dev/null || date +%Y%m%d%H%M%S)}"
if ! [[ "$release_id" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "ERROR: SLOPMUD_RELEASE_ID must contain only letters, numbers, dot, underscore, or dash: ${release_id}" >&2
  exit 2
fi
release_dir="${SLOPMUD_VERSIONED_BIN_DIR:-${remote_bin_dir}/releases}"
remote_release_bin="${release_dir}/shard_01-${release_id}"
bin_src="${SLOPMUD_BIN_SRC:-target/release/shard_01}"
case "${SLOPMUD_DEPLOY_FROM_S3:-0}" in
  1|true|yes|on) deploy_from_s3=1 ;;
  0|false|no|off|"") deploy_from_s3=0 ;;
  *)
    echo "ERROR: SLOPMUD_DEPLOY_FROM_S3 must be 0/1, true/false, yes/no, or on/off" >&2
    exit 2
    ;;
esac

if [[ "${SLOPMUD_SKIP_BUILD:-0}" == "1" ]]; then
  echo "Skipping build; using ${bin_src}"
else
  echo "Building shard_01 (release)"
  ./scripts/build_bookworm_release.sh shard_01
fi

if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

release_sha256="$(sha256sum "$bin_src" | awk '{print $1}')"
aws_region="${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}}"
release_s3_uri="${SLOPMUD_RELEASE_S3_URI:-}"

stage_release_from_s3_node() {
  local i="$1"
  local host="${node_hosts[$i]}"
  local node_id="${node_ids[$i]}"
  local target="${raft_ssh_user}@${host}"
  local uri_b64
  uri_b64="$(printf '%s' "$release_s3_uri" | base64 | tr -d '\n')"
  echo "Pulling shard_01 from S3 on ${node_id} (${host}) -> ${remote_release_bin}"
  ssh "${ssh_opts[@]}" "$target" "\
    set -euo pipefail; \
    release_uri=\$(printf %s '${uri_b64}' | base64 -d); \
    if ! id -u slopmud >/dev/null 2>&1; then sudo useradd --system --home '${REMOTE_ROOT}' --create-home --shell /usr/sbin/nologin slopmud; fi; \
    sudo mkdir -p '${REMOTE_ROOT}' '${remote_bin_dir}' '${release_dir}' '${REMOTE_ROOT}/var'; \
    sudo chown -R slopmud:slopmud '${REMOTE_ROOT}'; \
    if ! command -v aws >/dev/null 2>&1; then \
      if command -v apt-get >/dev/null 2>&1; then \
        sudo DEBIAN_FRONTEND=noninteractive apt-get update -y >/dev/null; \
        sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates awscli >/dev/null; \
      elif command -v dnf >/dev/null 2>&1; then \
        sudo dnf -y install ca-certificates awscli >/dev/null; \
      else \
        echo 'aws CLI is required for S3 deploys' >&2; \
        exit 1; \
      fi; \
    fi; \
    tmp='/tmp/shard_01.${release_id}'; \
    AWS_REGION='${aws_region}' AWS_DEFAULT_REGION='${aws_region}' aws s3 cp \"\$release_uri\" \"\$tmp\" --only-show-errors; \
    actual=\$(sha256sum \"\$tmp\" | awk '{print \$1}'); \
    if [ \"\$actual\" != '${release_sha256}' ]; then \
      echo \"checksum mismatch for \$release_uri: expected ${release_sha256}, got \$actual\" >&2; \
      rm -f \"\$tmp\"; \
      exit 1; \
    fi; \
    sudo install -m 0755 -o root -g root \"\$tmp\" '${remote_release_bin}'; \
    sudo ln -sfn '${remote_release_bin}' '${SHARD_REMOTE_BIN}.next'; \
    sudo mv -Tf '${SHARD_REMOTE_BIN}.next' '${SHARD_REMOTE_BIN}'; \
    rm -f \"\$tmp\" \
  "
}

if [[ "$deploy_from_s3" == "1" ]]; then
  if ! command -v aws >/dev/null 2>&1; then
    echo "ERROR: aws CLI is required locally for SLOPMUD_DEPLOY_FROM_S3=1" >&2
    exit 2
  fi
  if [[ -z "$release_s3_uri" ]]; then
    if [[ -n "${SLOPMUD_RELEASE_S3_BUCKET:-}" ]]; then
      release_s3_bucket="$SLOPMUD_RELEASE_S3_BUCKET"
    elif [[ -n "${ASSETS_BUCKET:-}" ]]; then
      release_s3_bucket="$ASSETS_BUCKET"
    else
      account_id="$(aws sts get-caller-identity --query Account --output text)"
      release_s3_bucket="slopmud-assets-${account_id}-${aws_region}"
    fi
    release_track="${SLOPMUD_RELEASE_TRACK:-${ENV_NAME:-split-raft}}"
    release_s3_prefix="${SLOPMUD_RELEASE_S3_PREFIX:-split-raft/${release_track}/${release_id}}"
    release_s3_prefix="${release_s3_prefix#/}"
    release_s3_prefix="${release_s3_prefix%/}"
    if ! [[ "$release_s3_prefix" =~ ^[A-Za-z0-9._=/-]+$ ]]; then
      echo "ERROR: SLOPMUD_RELEASE_S3_PREFIX contains unsupported characters: ${release_s3_prefix}" >&2
      exit 2
    fi
    release_s3_uri="s3://${release_s3_bucket}/${release_s3_prefix}/shard_01"
  fi
  if ! [[ "$release_s3_uri" =~ ^s3://[A-Za-z0-9._=/-]+$ ]]; then
    echo "ERROR: SLOPMUD_RELEASE_S3_URI contains unsupported characters: ${release_s3_uri}" >&2
    exit 2
  fi
  echo "Uploading release artifact -> ${release_s3_uri}"
  aws s3 cp "$bin_src" "$release_s3_uri" --only-show-errors
  printf '%s  shard_01\n' "$release_sha256" >"${tmp_dir}/shard_01.sha256"
  aws s3 cp "${tmp_dir}/shard_01.sha256" "${release_s3_uri}.sha256" --only-show-errors

  echo "Prefetching release from S3 on all Raft nodes"
  pids=()
  for i in 0 1 2; do
    stage_release_from_s3_node "$i" &
    pids+=("$!")
  done
  wait_jobs "S3 release prefetch" "${pids[@]}"
fi

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
  if [[ -n "${SHARD_RAFT_APPLICATION_MAX_FORMAT:-}" ]]; then
    echo "Environment=SHARD_RAFT_APPLICATION_MAX_FORMAT=${SHARD_RAFT_APPLICATION_MAX_FORMAT}" >>"$tmp_unit"
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
    sudo mkdir -p '${REMOTE_ROOT}' '${remote_bin_dir}' '${release_dir}' '${REMOTE_ROOT}/var'; \
    sudo chown -R slopmud:slopmud '${REMOTE_ROOT}' \
  "

  if [[ "$deploy_from_s3" == "1" ]]; then
    echo "Using S3-prefetched shard_01 on ${node_id} (${host}) at ${remote_release_bin}"
  elif [[ "${SLOPMUD_ATOMIC_BIN_SWAP:-1}" == "1" ]]; then
    echo "Uploading shard_01 -> ${target}:${remote_release_bin}"
    scp "${scp_opts[@]}" "$bin_src" "${target}:/tmp/shard_01.${release_id}"
    ssh "${ssh_opts[@]}" "$target" "\
      set -euo pipefail; \
      sudo install -m 0755 -o root -g root '/tmp/shard_01.${release_id}' '${remote_release_bin}'; \
      sudo ln -sfn '${remote_release_bin}' '${SHARD_REMOTE_BIN}.next'; \
      sudo mv -Tf '${SHARD_REMOTE_BIN}.next' '${SHARD_REMOTE_BIN}'; \
      sudo rm -f '/tmp/shard_01.${release_id}' \
    "
  else
    echo "Uploading shard_01 -> ${target}:${SHARD_REMOTE_BIN}"
    scp "${scp_opts[@]}" "$bin_src" "${target}:/tmp/shard_01"
    ssh "${ssh_opts[@]}" "$target" "\
      set -euo pipefail; \
      sudo install -m 0755 -o root -g root /tmp/shard_01 '${SHARD_REMOTE_BIN}'; \
      sudo rm -f /tmp/shard_01 \
    "
  fi

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
if [[ -n "$leader_i" && "$leader_i" != "$active_i" ]]; then
  echo "Current Raft leader: ${node_ids[$leader_i]} (${node_hosts[$leader_i]})"
fi

restart_order=()
for i in 0 1 2; do
  if [[ "$i" != "$active_i" && ( -z "$leader_i" || "$i" != "$leader_i" ) ]]; then
    restart_order+=("$i")
  fi
done
if [[ -n "$leader_i" && "$leader_i" != "$active_i" ]]; then
  restart_order+=("$active_i")
  restart_order+=("$leader_i")
else
  restart_order+=("$active_i")
fi

for i in "${restart_order[@]}"; do
  current_i="$(current_leader_index || true)"
  if [[ -z "$current_i" ]]; then
      if [[ "$(status_supported_count)" == "3" ]]; then
        wait_for_leader ""
        current_i="$(current_leader_index || true)"
      fi
      if [[ -z "$current_i" && "${SLOPMUD_STRICT_LIVE_UPGRADE:-0}" == "1" ]]; then
        wait_for_leader ""
        current_i="$(current_leader_index || true)"
      fi
      if [[ -z "$current_i" && "${SLOPMUD_STRICT_LIVE_UPGRADE:-0}" == "1" ]]; then
        echo "ERROR: refusing live upgrade restart without visible Raft leader" >&2
        exit 1
      fi
  fi
  if [[ "$current_i" == "$i" ]]; then
      target_i=$(((i + 1) % 3))
      if [[ "${SLOPMUD_ROLLING_TRANSFER_LEADER:-1}" != "0" ]]; then
        if try_transfer_leader "$i" "$target_i"; then
          wait_for_leader "$target_i"
        else
          if [[ "${SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART:-1}" == "0" ]]; then
            echo "ERROR: refusing to restart active leader without transfer" >&2
            exit 1
          fi
          echo "WARN: restarting leader without graceful transfer; this should only happen during the first rollout from older binaries" >&2
        fi
      fi
  elif [[ "$i" == "$active_i" && -n "$current_i" ]]; then
      echo "Leadership is on ${node_ids[$current_i]} (${node_hosts[$current_i]}); restarting old gateway-connected node ${node_ids[$active_i]}"
  elif [[ "$i" == "$active_i" ]]; then
      echo "WARN: no Raft leader visible before gateway-connected node restart" >&2
  fi
  acquire_restart_lease "$i"
  if [[ "${SLOPMUD_QUORUM_RESTART_GUARD:-1}" == "1" ]]; then
    if ! guard_quorum_before_restart "$i"; then
      release_restart_lease "$i"
      exit 1
    fi
  fi
  if ! restart_node "$i"; then
    release_restart_lease "$i"
    exit 1
  fi
  if ! wait_cluster_ready; then
    release_restart_lease "$i"
    exit 1
  fi
  release_restart_lease "$i"
done

shard_addrs_list=()
for host in "${node_hosts[@]}"; do
  shard_addrs_list+=("${host}:${shard_port}")
done
shard_addrs="$(IFS=','; echo "${shard_addrs_list[*]}")"
echo "OK: split Raft trio deployed"
echo "SHARD_ADDRS=${shard_addrs}"
