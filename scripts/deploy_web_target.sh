#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
env_name="${2:-prd}"

if [[ -z "${target}" ]]; then
  echo "usage: $0 <landing|webportal|both> [env]" >&2
  exit 2
fi

case "${target}" in
  landing)
    just deploy "${env_name}_landing"
    just https-smoke "${env_name}_landing"
    ;;
  webportal)
    just deploy "${env_name}"
    just https-smoke "${env_name}"
    ;;
  both)
    just deploy "${env_name}_landing"
    just https-smoke "${env_name}_landing"
    just deploy "${env_name}"
    just https-smoke "${env_name}"
    ;;
  *)
    echo "unknown target: ${target}" >&2
    echo "expected one of: landing, webportal, both" >&2
    exit 2
    ;;
esac
