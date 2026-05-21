#!/usr/bin/env bash
set -euo pipefail

env_file="${1:-env/prd.env}"
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
: "${SHARD_APP_NAME:?missing SHARD_APP_NAME in env file}"
: "${SHARD_REMOTE_BIN:?missing SHARD_REMOTE_BIN in env file}"
: "${SHARD_BIND:?missing SHARD_BIND in env file}"

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  IFS=',' read -r -a out_ref <<<"$raw"
  local i
  for i in "${!out_ref[@]}"; do
    out_ref[$i]="$(printf '%s' "${out_ref[$i]}" | xargs)"
  done
}

require_three() {
  local label="$1"
  local -n arr_ref="$2"
  if [[ "${#arr_ref[@]}" != "3" ]]; then
    echo "ERROR: ${label} must contain exactly 3 comma-separated values" >&2
    exit 2
  fi
}

suffix_log_path() {
  local path="$1"
  local suffix="$2"
  if [[ "$path" == *.jsonl ]]; then
    printf '%s_%s.jsonl' "${path%.jsonl}" "$suffix"
  else
    printf '%s_%s' "$path" "$suffix"
  fi
}

shard_host="${SHARD_BIND%:*}"
shard_port="${SHARD_BIND##*:}"
if ! [[ "$shard_port" =~ ^[0-9]+$ ]]; then
  echo "ERROR: SHARD_BIND port is not numeric: ${SHARD_BIND}" >&2
  exit 2
fi

default_shard_binds="${SHARD_BIND},${shard_host}:$((shard_port + 1)),${shard_host}:$((shard_port + 2))"
raft_host="${SHARD_RAFT_HOST:-127.0.0.1}"
raft_base_port="${SHARD_RAFT_BASE_PORT:-$((shard_port + 100))}"
default_raft_binds="${raft_host}:${raft_base_port},${raft_host}:$((raft_base_port + 1)),${raft_host}:$((raft_base_port + 2))"
default_unit_names="${SHARD_APP_NAME},${SHARD_APP_NAME}-n1,${SHARD_APP_NAME}-n2"
default_node_ids="${SHARD_RAFT_NODE_IDS:-n0,n1,n2}"
base_log="${SHARD_RAFT_LOG:-${REMOTE_ROOT}/var/shard_01_raft.jsonl}"
default_logs="${base_log},$(suffix_log_path "$base_log" n1),$(suffix_log_path "$base_log" n2)"

split_csv "${SHARD_TRIO_BINDS:-$default_shard_binds}" shard_binds
split_csv "${SHARD_TRIO_RAFT_BINDS:-$default_raft_binds}" raft_binds
split_csv "${SHARD_TRIO_APP_NAMES:-$default_unit_names}" unit_names
split_csv "$default_node_ids" node_ids
split_csv "${SHARD_TRIO_RAFT_LOGS:-$default_logs}" raft_logs

require_three "SHARD_TRIO_BINDS" shard_binds
require_three "SHARD_TRIO_RAFT_BINDS" raft_binds
require_three "SHARD_TRIO_APP_NAMES" unit_names
require_three "SHARD_RAFT_NODE_IDS" node_ids
require_three "SHARD_TRIO_RAFT_LOGS" raft_logs

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

remote_bin_dir="$(dirname "$SHARD_REMOTE_BIN")"
adminctl_src="${SLOPMUD_ADMINCTL_BIN_SRC:-target/release/slopmud_adminctl}"
remote_adminctl_bin="${remote_bin_dir}/slopmud_adminctl"
wal_restore_helper_src="scripts/restore_wal_backup.sh"
wal_restore_enabled=0
case "${SLOPMUD_WAL_RESTORE_ENABLED:-}" in
  1|true|TRUE|yes|YES|on|ON|auto) wal_restore_enabled=1 ;;
esac

echo "Building shard_01 (release)"
./scripts/build_bookworm_release.sh shard_01
if [[ "$wal_restore_enabled" == "1" ]]; then
  echo "Building slopmud_adminctl (release)"
  ./scripts/build_bookworm_release.sh slopmud_adminctl
fi

bin_src="target/release/shard_01"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at $bin_src" >&2
  exit 2
fi
if [[ "$wal_restore_enabled" == "1" && ! -x "$adminctl_src" ]]; then
  echo "ERROR: expected adminctl binary at $adminctl_src" >&2
  exit 2
fi
if [[ "$wal_restore_enabled" == "1" && ! -x "$wal_restore_helper_src" ]]; then
  echo "ERROR: expected restore helper at $wal_restore_helper_src" >&2
  exit 2
fi

echo "Provisioning remote directories + system user"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates awscli; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install ca-certificates awscli; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  if ! id -u slopmud >/dev/null 2>&1; then \
    sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
  fi; \
  sudo mkdir -p \"${REMOTE_ROOT}\" \"${remote_bin_dir}\" \"${REMOTE_ROOT}/var\"; \
  sudo chown -R slopmud:slopmud \"${REMOTE_ROOT}\" \
"

echo "Uploading binary -> ${SSH_USER}@${HOST}:${SHARD_REMOTE_BIN}"
scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$bin_src" "${SSH_USER}@${HOST}:/tmp/shard_01"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root /tmp/shard_01 \"${SHARD_REMOTE_BIN}\"; \
  sudo rm -f /tmp/shard_01 \
"

if [[ "$wal_restore_enabled" == "1" ]]; then
  echo "Uploading wal restore helper + adminctl"
  scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$adminctl_src" "${SSH_USER}@${HOST}:/tmp/slopmud_adminctl"
  scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$wal_restore_helper_src" "${SSH_USER}@${HOST}:/tmp/slopmud-wal-restore"
  ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
    set -euo pipefail; \
    sudo install -m 0755 -o root -g root /tmp/slopmud_adminctl \"${remote_adminctl_bin}\"; \
    sudo install -m 0755 -o root -g root /tmp/slopmud-wal-restore /usr/local/bin/slopmud-wal-restore; \
    sudo rm -f /tmp/slopmud_adminctl /tmp/slopmud-wal-restore \
  "
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

unit_args=()
for i in 0 1 2; do
  peers=()
  for j in 0 1 2; do
    if [[ "$i" == "$j" ]]; then
      continue
    fi
    peers+=("${node_ids[$j]}@${raft_binds[$j]}")
  done
  peers_csv="$(IFS=','; echo "${peers[*]}")"

  tmp_unit="${tmp_dir}/${unit_names[$i]}.service"
  cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud shard_01 trio node ${node_ids[$i]} (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=shard_01=info
Environment=NODE_ID=${NODE_ID:-${node_ids[$i]}}
Environment=SHARD_BIND=${shard_binds[$i]}
Environment=SHARD_RAFT_NODE_ID=${node_ids[$i]}
Environment=SHARD_RAFT_BIND=${raft_binds[$i]}
Environment=SHARD_RAFT_PEERS=${peers_csv}
Environment=SHARD_RAFT_LOG=${raft_logs[$i]}
EOF

  if [[ -n "${SHARD_RAFT_ELECTION_MS:-}" ]]; then
    echo "Environment=SHARD_RAFT_ELECTION_MS=${SHARD_RAFT_ELECTION_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_RAFT_HEARTBEAT_MS:-}" ]]; then
    echo "Environment=SHARD_RAFT_HEARTBEAT_MS=${SHARD_RAFT_HEARTBEAT_MS}" >>"$tmp_unit"
  fi
  for var in \
    SLOPMUD_WAL_BACKUP_ENABLED \
    SLOPMUD_WAL_BACKUP_DIR \
    SLOPMUD_WAL_BACKUP_INTERVAL_S \
    SLOPMUD_WAL_BACKUP_MAX_SEGMENT_BYTES \
    SLOPMUD_WAL_BACKUP_MAX_LOCAL_MANIFESTS \
    SLOPMUD_WAL_BACKUP_S3_BUCKET \
    SLOPMUD_WAL_BACKUP_S3_PREFIX \
    SLOPMUD_WAL_BACKUP_UPLOAD_ENABLED \
    SLOPMUD_WAL_RESTORE_ENABLED \
    SLOPMUD_WAL_RESTORE_DIR \
    SLOPMUD_WAL_RESTORE_S3_URI \
    SLOPMUD_WAL_RESTORE_S3_BUCKET \
    SLOPMUD_WAL_RESTORE_S3_PREFIX \
    SLOPMUD_WAL_RESTORE_CACHE_DIR \
    SLOPMUD_WAL_RESTORE_NODE_ID \
    SLOPMUD_WAL_RESTORE_OVERWRITE \
    SLOPMUD_WAL_RESTORE_MISSING_OK \
    SLOPMUD_WAL_RESTORE_MANIFEST_UNIX_AT_OR_BEFORE \
    SLOPMUD_WAL_RESTORE_UNTIL_OFFSET \
    SLOPMUD_WAL_RESTORE_UNTIL_INDEX \
    SLOPMUD_WAL_RESTORE_UNTIL_MS
  do
    if [[ -n "${!var:-}" ]]; then
      echo "Environment=${var}=${!var}" >>"$tmp_unit"
    fi
  done
  if [[ "$wal_restore_enabled" == "1" ]]; then
    echo "Environment=SLOPMUD_ADMINCTL_BIN=${remote_adminctl_bin}" >>"$tmp_unit"
  fi
  if [[ -n "${WORLD_TICK_MS:-}" ]]; then
    echo "Environment=WORLD_TICK_MS=${WORLD_TICK_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${WORLD_TIME_SCALE_PPM:-}" ]]; then
    echo "Environment=WORLD_TIME_SCALE_PPM=${WORLD_TIME_SCALE_PPM}" >>"$tmp_unit"
  fi
  if [[ -n "${BARTENDER_EMOTE_MS:-}" ]]; then
    echo "Environment=BARTENDER_EMOTE_MS=${BARTENDER_EMOTE_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${MOB_WANDER_MS:-}" ]]; then
    echo "Environment=MOB_WANDER_MS=${MOB_WANDER_MS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_BOOTSTRAP_ADMINS:-}" ]]; then
    echo "Environment=SHARD_BOOTSTRAP_ADMINS=${SHARD_BOOTSTRAP_ADMINS}" >>"$tmp_unit"
  fi
  if [[ -n "${SHARD_BOOTSTRAP_ADMIN_SSO:-}" ]]; then
    echo "Environment=SHARD_BOOTSTRAP_ADMIN_SSO=${SHARD_BOOTSTRAP_ADMIN_SSO}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_API_BASE:-}" ]]; then
    echo "Environment=OPENAI_API_BASE=${OPENAI_API_BASE}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_PING_MODEL:-}" ]]; then
    echo "Environment=OPENAI_PING_MODEL=${OPENAI_PING_MODEL}" >>"$tmp_unit"
  fi
  if [[ -n "${OPENAI_API_KEY_SSM:-}" ]]; then
    echo "Environment=OPENAI_API_KEY_SSM=${OPENAI_API_KEY_SSM}" >>"$tmp_unit"
  fi

  exec_start="${SHARD_REMOTE_BIN}"
  if [[ -n "${OPENAI_API_KEY_SSM:-}" ]]; then
    exec_start="/bin/bash -ceu 'export OPENAI_API_KEY=\"\$(aws ssm get-parameter --region us-east-1 --name \"\${OPENAI_API_KEY_SSM}\" --with-decryption --query Parameter.Value --output text)\"; exec \"${SHARD_REMOTE_BIN}\";'"
  fi

  cat >>"$tmp_unit" <<EOF
$(if [[ "$wal_restore_enabled" == "1" ]]; then echo "ExecStartPre=/usr/local/bin/slopmud-wal-restore"; fi)
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

  unit_args+=("${unit_names[$i]}.service")
  echo "Uploading systemd unit (${unit_names[$i]}.service)"
  scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp_unit" "${SSH_USER}@${HOST}:/tmp/${unit_names[$i]}.service"
done

unit_list="$(printf ' %q' "${unit_args[@]}")"
ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  for unit in${unit_list}; do \
    sudo mv \"/tmp/\${unit}\" \"/etc/systemd/system/\${unit}\"; \
  done; \
  sudo systemctl daemon-reload; \
  for unit in${unit_list}; do sudo systemctl enable --now \"\${unit}\"; done; \
  sudo systemctl restart${unit_list}; \
  sudo systemctl --no-pager --full status${unit_list} || true \
"

echo "Waiting for shard + raft ports"
for addr in "${shard_binds[@]}" "${raft_binds[@]}"; do
  port="${addr##*:}"
  ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
    set -euo pipefail; \
    for _ in {1..80}; do \
      if sudo ss -lntp | grep -q \":${port}\\\\b\"; then exit 0; fi; \
      sleep 0.25; \
    done; \
    sudo ss -lntp | grep -n \":${port}\\\\b\" || true; \
    echo 'not listening on ${addr}'; \
    exit 1 \
  "
done

echo "OK: shard trio deployed (${shard_binds[*]} / ${raft_binds[*]})"
