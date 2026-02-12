#!/usr/bin/env bash
set -euo pipefail

# Smoke-test running multiple local agent stacks concurrently without port collisions.
#
# This validates the port allocation scheme produced by scripts/mk_agent_env.sh by actually
# launching shard_01 + slopmud for N agents and verifying expected ports are listening.
#
# Usage:
#   scripts/agent_ports_smoke.sh [base_port] [step] [n]
#
# Defaults:
#   base_port=4940 step=100 n=4

base_port="${1:-4940}"
step="${2:-100}"
n="${3:-4}"

if ! [[ "${base_port}" =~ ^[0-9]+$ && "${step}" =~ ^[0-9]+$ && "${n}" =~ ^[0-9]+$ ]]; then
  echo "args must be integers: base_port step n" >&2
  exit 2
fi
if (( base_port < 1024 )); then
  echo "base_port must be >= 1024" >&2
  exit 2
fi
if (( step < 20 )); then
  echo "step must be >= 20 (avoid overlapping port blocks)" >&2
  exit 2
fi
if (( n < 1 || n > 16 )); then
  echo "n must be 1..16" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d /tmp/slopmud_agent_ports_smoke.XXXXXX)"

cleanup() {
  set +e
  if [[ -n "${pids:-}" ]]; then
    # shellcheck disable=SC2086
    kill ${pids} 2>/dev/null || true
    # shellcheck disable=SC2086
    wait ${pids} 2>/dev/null || true
  fi
  rm -rf "${tmp_dir}" 2>/dev/null || true
}
trap cleanup EXIT

cd "${root}"

echo "[build] shard_01 + slopmud"
cargo build -q -p shard_01 -p slopmud

declare -a env_files=()
declare -a agent_names=()

for i in $(seq 0 $((n - 1))); do
  name="$(printf "agent%02d" "${i}")"
  base=$((base_port + i * step))
  out="${tmp_dir}/${name}.env"
  ./scripts/mk_agent_env.sh "${name}" "${base}" "${out}" >/dev/null
  env_files+=("${out}")
  agent_names+=("${name}")
done

# Ensure the computed ports don't collide (within the port block layout).
declare -A used_ports=()
for f in "${env_files[@]}"; do
  # shellcheck disable=SC1090
  set -a; source "${f}"; set +a

  for v in SLOPMUD_BIND SHARD_BIND SLOPMUD_ADMIN_BIND STATIC_WEB_BIND OAUTH_WEB_BIND WS_BIND; do
    addr="${!v:-}"
    port="${addr##*:}"
    key="${port}"
    if [[ -n "${used_ports[$key]:-}" ]]; then
      echo "[fail] port collision: ${port} used by ${used_ports[$key]} and ${f}" >&2
      exit 1
    fi
    used_ports[$key]="${f}:${v}"
  done
done

ports_free() {
  local port="$1"
  if ss -ltn "( sport = :${port} )" | tail -n +2 | grep -q .; then
    return 1
  fi
  return 0
}

echo "[start] shard_01 x${n}"
pids=""
for idx in "${!env_files[@]}"; do
  f="${env_files[$idx]}"
  name="${agent_names[$idx]}"
  # shellcheck disable=SC1090
  set -a; source "${f}"; set +a

  shard_port="${SHARD_BIND##*:}"
  if ! ports_free "${shard_port}"; then
    echo "[fail] port already in use before start: SHARD_BIND=${SHARD_BIND} (${name})" >&2
    exit 1
  fi

  log="${tmp_dir}/${name}.shard.log"
  (cd "${root}" && ./target/debug/shard_01 >"${log}" 2>&1) &
  pids="${pids} $!"
done

sleep 0.8

echo "[start] slopmud x${n}"
for idx in "${!env_files[@]}"; do
  f="${env_files[$idx]}"
  name="${agent_names[$idx]}"
  # shellcheck disable=SC1090
  set -a; source "${f}"; set +a

  broker_port="${SLOPMUD_BIND##*:}"
  admin_port="${SLOPMUD_ADMIN_BIND##*:}"
  if ! ports_free "${broker_port}"; then
    echo "[fail] port already in use before start: SLOPMUD_BIND=${SLOPMUD_BIND} (${name})" >&2
    exit 1
  fi
  if ! ports_free "${admin_port}"; then
    echo "[fail] port already in use before start: SLOPMUD_ADMIN_BIND=${SLOPMUD_ADMIN_BIND} (${name})" >&2
    exit 1
  fi

  log="${tmp_dir}/${name}.broker.log"
  (cd "${root}" && ./target/debug/slopmud >"${log}" 2>&1) &
  pids="${pids} $!"
done

sleep 1.2

echo "[check] listening ports"
missing=0
for idx in "${!env_files[@]}"; do
  f="${env_files[$idx]}"
  name="${agent_names[$idx]}"
  # shellcheck disable=SC1090
  set -a; source "${f}"; set +a

  ok_one=1
  for addr in "${SLOPMUD_BIND}" "${SHARD_BIND}" "${SLOPMUD_ADMIN_BIND}"; do
    port="${addr##*:}"
    if ports_free "${port}"; then
      echo "[fail] expected listening, but not found: ${name} ${addr}" >&2
      ok_one=0
      missing=1
    fi
  done

  if (( ok_one == 0 )); then
    echo "[logs] ${name} shard tail:" >&2
    tail -n 20 "${tmp_dir}/${name}.shard.log" >&2 || true
    echo "[logs] ${name} broker tail:" >&2
    tail -n 20 "${tmp_dir}/${name}.broker.log" >&2 || true
  fi
done

if (( missing != 0 )); then
  echo "[result] FAIL" >&2
  exit 1
fi

echo "[result] OK (no port collisions; all expected listeners present)"
