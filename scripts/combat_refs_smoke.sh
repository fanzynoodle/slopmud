#!/usr/bin/env bash
set -euo pipefail

# Bring up shard_01 + ws_gateway and run the combat reference suite against it.
#
# Usage:
#   scripts/combat_refs_smoke.sh <env_file> [suite_path]
#
# Notes:
# - The reference scenarios currently use `warp`, so the shard must bootstrap the actors as admins.
#   We do that here by setting SHARD_BOOTSTRAP_ADMINS to the scenario actor names.

env_file="${1:-}"
suite="${2:-reference/combats/suite.json}"

if [[ -z "${env_file}" ]]; then
  echo "usage: $0 <env_file> [suite_path]" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

if [[ ! -f "${env_file}" ]]; then
  echo "missing env_file: ${env_file}" >&2
  exit 2
fi
if [[ ! -f "${suite}" ]]; then
  echo "missing suite: ${suite}" >&2
  exit 2
fi

echo "[build] shard_01 + ws_gateway + combat_refs"
cargo build -q -p shard_01 -p ws_gateway -p combat_refs

# Extract actor names from the suite (best-effort; keep it simple).
actors="$(python3 - "${suite}" <<'PY'
import json, sys
suite_path = sys.argv[1]
with open(suite_path, 'r', encoding='utf-8') as f:
  suite = json.load(f)
names = []
for sp in suite.get("scenarios", []):
  try:
    with open(sp, 'r', encoding='utf-8') as sf:
      sc = json.load(sf)
    for a in sc.get("actors", []):
      n = (a.get("name") or "").strip()
      if n:
        names.append(n)
  except Exception:
    pass
names = sorted(set(names), key=str.lower)
print(",".join(names))
PY
)"

cleanup() {
  set +e
  if [[ -n "${ws_pid:-}" ]]; then kill "${ws_pid}" 2>/dev/null || true; fi
  if [[ -n "${shard_pid:-}" ]]; then kill "${shard_pid}" 2>/dev/null || true; fi
  if [[ -n "${ws_pid:-}" ]]; then wait "${ws_pid}" 2>/dev/null || true; fi
  if [[ -n "${shard_pid:-}" ]]; then wait "${shard_pid}" 2>/dev/null || true; fi
}
trap cleanup EXIT

echo "[start] shard_01"
(
  set -a
  # shellcheck disable=SC1090
  source "${env_file}"
  set +a
  export SHARD_BOOTSTRAP_ADMINS="${actors}"
  ./target/debug/shard_01
) >/tmp/slopmud_combat_refs_shard.log 2>&1 &
shard_pid=$!

sleep 0.6

echo "[start] ws_gateway"
(
  set -a
  # shellcheck disable=SC1090
  source "${env_file}"
  set +a
  ./target/debug/ws_gateway
) >/tmp/slopmud_combat_refs_ws.log 2>&1 &
ws_pid=$!

sleep 0.6

set -a
# shellcheck disable=SC1090
source "${env_file}"
set +a

# Match ws_gateway defaults if the env file doesn't define WS_BIND.
ws_bind="${WS_BIND:-127.0.0.1:4100}"
ws_url="ws://${ws_bind}/v1/json"
echo "[run] suite=${suite} ws=${ws_url}"
./target/debug/combat_refs --ws "${ws_url}" --suite "${suite}"
