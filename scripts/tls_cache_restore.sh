#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  tls_cache_restore.sh /path/to/env.env

Restores cached TLS material from SSM Parameter Store onto a remote host using
the host's IAM role. The env file must define HOST, SSH_USER, SSH_PORT,
REMOTE_ROOT, TLS_DST_DIR, TLS_CACHE_FULLCHAIN_SSM, and TLS_CACHE_PRIVKEY_SSM.
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

: "${HOST:?missing HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
: "${TLS_DST_DIR:?missing TLS_DST_DIR in env file}"
: "${TLS_CACHE_FULLCHAIN_SSM:?missing TLS_CACHE_FULLCHAIN_SSM in env file}"
: "${TLS_CACHE_PRIVKEY_SSM:?missing TLS_CACHE_PRIVKEY_SSM in env file}"

ssh_opts=(-o StrictHostKeyChecking=accept-new -p "$SSH_PORT")

echo "Restoring TLS cache on ${SSH_USER}@${HOST}:${TLS_DST_DIR}"
ssh "${ssh_opts[@]}" "${SSH_USER}@${HOST}" \
  TLS_DST_DIR="$TLS_DST_DIR" \
  TLS_CACHE_FULLCHAIN_SSM="$TLS_CACHE_FULLCHAIN_SSM" \
  TLS_CACHE_PRIVKEY_SSM="$TLS_CACHE_PRIVKEY_SSM" \
  REMOTE_ROOT="$REMOTE_ROOT" \
  'bash -s' <<'REMOTE'
set -euo pipefail

if command -v apt-get >/dev/null 2>&1; then
  sudo DEBIAN_FRONTEND=noninteractive apt-get update -y >/dev/null
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y awscli ca-certificates openssl >/dev/null
elif command -v dnf >/dev/null 2>&1; then
  sudo dnf -y install awscli ca-certificates openssl >/dev/null
else
  echo "Unsupported OS (need apt-get or dnf)" >&2
  exit 2
fi

if ! id -u slopmud >/dev/null 2>&1; then
  sudo useradd --system --home "${REMOTE_ROOT}" --create-home --shell /usr/sbin/nologin slopmud
fi

tmp="$(mktemp -d)"
trap 'sudo rm -rf "$tmp"' EXIT

aws ssm get-parameter \
  --name "${TLS_CACHE_FULLCHAIN_SSM}" \
  --with-decryption \
  --query Parameter.Value \
  --output text >"${tmp}/fullchain.pem"
aws ssm get-parameter \
  --name "${TLS_CACHE_PRIVKEY_SSM}" \
  --with-decryption \
  --query Parameter.Value \
  --output text >"${tmp}/privkey.pem"

openssl x509 -in "${tmp}/fullchain.pem" -noout -subject -issuer -dates >/dev/null
openssl pkey -in "${tmp}/privkey.pem" -noout >/dev/null

sudo install -d -o slopmud -g slopmud -m 0750 "${TLS_DST_DIR}"
sudo install -o slopmud -g slopmud -m 0640 "${tmp}/fullchain.pem" "${TLS_DST_DIR}/fullchain.pem"
sudo install -o slopmud -g slopmud -m 0640 "${tmp}/privkey.pem" "${TLS_DST_DIR}/privkey.pem"
sudo openssl x509 -in "${TLS_DST_DIR}/fullchain.pem" -noout -subject -enddate
REMOTE
