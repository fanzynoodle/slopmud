#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  k8s_raft_fast_restart.sh [namespace] [statefulset]

App-aware Kubernetes restart for a three-voter shard_01 Raft StatefulSet.
Kubernetes gets a 10 second default rolling budget; bare-metal split deploys
keep their separate 5 second default budget.

Optional env:
  KUBECTL                                  default kubectl
  SHARD_RAFT_PORT                          default 5001
  SHARD_RAFT_NODE_IDS                      default n0,n1,n2
  SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS    default 10000
  SLOPMUD_K8S_POLL_SLEEP_S                 default 0.10
  SLOPMUD_K8S_LEADER_WAIT_TIMEOUT_S        default 10
  SLOPMUD_K8S_CLUSTER_READY_TIMEOUT_S      default 10
  SLOPMUD_K8S_RESTART_LEASE_TIMEOUT_S      default 10
  SLOPMUD_K8S_RAFT_RESTART_LEASE           default required; required|auto|off
EOF
}

namespace="${1:-${NAMESPACE:-slopmud}}"
statefulset="${2:-${STATEFULSET:-}}"
if [[ "$namespace" == "-h" || "$namespace" == "--help" ]]; then
  usage
  exit 0
fi

kubectl_cmd="${KUBECTL:-kubectl}"
raft_port="${SHARD_RAFT_PORT:-5001}"
node_ids_csv="${SHARD_RAFT_NODE_IDS:-}"
budget_ms="${SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS:-10000}"
poll_sleep_s="${SLOPMUD_K8S_POLL_SLEEP_S:-0.10}"
leader_wait_timeout_s="${SLOPMUD_K8S_LEADER_WAIT_TIMEOUT_S:-10}"
cluster_ready_timeout_s="${SLOPMUD_K8S_CLUSTER_READY_TIMEOUT_S:-10}"
restart_lease_timeout_s="${SLOPMUD_K8S_RESTART_LEASE_TIMEOUT_S:-10}"
lease_mode="${SLOPMUD_K8S_RAFT_RESTART_LEASE:-required}"

if ! [[ "$budget_ms" =~ ^[0-9]+$ ]] || [[ "$budget_ms" == "0" ]]; then
  echo "ERROR: SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
for pair in \
  "SLOPMUD_K8S_LEADER_WAIT_TIMEOUT_S:${leader_wait_timeout_s}" \
  "SLOPMUD_K8S_CLUSTER_READY_TIMEOUT_S:${cluster_ready_timeout_s}" \
  "SLOPMUD_K8S_RESTART_LEASE_TIMEOUT_S:${restart_lease_timeout_s}"
do
  name="${pair%%:*}"
  value="${pair#*:}"
  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" == "0" ]]; then
    echo "ERROR: ${name} must be a positive integer" >&2
    exit 2
  fi
done
case "$lease_mode" in
  required|auto|off|0|false|no) ;;
  *)
    echo "ERROR: SLOPMUD_K8S_RAFT_RESTART_LEASE must be required, auto, or off" >&2
    exit 2
    ;;
esac

now_ms() {
  date +%s%3N
}

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  IFS=',' read -r -a out_ref <<<"$raw"
  local i
  for i in "${!out_ref[@]}"; do
    out_ref[$i]="$(printf '%s' "${out_ref[$i]}" | xargs)"
  done
}

if [[ -z "$statefulset" ]]; then
  statefulset="$("$kubectl_cmd" -n "$namespace" get statefulsets \
    -l app.kubernetes.io/component=shard-01 \
    -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)"
fi
if [[ -z "$statefulset" ]]; then
  echo "ERROR: statefulset not provided and auto-detect found none" >&2
  exit 2
fi

replicas="$("$kubectl_cmd" -n "$namespace" get statefulset "$statefulset" -o jsonpath='{.spec.replicas}')"
if [[ "${replicas:-0}" != "3" ]]; then
  echo "ERROR: ${namespace}/${statefulset} must have exactly 3 replicas for guarded Raft restart; got ${replicas:-unknown}" >&2
  exit 2
fi

if [[ -z "$node_ids_csv" ]]; then
  node_ids_csv="${statefulset}-0,${statefulset}-1,${statefulset}-2"
fi
split_csv "$node_ids_csv" node_ids
if [[ "${#node_ids[@]}" != "3" ]]; then
  echo "ERROR: SHARD_RAFT_NODE_IDS must contain exactly three comma-separated ids" >&2
  exit 2
fi

json_field() {
  local json="$1"
  local field="$2"
  printf '%s\n' "$json" | sed -nE "s/.*\"${field}\"[[:space:]]*:[[:space:]]*\"([^\"]*)\".*/\1/p"
}

json_bool_true() {
  local json="$1"
  local field="$2"
  printf '%s\n' "$json" | grep -Eq "\"${field}\"[[:space:]]*:[[:space:]]*true"
}

pod_for_ordinal() {
  printf '%s-%s' "$statefulset" "$1"
}

raft_rpc_ordinal() {
  local ordinal="$1"
  local payload="$2"
  local pod payload_b64
  pod="$(pod_for_ordinal "$ordinal")"
  payload_b64="$(printf '%s' "$payload" | base64 | tr -d '\n')"
  "$kubectl_cmd" -n "$namespace" exec "$pod" -- \
    env PAYLOAD_B64="$payload_b64" RAFT_PORT="$raft_port" bash -lc \
      'set -euo pipefail; payload=$(printf %s "$PAYLOAD_B64" | base64 -d); exec 3<>/dev/tcp/127.0.0.1/"$RAFT_PORT"; printf "%s\n" "$payload" >&3; IFS= read -r -t 5 line <&3; printf "%s\n" "$line"'
}

raft_status_ordinal() {
  raft_rpc_ordinal "$1" '{"t":"StatusReq"}'
}

current_leader_ordinal() {
  local i resp kind role
  for i in 0 1 2; do
    resp="$(raft_status_ordinal "$i" 2>/dev/null || true)"
    kind="$(json_field "$resp" t)"
    role="$(json_field "$resp" role)"
    if [[ "$kind" == "StatusResp" && "$role" == "Leader" ]]; then
      printf '%s\n' "$i"
      return 0
    fi
  done
  return 1
}

wait_for_leader() {
  local expected="${1:-}"
  local deadline_ms=$(( $(now_ms) + leader_wait_timeout_s * 1000 ))
  local leader
  while (( $(now_ms) < deadline_ms )); do
    leader="$(current_leader_ordinal || true)"
    if [[ -n "$leader" && ( -z "$expected" || "$leader" == "$expected" ) ]]; then
      echo "Kubernetes Raft leader is ${node_ids[$leader]} ($(pod_for_ordinal "$leader"))"
      return 0
    fi
    sleep "$poll_sleep_s"
  done
  echo "ERROR: timed out waiting for Kubernetes Raft leader ${expected:-any}" >&2
  return 1
}

wait_cluster_ready() {
  local deadline_ms=$(( $(now_ms) + cluster_ready_timeout_s * 1000 ))
  local i resp kind leader status_seen
  while (( $(now_ms) < deadline_ms )); do
    leader=""
    status_seen=0
    for i in 0 1 2; do
      resp="$(raft_status_ordinal "$i" 2>/dev/null || true)"
      kind="$(json_field "$resp" t)"
      if [[ "$kind" == "StatusResp" ]]; then
        status_seen=$((status_seen + 1))
        if [[ "$(json_field "$resp" role)" == "Leader" ]]; then
          leader="$i"
        fi
      fi
    done
    if [[ "$status_seen" == "3" && -n "$leader" ]]; then
      echo "Kubernetes Raft cluster ready: 3/3 voters visible, leader=${node_ids[$leader]}"
      return 0
    fi
    sleep "$poll_sleep_s"
  done
  echo "ERROR: timed out waiting for Kubernetes Raft cluster readiness" >&2
  return 1
}

try_transfer_leader() {
  local from_i="$1"
  local target_i="$2"
  local resp
  echo "Requesting Kubernetes Raft leadership transfer ${node_ids[$from_i]} -> ${node_ids[$target_i]}"
  resp="$(raft_rpc_ordinal "$from_i" "{\"t\":\"TransferLeaderReq\",\"target_id\":\"${node_ids[$target_i]}\"}" 2>/dev/null || true)"
  if [[ "$(json_field "$resp" t)" == "TransferLeaderResp" ]] && json_bool_true "$resp" accepted; then
    wait_for_leader "$target_i"
    return 0
  fi
  echo "ERROR: leadership transfer was not accepted: ${resp:-no response}" >&2
  return 1
}

restart_lease_tokens=("" "" "")

acquire_restart_lease() {
  local target_i="$1"
  local ttl_ms="${SLOPMUD_K8S_RAFT_RESTART_LEASE_TTL_MS:-60000}"
  local token deadline_ms leader_i resp kind reason
  case "$lease_mode" in
    off|0|false|no) return 0 ;;
  esac
  token="k8s-${statefulset}-${node_ids[$target_i]}-$$-${RANDOM}-${RANDOM}"
  deadline_ms=$(( $(now_ms) + restart_lease_timeout_s * 1000 ))
  while (( $(now_ms) < deadline_ms )); do
    leader_i="$(current_leader_ordinal || true)"
    if [[ -z "$leader_i" ]]; then
      sleep "$poll_sleep_s"
      continue
    fi
    resp="$(raft_rpc_ordinal "$leader_i" "{\"t\":\"RestartLeaseReq\",\"node_id\":\"${node_ids[$target_i]}\",\"token\":\"${token}\",\"ttl_ms\":${ttl_ms}}" 2>/dev/null || true)"
    kind="$(json_field "$resp" t)"
    if [[ "$kind" == "RestartLeaseResp" ]] && json_bool_true "$resp" accepted; then
      restart_lease_tokens[$target_i]="$token"
      echo "Kubernetes Raft restart lease acquired for ${node_ids[$target_i]} via ${node_ids[$leader_i]}"
      return 0
    fi
    if [[ "$kind" != "RestartLeaseResp" && "$lease_mode" == "auto" ]]; then
      echo "WARN: restart lease unsupported by current leader; falling back to quorum guard"
      return 0
    fi
    reason="$(json_field "$resp" reason)"
    if [[ "$reason" != "another restart lease is active" && "$reason" != "another restart lease won the race" ]]; then
      echo "ERROR: restart lease rejected for ${node_ids[$target_i]}: ${reason:-${resp:-no response}}" >&2
      return 1
    fi
    sleep "$poll_sleep_s"
  done
  echo "ERROR: timed out acquiring Kubernetes restart lease for ${node_ids[$target_i]}" >&2
  return 1
}

release_restart_lease() {
  local target_i="$1"
  local token="${restart_lease_tokens[$target_i]:-}"
  local leader_i resp
  if [[ -z "$token" ]]; then
    return 0
  fi
  leader_i="$(current_leader_ordinal || true)"
  if [[ -z "$leader_i" ]]; then
    echo "WARN: no leader visible while releasing restart lease for ${node_ids[$target_i]}; lease will expire" >&2
    return 0
  fi
  resp="$(raft_rpc_ordinal "$leader_i" "{\"t\":\"RestartLeaseReleaseReq\",\"node_id\":\"${node_ids[$target_i]}\",\"token\":\"${token}\"}" 2>/dev/null || true)"
  restart_lease_tokens[$target_i]=""
  if [[ "$(json_field "$resp" t)" == "RestartLeaseReleaseResp" ]] && json_bool_true "$resp" accepted; then
    echo "Kubernetes Raft restart lease released for ${node_ids[$target_i]}"
    return 0
  fi
  echo "WARN: restart lease release for ${node_ids[$target_i]} was not accepted: ${resp:-no response}" >&2
}

guard_quorum_before_restart() {
  local candidate_i="$1"
  local i resp kind role other_seen leader
  other_seen=0
  leader=""
  for i in 0 1 2; do
    resp="$(raft_status_ordinal "$i" 2>/dev/null || true)"
    kind="$(json_field "$resp" t)"
    role="$(json_field "$resp" role)"
    if [[ "$kind" == "StatusResp" ]]; then
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
  echo "Kubernetes quorum guard: ${node_ids[$candidate_i]} can restart; ${other_seen}/2 remaining voters visible, leader=${node_ids[$leader]}"
}

restart_pod() {
  local ordinal="$1"
  local pod old_uid deadline_ms uid ready
  pod="$(pod_for_ordinal "$ordinal")"
  old_uid="$("$kubectl_cmd" -n "$namespace" get pod "$pod" -o jsonpath='{.metadata.uid}' 2>/dev/null || true)"
  echo "Deleting ${namespace}/${pod} for app-aware fast restart"
  "$kubectl_cmd" -n "$namespace" delete pod "$pod" --grace-period=0 --force --wait=false
  deadline_ms=$(( $(now_ms) + cluster_ready_timeout_s * 1000 ))
  while (( $(now_ms) < deadline_ms )); do
    uid="$("$kubectl_cmd" -n "$namespace" get pod "$pod" -o jsonpath='{.metadata.uid}' 2>/dev/null || true)"
    ready="$("$kubectl_cmd" -n "$namespace" get pod "$pod" -o jsonpath='{range .status.conditions[?(@.type=="Ready")]}{.status}{end}' 2>/dev/null || true)"
    if [[ -n "$uid" && "$uid" != "$old_uid" && "$ready" == "True" ]]; then
      echo "${namespace}/${pod} is Ready after replacement"
      return 0
    fi
    sleep "$poll_sleep_s"
  done
  echo "ERROR: timed out waiting for ${namespace}/${pod} replacement to become Ready" >&2
  return 1
}

wait_for_leader ""
leader_i="$(current_leader_ordinal || true)"
if [[ -z "$leader_i" ]]; then
  echo "ERROR: cannot plan restart order without a visible leader" >&2
  exit 1
fi

restart_order=()
for i in 0 1 2; do
  if [[ "$i" != "$leader_i" ]]; then
    restart_order+=("$i")
  fi
done
restart_order+=("$leader_i")

echo "Kubernetes fast restart budget_ms=${budget_ms} statefulset=${namespace}/${statefulset}"
rolling_start_ms="$(now_ms)"
for i in "${restart_order[@]}"; do
  node_start_ms="$(now_ms)"
  current_i="$(current_leader_ordinal || true)"
  if [[ "$current_i" == "$i" ]]; then
    target_i=$(((i + 1) % 3))
    try_transfer_leader "$i" "$target_i"
  fi
  acquire_restart_lease "$i"
  if ! guard_quorum_before_restart "$i"; then
    release_restart_lease "$i"
    exit 1
  fi
  if ! restart_pod "$i"; then
    release_restart_lease "$i"
    exit 1
  fi
  if ! wait_cluster_ready; then
    release_restart_lease "$i"
    exit 1
  fi
  release_restart_lease "$i"
  node_elapsed_ms=$(( $(now_ms) - node_start_ms ))
  echo "Kubernetes rolling restart node ${node_ids[$i]} elapsed_ms=${node_elapsed_ms}"
done
rolling_elapsed_ms=$(( $(now_ms) - rolling_start_ms ))
echo "Kubernetes rolling restart elapsed_ms=${rolling_elapsed_ms}"
if [[ "$rolling_elapsed_ms" -gt "$budget_ms" ]]; then
  echo "ERROR: Kubernetes rolling restart exceeded budget_ms=${budget_ms} elapsed_ms=${rolling_elapsed_ms}" >&2
  exit 1
fi
