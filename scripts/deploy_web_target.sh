#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
env_name="${2:-prd}"

if [[ -z "${target}" ]]; then
  echo "usage: $0 <landing|webportal|both> [env]" >&2
  exit 2
fi

env_exists() {
  [[ -f "env/$1.env" ]]
}

resolve_landing_env() {
  local base="$1"
  local override="${SLOPMUD_LANDING_ENV:-}"
  if [[ -n "${override}" ]]; then
    echo "${override}"
    return
  fi
  for candidate in "${base}_landing" "${base}-landing"; do
    if env_exists "${candidate}"; then
      echo "${candidate}"
      return
    fi
  done
  echo "${base}_landing"
}

resolve_webportal_env() {
  local base="$1"
  local override="${SLOPMUD_WEBPORTAL_ENV:-}"
  if [[ -n "${override}" ]]; then
    echo "${override}"
    return
  fi
  for candidate in "${base}_webportal" "${base}-webportal" "${base}-oauth" "${base}_oauth" "${base}"; do
    if env_exists "${candidate}"; then
      echo "${candidate}"
      return
    fi
  done
  echo "${base}"
}

landing_env="$(resolve_landing_env "${env_name}")"
webportal_env="$(resolve_webportal_env "${env_name}")"

case "${target}" in
  landing)
    just deploy "${landing_env}"
    just https-smoke "${landing_env}"
    ;;
  webportal)
    just deploy "${webportal_env}"
    just https-smoke "${webportal_env}"
    ;;
  both)
    just deploy "${landing_env}"
    just https-smoke "${landing_env}"
    just deploy "${webportal_env}"
    just https-smoke "${webportal_env}"
    ;;
  *)
    echo "unknown target: ${target}" >&2
    echo "expected one of: landing, webportal, both" >&2
    exit 2
    ;;
esac
