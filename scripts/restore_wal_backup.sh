#!/usr/bin/env bash
set -euo pipefail

bool_true() {
  case "${1:-}" in
    1|true|TRUE|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

bool_false() {
  case "${1:-}" in
    ""|0|false|FALSE|no|NO|off|OFF) return 0 ;;
    *) return 1 ;;
  esac
}

enabled="${SLOPMUD_WAL_RESTORE_ENABLED:-}"
if bool_false "$enabled"; then
  echo "wal restore: disabled"
  exit 0
fi

adminctl="${SLOPMUD_ADMINCTL_BIN:-${SLOPMUD_WAL_RESTORE_ADMINCTL:-slopmud_adminctl}}"
target="${SLOPMUD_WAL_RESTORE_OUT:-${SHARD_RAFT_LOG:-}}"
node_id="${SLOPMUD_WAL_RESTORE_NODE_ID:-${SHARD_RAFT_NODE_ID:-${NODE_ID:-}}}"
force="${SLOPMUD_WAL_RESTORE_OVERWRITE:-${SLOPMUD_WAL_RESTORE_FORCE:-0}}"

source_kind=""
source_value=""
restore_s3_uri=""
if [[ -n "${SLOPMUD_WAL_RESTORE_S3_URI:-}" ]]; then
  restore_s3_uri="${SLOPMUD_WAL_RESTORE_S3_URI}"
elif [[ -n "${SLOPMUD_WAL_RESTORE_S3_BUCKET:-}" ]]; then
  restore_s3_uri="s3://${SLOPMUD_WAL_RESTORE_S3_BUCKET}/${SLOPMUD_WAL_RESTORE_S3_PREFIX:-slopmud/wal-backups}"
fi
if [[ -n "${restore_s3_uri}" && -n "${SLOPMUD_WAL_RESTORE_DIR:-}" ]]; then
  echo "ERROR: use only one of SLOPMUD_WAL_RESTORE_DIR or SLOPMUD_WAL_RESTORE_S3_*" >&2
  exit 2
fi

if [[ -n "${restore_s3_uri}" ]]; then
  source_kind="s3"
  source_value="${restore_s3_uri}"
elif [[ -n "${SLOPMUD_WAL_RESTORE_DIR:-}" ]]; then
  source_kind="dir"
  source_value="${SLOPMUD_WAL_RESTORE_DIR}"
elif [[ -n "${SLOPMUD_WAL_BACKUP_S3_BUCKET:-}" ]]; then
  source_kind="s3"
  source_value="s3://${SLOPMUD_WAL_BACKUP_S3_BUCKET}/${SLOPMUD_WAL_BACKUP_S3_PREFIX:-slopmud/wal-backups}"
elif [[ -n "${SLOPMUD_WAL_BACKUP_DIR:-}" ]]; then
  source_kind="dir"
  source_value="${SLOPMUD_WAL_BACKUP_DIR}"
fi

if [[ -z "$target" || -z "$source_kind" ]]; then
  if [[ "$enabled" == "auto" ]]; then
    echo "wal restore: no target/source config; skipping"
    exit 0
  fi
  echo "ERROR: wal restore requires SHARD_RAFT_LOG or SLOPMUD_WAL_RESTORE_OUT and a restore source" >&2
  exit 2
fi

if [[ -s "$target" ]] && ! bool_true "$force"; then
  echo "wal restore: ${target} is non-empty; skipping"
  exit 0
fi

if ! command -v "$adminctl" >/dev/null 2>&1; then
  echo "ERROR: wal restore adminctl not found: ${adminctl}" >&2
  exit 2
fi

mkdir -p "$(dirname "$target")"

cmd=("$adminctl" "wal-backup" "recover" "--out" "$target")
case "$source_kind" in
  s3)
    cache_dir="${SLOPMUD_WAL_RESTORE_CACHE_DIR:-${SLOPMUD_WAL_BACKUP_CACHE_DIR:-${SLOPMUD_WAL_BACKUP_DIR:-${target}.walrestore-cache}}}"
    mkdir -p "$cache_dir"
    cmd+=("--s3" "$source_value" "--cache-dir" "$cache_dir")
    ;;
  dir)
    cmd+=("--dir" "$source_value")
    ;;
  *)
    echo "ERROR: unsupported wal restore source kind: ${source_kind}" >&2
    exit 2
    ;;
esac

if [[ -n "$node_id" ]]; then
  cmd+=("--node-id" "$node_id")
fi
if [[ -n "${SLOPMUD_WAL_RESTORE_MANIFEST_UNIX_AT_OR_BEFORE:-}" ]]; then
  cmd+=("--manifest-unix-at-or-before" "${SLOPMUD_WAL_RESTORE_MANIFEST_UNIX_AT_OR_BEFORE}")
fi
if [[ "$enabled" == "auto" ]] || bool_true "${SLOPMUD_WAL_RESTORE_MISSING_OK:-0}"; then
  cmd+=("--missing-ok")
fi

target_count=0
if [[ -n "${SLOPMUD_WAL_RESTORE_UNTIL_OFFSET:-}" ]]; then
  cmd+=("--until-offset" "${SLOPMUD_WAL_RESTORE_UNTIL_OFFSET}")
  target_count=$((target_count + 1))
fi
if [[ -n "${SLOPMUD_WAL_RESTORE_UNTIL_INDEX:-}" ]]; then
  cmd+=("--until-index" "${SLOPMUD_WAL_RESTORE_UNTIL_INDEX}")
  target_count=$((target_count + 1))
fi
if [[ -n "${SLOPMUD_WAL_RESTORE_UNTIL_MS:-}" ]]; then
  cmd+=("--until-ms" "${SLOPMUD_WAL_RESTORE_UNTIL_MS}")
  target_count=$((target_count + 1))
fi
if (( target_count > 1 )); then
  echo "ERROR: use at most one SLOPMUD_WAL_RESTORE_UNTIL_* target" >&2
  exit 2
fi

echo "wal restore: restoring ${target} from ${source_kind}:${source_value}"
set +e
restore_output="$("${cmd[@]}" 2>&1)"
restore_rc=$?
set -e
if [[ -n "$restore_output" ]]; then
  printf '%s\n' "$restore_output"
fi
if (( restore_rc != 0 )); then
  if { [[ "$enabled" == "auto" ]] || bool_true "${SLOPMUD_WAL_RESTORE_MISSING_OK:-0}"; } \
    && [[ "$restore_output" == *"no matching"*manifest* ]]; then
    echo "wal restore: no matching manifest; skipping"
    exit 0
  fi
  exit "$restore_rc"
fi
rm -f "${target}.state.json"
