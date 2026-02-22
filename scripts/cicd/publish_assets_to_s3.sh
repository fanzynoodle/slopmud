#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  publish_assets_to_s3.sh /path/to/env/<dev|stg|prd|sandbox>.env

Builds a slopmud artifact (same format as CI) and uploads it to the assets bucket:
  s3://slopmud-assets-<account>-<region>/<track>/<sha>/artifact.tgz

Track mapping:
  prd -> prod
  dev/stg/sandbox -> same as ENV_NAME
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

: "${ENV_NAME:?missing ENV_NAME in env file}"

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

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

artifact_path="$(
  TRACK="$track" \
  CLEAN_BUILD="${CLEAN_BUILD:-0}" \
  ASSETS_ROOT="${ASSETS_ROOT:-assets}" \
  ./scripts/cicd/build_assets.sh
)"
sha="$(basename "$(dirname "$artifact_path")")"
key="${track}/${sha}/artifact.tgz"
s3_uri="s3://${bucket}/${key}"

echo "Uploading artifact -> ${s3_uri}"
aws s3 cp "$artifact_path" "$s3_uri"

echo "OK: uploaded ${s3_uri}"
echo "$s3_uri"
