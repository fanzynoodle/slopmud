#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  tls_cache_ssm.sh ensure --cert PATH --key PATH --fullchain-ssm NAME --privkey-ssm NAME [--min-valid-days 5] [--owner USER] [--group GROUP]
  tls_cache_ssm.sh restore --cert PATH --key PATH --fullchain-ssm NAME --privkey-ssm NAME [--owner USER] [--group GROUP]
  tls_cache_ssm.sh store --cert PATH --key PATH --fullchain-ssm NAME --privkey-ssm NAME

Subcommands:
  ensure   Use the local cert if it is still valid for N days; otherwise restore from SSM.
  restore  Restore both PEM files from SSM Parameter Store SecureStrings.
  store    Upload both PEM files into SSM Parameter Store SecureStrings.
EOF
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "ERROR: missing required command: $1" >&2
    exit 2
  fi
}

is_valid_for_days() {
  local cert_path="$1"
  local key_path="$2"
  local min_valid_days="$3"
  local min_valid_seconds=$((min_valid_days * 86400))

  [[ -s "$cert_path" && -s "$key_path" ]] || return 1
  openssl x509 -checkend "$min_valid_seconds" -noout -in "$cert_path" >/dev/null 2>&1
}

write_file() {
  local dst="$1"
  local content="$2"
  local owner="$3"
  local group="$4"
  local file_mode="$5"

  local dir
  dir="$(dirname "$dst")"
  install -d -m 0750 "$dir"

  local tmp
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' RETURN
  printf '%s\n' "$content" >"$tmp"
  install -m "$file_mode" "$tmp" "$dst"

  if [[ -n "$owner" || -n "$group" ]]; then
    chown "${owner:-$(stat -c %U "$dst")}":"${group:-$(stat -c %G "$dst")}" "$dst"
  fi
}

subcommand="${1:-}"
if [[ -z "$subcommand" ]]; then
  usage
  exit 2
fi
shift

cert_path=""
key_path=""
fullchain_ssm=""
privkey_ssm=""
min_valid_days="5"
owner=""
group=""
file_mode="0640"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cert)
      cert_path="${2:-}"; shift 2 ;;
    --key)
      key_path="${2:-}"; shift 2 ;;
    --fullchain-ssm)
      fullchain_ssm="${2:-}"; shift 2 ;;
    --privkey-ssm)
      privkey_ssm="${2:-}"; shift 2 ;;
    --min-valid-days)
      min_valid_days="${2:-}"; shift 2 ;;
    --owner)
      owner="${2:-}"; shift 2 ;;
    --group)
      group="${2:-}"; shift 2 ;;
    --file-mode)
      file_mode="${2:-}"; shift 2 ;;
    -h|--help)
      usage
      exit 0 ;;
    *)
      echo "ERROR: unknown arg: $1" >&2
      usage
      exit 2 ;;
  esac
done

: "${cert_path:?missing --cert}"
: "${key_path:?missing --key}"
: "${fullchain_ssm:?missing --fullchain-ssm}"
: "${privkey_ssm:?missing --privkey-ssm}"

need_cmd aws
need_cmd openssl

case "$subcommand" in
  ensure)
    if is_valid_for_days "$cert_path" "$key_path" "$min_valid_days"; then
      exit 0
    fi
    "$0" restore \
      --cert "$cert_path" \
      --key "$key_path" \
      --fullchain-ssm "$fullchain_ssm" \
      --privkey-ssm "$privkey_ssm" \
      --owner "$owner" \
      --group "$group" \
      --file-mode "$file_mode"
    is_valid_for_days "$cert_path" "$key_path" "$min_valid_days"
    ;;
  restore)
    cert_value="$(aws ssm get-parameter --name "$fullchain_ssm" --with-decryption --query Parameter.Value --output text)"
    key_value="$(aws ssm get-parameter --name "$privkey_ssm" --with-decryption --query Parameter.Value --output text)"
    write_file "$cert_path" "$cert_value" "$owner" "$group" "$file_mode"
    write_file "$key_path" "$key_value" "$owner" "$group" "$file_mode"
    ;;
  store)
    [[ -s "$cert_path" && -s "$key_path" ]] || {
      echo "ERROR: both cert and key files must exist for store" >&2
      exit 2
    }
    cert_value="$(cat "$cert_path")"
    key_value="$(cat "$key_path")"
    aws ssm put-parameter --name "$fullchain_ssm" --type SecureString --overwrite --value "$cert_value" >/dev/null
    aws ssm put-parameter --name "$privkey_ssm" --type SecureString --overwrite --value "$key_value" >/dev/null
    ;;
  *)
    echo "ERROR: unknown subcommand: $subcommand" >&2
    usage
    exit 2 ;;
esac
