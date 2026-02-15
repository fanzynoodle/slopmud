#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  hot_deploy_slopmud.sh /path/to/env/<dev|stg|prd>.env

Builds the same slopmud "asset" tarball as CI and deploys it to the target host by:
- scp'ing the tarball to /tmp
- running /usr/local/bin/slopmud-shuttle-assets on the host (via sudo)

NOTE:
  This script assumes the host has been bootstrapped once via scripts/cicd/bootstrap_runner.sh
  (or equivalent manual setup) so that slopmud-shuttle-assets is installed.
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
: "${HOST:?missing HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"

if [[ "${ENABLED:-1}" != "1" ]]; then
  echo "${ENV_NAME} disabled (ENABLED=${ENABLED:-})"
  exit 0
fi

case "$ENV_NAME" in
  dev|stg|prd) ;;
  *)
    echo "ERROR: unsupported ENV_NAME for hot deploy: $ENV_NAME (expected dev|stg|prd)" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

artifact_path="$(TRACK="$ENV_NAME" CLEAN_BUILD="${CLEAN_BUILD:-0}" ASSETS_ROOT="${ASSETS_ROOT:-assets}" ./scripts/cicd/build_assets.sh)"
sha="$(basename "$(dirname "$artifact_path")")"

remote_artifact="/tmp/slopmud-${ENV_NAME}-${sha}-artifact.tgz"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

echo "Uploading artifact -> ${SSH_USER}@${HOST}:${remote_artifact}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$artifact_path" "${SSH_USER}@${HOST}:${remote_artifact}"

echo "Deploying on host via slopmud-shuttle-assets (env=${ENV_NAME})"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo /usr/local/bin/slopmud-shuttle-assets --env \"${ENV_NAME}\" --from-file \"${remote_artifact}\"; \
  rm -f \"${remote_artifact}\"; \
"

echo "OK: hot deploy complete (env=${ENV_NAME}, sha=${sha})"
