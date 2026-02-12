#!/usr/bin/env bash
set -euo pipefail

# Local demo stack:
# - shard_01
# - slopmud (broker)
# - internal_oidc (IdP)
# - slopmud_web (serves web_homepage + /api/oauth/* + /ws)
#
# Prints a URL to open. Ctrl+C to stop all processes.

pick_ports() {
  python3 scripts/alloc_port_block.py --range 5250-5349 --stride 5 --offsets 0,1,2,4
}

base="$(pick_ports)"
broker_bind="127.0.0.1:${base}"
shard_bind="127.0.0.1:$((base + 1))"
web_bind="127.0.0.1:$((base + 2))"
oidc_bind="127.0.0.1:$((base + 4))"

run_id="$(date +%s%N)"
users_path="/tmp/slopmud_internal_oidc_users_demo_${run_id}.json"
cat >"${users_path}" <<JSON
{"users":[]}
JSON

export WORLD_TICK_MS=200

export SLOPMUD_BIND="${broker_bind}"
export SHARD_BIND="${shard_bind}"
export SHARD_ADDR="${shard_bind}"
export SESSION_TCP_ADDR="${broker_bind}"

export OIDC_BIND="${oidc_bind}"
export OIDC_ISSUER="http://${oidc_bind}"
export OIDC_CLIENT_ID="slopmud-local"
export OIDC_CLIENT_SECRET="slopmud-local-secret"
export OIDC_USERS_PATH="${users_path}"
export OIDC_ALLOWED_REDIRECT_URIS="http://${web_bind}/auth/oidc/callback"
export OIDC_ALLOW_PLAINTEXT_PASSWORDS="1"
export OIDC_ALLOW_REGISTRATION="1"
export OIDC_ALLOW_PASSWORD_RESET="1"

export BIND="${web_bind}"
export STATIC_DIR="web_homepage"
export SLOPMUD_OIDC_SSO_AUTH_URL="http://${oidc_bind}/authorize"
export SLOPMUD_OIDC_SSO_TOKEN_URL="http://${oidc_bind}/token"
export SLOPMUD_OIDC_SSO_USERINFO_URL="http://${oidc_bind}/userinfo"
export SLOPMUD_OIDC_SSO_CLIENT_ID="${OIDC_CLIENT_ID}"
export SLOPMUD_OIDC_SSO_CLIENT_SECRET="${OIDC_CLIENT_SECRET}"
export SLOPMUD_OIDC_SSO_REDIRECT_URI="http://${web_bind}/auth/oidc/callback"

echo "Building (debug)..."
cargo build -q -p shard_01 -p slopmud -p internal_oidc -p slopmud_web

log_dir="/tmp"
shard_log="${log_dir}/slopmud_demo_slopsso_shard_${run_id}.log"
broker_log="${log_dir}/slopmud_demo_slopsso_broker_${run_id}.log"
oidc_log="${log_dir}/slopmud_demo_slopsso_oidc_${run_id}.log"
web_log="${log_dir}/slopmud_demo_slopsso_web_${run_id}.log"

cleanup() {
  kill "${pid_web:-}" "${pid_oidc:-}" "${pid_broker:-}" "${pid_shard:-}" 2>/dev/null || true
}
trap cleanup EXIT

./target/debug/shard_01 >"${shard_log}" 2>&1 & pid_shard=$!
sleep 0.6
./target/debug/slopmud >"${broker_log}" 2>&1 & pid_broker=$!
sleep 0.6
./target/debug/internal_oidc >"${oidc_log}" 2>&1 & pid_oidc=$!
sleep 0.6
./target/debug/slopmud_web --bind "${web_bind}" --dir web_homepage --session-tcp-addr "${broker_bind}" >"${web_log}" 2>&1 & pid_web=$!

echo
echo "SlopSSO demo running:"
echo "  web:  http://${web_bind}/play.html"
echo "  idp:  http://${oidc_bind}/"
echo "  register a user via the IdP (it will prompt for password twice)"
echo "  logs: ${web_log} ${broker_log} ${shard_log} ${oidc_log}"
echo

exec bash -lc "xdg-open http://${web_bind}/play.html >/dev/null 2>&1 || true; echo 'Ctrl+C to stop'; while true; do sleep 3600; done"
