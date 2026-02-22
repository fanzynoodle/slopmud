#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  deploy_slopmud_from_s3.sh /path/to/env/<dev|stg|prd|sandbox>.env [SHA|s3://BUCKET/KEY]

Deploys using /usr/local/bin/slopmud-shuttle-assets on the remote host.

If the second arg is omitted, deploys the latest artifact key under the env track.
Track mapping:
  prd -> prod
  dev/stg/sandbox -> same as ENV_NAME
EOF
}

env_file="${1:-}"
artifact_ref="${2:-}"

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

: "${ENV_NAME:?missing ENV_NAME in env file}"
: "${HOST:?missing HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${SLOPMUD_BIND:?missing SLOPMUD_BIND in env file}"

case "$ENV_NAME" in
  prd) track="prod" ;;
  dev|stg|sandbox) track="$ENV_NAME" ;;
  *)
    echo "ERROR: unsupported ENV_NAME: $ENV_NAME" >&2
    exit 2
    ;;
esac

aws_region="${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}}"
account_id="$(aws sts get-caller-identity --query Account --output text)"
bucket="${ASSETS_BUCKET:-slopmud-assets-${account_id}-${aws_region}}"

if [[ -z "$artifact_ref" ]]; then
  key="$(aws s3api list-objects-v2 \
    --bucket "$bucket" \
    --prefix "${track}/" \
    --query 'reverse(sort_by(Contents,&LastModified))[?ends_with(Key, `artifact.tgz`)].Key | [0]' \
    --output text)"
  if [[ -z "$key" || "$key" == "None" ]]; then
    echo "ERROR: no artifact found under s3://${bucket}/${track}/" >&2
    exit 1
  fi
  s3_uri="s3://${bucket}/${key}"
elif [[ "$artifact_ref" == s3://*/* ]]; then
  s3_uri="$artifact_ref"
else
  s3_uri="s3://${bucket}/${track}/${artifact_ref}/artifact.tgz"
fi

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")

echo "Deploying ${s3_uri} to ${ENV_NAME} on ${HOST}"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo /usr/local/bin/slopmud-shuttle-assets --env \"${ENV_NAME}\" --from-s3 \"${s3_uri}\"; \
"

port="${SLOPMUD_BIND##*:}"
echo "Listening check (port ${port})"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo ss -lnt | grep -qE ':${port}([[:space:]]|$)'; \
"

echo "OK: deployed ${s3_uri}"
