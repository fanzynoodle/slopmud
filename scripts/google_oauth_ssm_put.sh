#!/usr/bin/env bash
set -euo pipefail

env_name="${1:-}"
if [[ -z "${env_name}" ]]; then
  echo "USAGE: $0 <env>" >&2
  echo "  env: prd | stg | dev (or any string used in your SSM path prefix)" >&2
  exit 2
fi

prefix="${SSM_PREFIX:-/slopmud/${env_name}}"
id_name="${GOOGLE_OAUTH_CLIENT_ID_SSM_NAME:-${prefix}/google_oauth_client_id}"
secret_name="${GOOGLE_OAUTH_CLIENT_SECRET_SSM_NAME:-${prefix}/google_oauth_client_secret}"

if [[ -n "${GOOGLE_OAUTH_CLIENT_ID:-}" && -n "${GOOGLE_OAUTH_CLIENT_SECRET:-}" ]]; then
  client_id="${GOOGLE_OAUTH_CLIENT_ID}"
  client_secret="${GOOGLE_OAUTH_CLIENT_SECRET}"
else
  read -r -p "Google OAuth Client ID: " client_id
  read -r -s -p "Google OAuth Client Secret: " client_secret
  echo
fi

if [[ -z "${client_id}" || -z "${client_secret}" ]]; then
  echo "ERROR: missing client id/secret" >&2
  exit 2
fi

echo "Writing SSM parameters:"
echo "  ${id_name}"
echo "  ${secret_name}"

aws ssm put-parameter \
  --name "${id_name}" \
  --type SecureString \
  --value "${client_id}" \
  --overwrite >/dev/null

aws ssm put-parameter \
  --name "${secret_name}" \
  --type SecureString \
  --value "${client_secret}" \
  --overwrite >/dev/null

echo "OK"
