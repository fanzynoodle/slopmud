#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  deploy_split_raft_trio_from_asset.sh /path/to/split.env /path/to/artifact.tgz
  deploy_split_raft_trio_from_asset.sh /path/to/split.env s3://BUCKET/KEY/artifact.tgz

Deploys a CI asset to the split Raft trio and then rewrites/restarts the public
telnet gateway so it uses SHARD_ADDRS. This is intentionally the dev/prod split
Raft path, not the legacy one-box shuttle path.
EOF
}

env_file="${1:-}"
artifact_ref="${2:-}"
if [[ -z "$env_file" || -z "$artifact_ref" ]]; then
  usage
  exit 2
fi
if [[ ! -f "$env_file" ]]; then
  echo "ERROR: env file not found: ${env_file}" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

artifact_was_s3=0
asset_dir="${tmp_dir}/asset"
if [[ -d "$artifact_ref" ]]; then
  asset_dir="$(cd "$artifact_ref" && pwd)"
else
  mkdir -p "$asset_dir"
  artifact_tgz="${tmp_dir}/artifact.tgz"
  if [[ "$artifact_ref" == s3://* ]]; then
    if ! command -v aws >/dev/null 2>&1; then
      echo "ERROR: aws CLI is required to fetch ${artifact_ref}" >&2
      exit 2
    fi
    echo "Downloading CI asset: ${artifact_ref}"
    aws s3 cp "$artifact_ref" "$artifact_tgz" --only-show-errors
    artifact_was_s3=1
  else
    if [[ ! -f "$artifact_ref" ]]; then
      echo "ERROR: artifact not found: ${artifact_ref}" >&2
      exit 2
    fi
    cp -f "$artifact_ref" "$artifact_tgz"
  fi
  tar -xzf "$artifact_tgz" -C "$asset_dir"
fi

gateway_host="${HOST:-${GATEWAY_HOST:-}}"
: "${gateway_host:?missing HOST or GATEWAY_HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
: "${SLOPMUD_BIND:?missing SLOPMUD_BIND in env file}"
: "${SHARD_ADDRS:?missing SHARD_ADDRS in env file; dev must point the gateway at the Raft trio}"
: "${SHARD_NODE_HOSTS:?missing SHARD_NODE_HOSTS in env file}"

ENV_NAME="${ENV_NAME:-dev}"
HOST="$gateway_host"
GATEWAY_HOST="${GATEWAY_HOST:-$gateway_host}"
SLOPMUD_APP_NAME="${SLOPMUD_APP_NAME:-slopmud-${ENV_NAME}}"
SLOPMUD_REMOTE_BIN="${SLOPMUD_REMOTE_BIN:-${REMOTE_ROOT}/bin/slopmud-${ENV_NAME}}"
NODE_ID="${NODE_ID:-gateway-${ENV_NAME}}"
SHARD_APP_NAME="${SHARD_APP_NAME:-shard-01-${ENV_NAME}}"
SHARD_REMOTE_BIN="${SHARD_REMOTE_BIN:-${REMOTE_ROOT}/bin/shard_01-${ENV_NAME}}"
export ENV_NAME HOST GATEWAY_HOST SLOPMUD_APP_NAME SLOPMUD_REMOTE_BIN NODE_ID SHARD_APP_NAME SHARD_REMOTE_BIN

ssh_connect_timeout_s="${SLOPMUD_SSH_CONNECT_TIMEOUT_S:-10}"
if ! [[ "$ssh_connect_timeout_s" =~ ^[0-9]+$ ]] || [[ "$ssh_connect_timeout_s" == "0" ]]; then
  echo "ERROR: SLOPMUD_SSH_CONNECT_TIMEOUT_S must be a positive integer" >&2
  exit 2
fi
hostkey_churn_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null)
ssh_liveness_opts=(-o BatchMode=yes -o "ConnectTimeout=${ssh_connect_timeout_s}" -o ServerAliveInterval=5 -o ServerAliveCountMax=2)

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  IFS=',' read -r -a out_ref <<<"$raw"
  local i
  for i in "${!out_ref[@]}"; do
    out_ref[$i]="$(printf '%s' "${out_ref[$i]}" | xargs)"
  done
}

split_csv "$SHARD_ADDRS" shard_addrs
if [[ "${#shard_addrs[@]}" != "3" ]]; then
  echo "ERROR: SHARD_ADDRS must contain exactly 3 comma-separated raft shard addresses" >&2
  exit 2
fi
split_csv "$SHARD_NODE_HOSTS" shard_hosts
if [[ "${#shard_hosts[@]}" != "3" ]]; then
  echo "ERROR: SHARD_NODE_HOSTS must contain exactly 3 comma-separated raft node hosts" >&2
  exit 2
fi
raft_ssh_user="${RAFT_SSH_USER:-$SSH_USER}"
raft_ssh_port="${RAFT_SSH_PORT:-22}"
raft_port="${SHARD_RAFT_PORT:-5100}"

gateway_bin_src="${asset_dir}/bin/slopmud"
shard_bin_src="${asset_dir}/bin/shard_01"
adminctl_src="${asset_dir}/bin/slopmud_adminctl"
walbackupd_src="${asset_dir}/bin/slopmud_walbackupd"
if [[ ! -x "$gateway_bin_src" ]]; then
  echo "ERROR: artifact is missing executable bin/slopmud" >&2
  exit 2
fi
if [[ ! -x "$shard_bin_src" ]]; then
  echo "ERROR: artifact is missing executable bin/shard_01" >&2
  exit 2
fi

release_id="${SLOPMUD_RELEASE_ID:-}"
if [[ -z "$release_id" && -f "${asset_dir}/BUILD_INFO.txt" ]]; then
  release_id="$(sed -n 's/^sha=//p' "${asset_dir}/BUILD_INFO.txt" | head -n 1)"
fi
if [[ -z "$release_id" ]]; then
  case "$artifact_ref" in
    s3://*) release_id="$(basename "$(dirname "${artifact_ref#s3://*/}")")" ;;
    *) release_id="$(basename "$(dirname "$artifact_ref")")" ;;
  esac
fi
if [[ -z "$release_id" || "$release_id" == "." || "$release_id" == "/" ]]; then
  release_id="$(date +%Y%m%d%H%M%S)"
fi
if ! [[ "$release_id" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "ERROR: release id contains unsupported characters: ${release_id}" >&2
  exit 2
fi
export SLOPMUD_RELEASE_ID="$release_id"

case "${SLOPMUD_WAL_RESTORE_ENABLED:-}" in
  1|true|TRUE|yes|YES|on|ON|auto)
    if [[ ! -x "$adminctl_src" ]]; then
      echo "ERROR: WAL restore is enabled but artifact is missing bin/slopmud_adminctl" >&2
      exit 2
    fi
    export SLOPMUD_ADMINCTL_BIN_SRC="$adminctl_src"
    ;;
esac

walbackupd_needed=0
case "${SLOPMUD_WAL_BACKUP_ENABLED:-}" in
  1|true|TRUE|yes|YES|on|ON) walbackupd_needed=1 ;;
esac
case "${SLOPMUD_EVENTLOG_UPLOAD_ENABLED:-}" in
  1|true|TRUE|yes|YES|on|ON) walbackupd_needed=1 ;;
esac
if [[ -n "${SLOPMUD_WAL_BACKUP_DIR:-}" || -n "${SLOPMUD_WAL_BACKUP_S3_BUCKET:-}" ]]; then
  walbackupd_needed=1
fi
if [[ "$walbackupd_needed" == "1" ]]; then
  if [[ ! -x "$walbackupd_src" ]]; then
    echo "ERROR: WAL/eventlog backup is enabled but artifact is missing bin/slopmud_walbackupd" >&2
    exit 2
  fi
  export SLOPMUD_WALBACKUPD_BIN_SRC="$walbackupd_src"
fi

export SLOPMUD_SKIP_BUILD=1
export SLOPMUD_BIN_SRC="$shard_bin_src"
export SLOPMUD_FAST_ROLLING_RESTART="${SLOPMUD_FAST_ROLLING_RESTART:-1}"
export SLOPMUD_ROLLING_RESTART_BUDGET_MS="${SLOPMUD_ROLLING_RESTART_BUDGET_MS:-5000}"
export SLOPMUD_STRICT_LIVE_UPGRADE="${SLOPMUD_STRICT_LIVE_UPGRADE:-1}"
export SLOPMUD_QUORUM_RESTART_GUARD="${SLOPMUD_QUORUM_RESTART_GUARD:-1}"
export SLOPMUD_RAFT_RESTART_LEASE="${SLOPMUD_RAFT_RESTART_LEASE:-required}"
export SLOPMUD_ROLLING_TRANSFER_LEADER="${SLOPMUD_ROLLING_TRANSFER_LEADER:-1}"
export SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART="${SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART:-0}"
export SLOPMUD_ATOMIC_BIN_SWAP="${SLOPMUD_ATOMIC_BIN_SWAP:-1}"
if [[ "$artifact_was_s3" == "1" && -z "${SLOPMUD_DEPLOY_FROM_S3:-}" ]]; then
  export SLOPMUD_DEPLOY_FROM_S3=1
fi

echo "Deploying split Raft trio from artifact (env=${ENV_NAME}, release=${release_id})"
./scripts/deploy_split_raft_trio.sh "$env_file"

gateway_ssh_opts=("${hostkey_churn_opts[@]}" "${ssh_liveness_opts[@]}" -p "$SSH_PORT")
gateway_scp_opts=("${hostkey_churn_opts[@]}" "${ssh_liveness_opts[@]}" -P "$SSH_PORT")
remote_bin_dir="$(dirname "$SLOPMUD_REMOTE_BIN")"
release_dir="${SLOPMUD_VERSIONED_BIN_DIR:-${remote_bin_dir}/releases}"
remote_release_bin="${release_dir}/slopmud-${release_id}"
unit_name="${SLOPMUD_APP_NAME}.service"

tmp_unit="${tmp_dir}/${unit_name}"
cat >"$tmp_unit" <<EOF
[Unit]
Description=slopmud gateway (env: ${ENV_NAME}, sha: ${release_id})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=slopmud=info
Environment=NODE_ID=${NODE_ID}
Environment=SLOPMUD_BIND=${SLOPMUD_BIND}
Environment=SHARD_ADDR=${shard_addrs[0]}
Environment=SHARD_ADDRS=${SHARD_ADDRS}
EOF

append_env_if_set() {
  local name="$1"
  if [[ -n "${!name:-}" ]]; then
    echo "Environment=${name}=${!name}" >>"$tmp_unit"
  fi
}

for var in \
  SLOPMUD_OIDC_TOKEN_URL \
  SLOPMUD_OIDC_CLIENT_ID \
  SLOPMUD_OIDC_CLIENT_SECRET \
  SLOPMUD_OIDC_SCOPE \
  SLOPMUD_OIDC_TOKEN_URL_SSM \
  SLOPMUD_OIDC_CLIENT_ID_SSM \
  SLOPMUD_OIDC_CLIENT_SECRET_SSM \
  SLOPMUD_OIDC_SCOPE_SSM \
  SLOPMUD_GOOGLE_OAUTH_DIR \
  SLOPMUD_GOOGLE_AUTH_BASE_URL \
  SLOPMUD_ACCOUNTS_PATH \
  SLOPMUD_PLAYERS_PATH \
  SLOPMUD_LOCALE \
  SLOPMUD_ADMIN_BIND \
  SLOPMUD_BANS_PATH \
  SLOPMUD_SBC_ENABLED \
  SLOPMUD_NEARLINE_DIR \
  SLOPMUD_NEARLINE_MAX_SEGMENTS \
  SLOPMUD_NEARLINE_SEGMENT_MAX_BYTES \
  SLOPMUD_BLOB_SPOOL_DIR \
  SLOPMUD_EVENTLOG_ENABLED \
  SLOPMUD_EVENTLOG_SPOOL_DIR \
  SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S \
  SLOPMUD_EVENTLOG_S3_BUCKET \
  SLOPMUD_EVENTLOG_S3_PREFIX \
  SLOPMUD_EVENTLOG_UPLOAD_ENABLED \
  SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL \
  SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S \
  SLOPMUD_WEBAUTH_JWT_SECRET
do
  append_env_if_set "$var"
done

exec_start="${SLOPMUD_REMOTE_BIN}"
if [[ -n "${SLOPMUD_OIDC_TOKEN_URL_SSM:-}" || -n "${SLOPMUD_OIDC_CLIENT_ID_SSM:-}" || -n "${SLOPMUD_OIDC_CLIENT_SECRET_SSM:-}" || -n "${SLOPMUD_OIDC_SCOPE_SSM:-}" ]]; then
  exec_start="/bin/bash -ceu ' \
    if [[ -n \"\${SLOPMUD_OIDC_TOKEN_URL_SSM}\" ]]; then \
      export SLOPMUD_OIDC_TOKEN_URL=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_OIDC_TOKEN_URL_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${SLOPMUD_OIDC_CLIENT_ID_SSM}\" ]]; then \
      export SLOPMUD_OIDC_CLIENT_ID=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_OIDC_CLIENT_ID_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${SLOPMUD_OIDC_CLIENT_SECRET_SSM}\" ]]; then \
      export SLOPMUD_OIDC_CLIENT_SECRET=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_OIDC_CLIENT_SECRET_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    if [[ -n \"\${SLOPMUD_OIDC_SCOPE_SSM}\" ]]; then \
      export SLOPMUD_OIDC_SCOPE=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_OIDC_SCOPE_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
    fi; \
    exec \"${SLOPMUD_REMOTE_BIN}\"; \
  '"
fi

cat >>"$tmp_unit" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

echo "Provisioning gateway (${gateway_host})"
ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
  set -euo pipefail; \
  if ! id -u slopmud >/dev/null 2>&1; then sudo useradd --system --home '${REMOTE_ROOT}' --create-home --shell /usr/sbin/nologin slopmud; fi; \
  sudo mkdir -p '${REMOTE_ROOT}' '${remote_bin_dir}' '${release_dir}' '${REMOTE_ROOT}/var'; \
  sudo chown slopmud:slopmud '${REMOTE_ROOT}' '${REMOTE_ROOT}/var' \
"

if [[ "$walbackupd_needed" == "1" ]]; then
  echo "Uploading slopmud_walbackupd -> gateway"
  scp "${gateway_scp_opts[@]}" "$walbackupd_src" "${SSH_USER}@${gateway_host}:/tmp/slopmud_walbackupd.${release_id}"
  ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
    set -euo pipefail; \
    sudo install -m 0755 -o root -g root '/tmp/slopmud_walbackupd.${release_id}' '${remote_bin_dir}/slopmud_walbackupd'; \
    sudo rm -f '/tmp/slopmud_walbackupd.${release_id}' \
  "
fi

echo "Uploading slopmud gateway -> ${gateway_host}:${remote_release_bin}"
scp "${gateway_scp_opts[@]}" "$gateway_bin_src" "${SSH_USER}@${gateway_host}:/tmp/slopmud.${release_id}"
ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
  set -euo pipefail; \
  sudo install -m 0755 -o root -g root '/tmp/slopmud.${release_id}' '${remote_release_bin}'; \
  sudo ln -sfn '${remote_release_bin}' '${SLOPMUD_REMOTE_BIN}.next'; \
  sudo mv -Tf '${SLOPMUD_REMOTE_BIN}.next' '${SLOPMUD_REMOTE_BIN}'; \
  sudo rm -f '/tmp/slopmud.${release_id}' \
"

legacy_shard_unit="${SHARD_APP_NAME}.service"
echo "Disabling legacy gateway-local shard unit (${legacy_shard_unit})"
ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
  set -euo pipefail; \
  if systemctl list-unit-files '${legacy_shard_unit}' >/dev/null 2>&1 || systemctl list-units --all '${legacy_shard_unit}' >/dev/null 2>&1; then \
    sudo systemctl disable --now '${legacy_shard_unit}' >/dev/null 2>&1 || true; \
  fi \
"

echo "Installing gateway systemd unit (${unit_name})"
scp "${gateway_scp_opts[@]}" "$tmp_unit" "${SSH_USER}@${gateway_host}:/tmp/${unit_name}"
ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
  set -euo pipefail; \
  sudo mv '/tmp/${unit_name}' '/etc/systemd/system/${unit_name}'; \
  sudo systemctl daemon-reload; \
  sudo systemctl enable '${unit_name}'; \
  sudo systemctl restart '${unit_name}'; \
  sudo systemctl is-active --quiet '${unit_name}'; \
  expected=\$(printf %s '$(printf '%s' "$SHARD_ADDRS" | base64 | tr -d '\n')' | base64 -d); \
  sudo systemctl show -p Environment --value '${unit_name}' | tr ' ' '\n' | grep -Fx \"SHARD_ADDRS=\$expected\" >/dev/null \
"

port="${SLOPMUD_BIND##*:}"
echo "Gateway listening check (port ${port})"
ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
  set -euo pipefail; \
  sudo ss -lntp | grep -n ':${port}\\b' >/dev/null || { echo 'not listening'; exit 1; } \
"

case "${SLOPMUD_STATUS_UPDATE_ENABLED:-1}" in
  1|true|TRUE|yes|YES|on|ON)
    status_unit="${tmp_dir}/slopmud-statusd.service"
    status_node_ids_csv="${SHARD_RAFT_NODE_IDS:-${SHARD_NODE_IDS:-n0,n1,n2}}"
    split_csv "$status_node_ids_csv" status_node_ids
    if [[ "${#status_node_ids[@]}" != "3" ]]; then
      echo "ERROR: SHARD_RAFT_NODE_IDS/SHARD_NODE_IDS must contain exactly 3 values for statusd" >&2
      exit 2
    fi
    status_hosts="status.slopmud.com,localhost,127.0.0.1,${gateway_host}"
    status_checks="dev/broker/gateway=tcp://127.0.0.1:${port};build=${release_id};detail=split Raft gateway broker"
    status_checks+=",dev/raft-voter/${status_node_ids[0]}=tcp://${shard_hosts[0]}:${raft_port};build=${release_id};detail=Raft RPC voter"
    status_checks+=",dev/raft-voter/${status_node_ids[1]}=tcp://${shard_hosts[1]}:${raft_port};build=${release_id};detail=Raft RPC voter"
    status_checks+=",dev/raft-voter/${status_node_ids[2]}=tcp://${shard_hosts[2]}:${raft_port};build=${release_id};detail=Raft RPC voter"
    status_checks+="${SLOPMUD_STATUS_EXTRA_CHECKS:-,stg/broker/gateway=tcp://127.0.0.1:4023;build=not deployed;detail=expected stg broker port,prd/broker/gateway=tcp://127.0.0.1:4200;build=unknown;detail=systemd slopmud-prd prd-split-az1,prd/websocket/gateway=tcp://127.0.0.1:4242;build=unknown;detail=slopmud-web-prd-oauth,sandbox/broker/gateway=tcp://127.0.0.1:4500;build=unknown;detail=systemd slopmud-sandbox,ops/metrics/gateway=tcp://127.0.0.1:9912;build=current;detail=sbc-metricsd,public/website/slopmud.com=tcp://slopmud.com:443;build=current;detail=slopmud-landing,public/website/www=tcp://www.slopmud.com:443;build=current;detail=slopmud-landing,public/website/prd-gaia=tcp://prd-gaia.slopmud.com:443;build=current;detail=slopmud-landing,public/portal/mud-443=tcp://mud.slopmud.com:443;build=current;detail=slopmud-landing,public/portal/mud-4242=tcp://mud.slopmud.com:4242;build=current;detail=slopmud-web-prd-oauth}"
    cat >"$status_unit" <<EOF
[Unit]
Description=Slopmud public status dashboard
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
NoNewPrivileges=true
Environment=SLOPMUD_STATUS_BIND=0.0.0.0:80
Environment="SLOPMUD_STATUS_TITLE=Slopmud Status"
Environment=SLOPMUD_STATUS_ENVS=dev,stg,prd,sandbox,public
Environment=SLOPMUD_STATUS_HOSTS=${status_hosts}
Environment="SLOPMUD_STATUS_CHECKS=${status_checks}"
ExecStart=/usr/local/bin/slopmud_statusd
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
EOF
    echo "Updating status dashboard checks"
    scp "${gateway_scp_opts[@]}" "$status_unit" "${SSH_USER}@${gateway_host}:/tmp/slopmud-statusd.service"
    ssh "${gateway_ssh_opts[@]}" "${SSH_USER}@${gateway_host}" "\
      set -euo pipefail; \
      if [ -x /usr/local/bin/slopmud_statusd ]; then \
        sudo mv /tmp/slopmud-statusd.service /etc/systemd/system/slopmud-statusd.service; \
        sudo systemctl daemon-reload; \
        sudo systemctl enable slopmud-statusd.service >/dev/null; \
        sudo systemctl restart slopmud-statusd.service; \
        sudo systemctl is-active --quiet slopmud-statusd.service; \
      else \
        rm -f /tmp/slopmud-statusd.service; \
        echo 'status dashboard binary missing; skipping statusd update'; \
      fi \
    "
    ;;
esac

echo "Verifying Raft voter status on all three nodes"
raft_proxy_command="ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=${ssh_connect_timeout_s} -p ${SSH_PORT} -W %h:%p ${SSH_USER}@${gateway_host}"
raft_ssh_opts=("${hostkey_churn_opts[@]}" "${ssh_liveness_opts[@]}" -o "ProxyCommand=${raft_proxy_command}" -p "$raft_ssh_port")
status_payload_b64="$(printf '%s' '{"t":"StatusReq"}' | base64 | tr -d '\n')"
for host in "${shard_hosts[@]}"; do
  ssh "${raft_ssh_opts[@]}" "${raft_ssh_user}@${host}" "\
    set -euo pipefail; \
    payload=\$(printf %s '${status_payload_b64}' | base64 -d); \
    exec 3<>/dev/tcp/127.0.0.1/${raft_port}; \
    printf '%s\n' \"\$payload\" >&3; \
    IFS= read -r -t 5 line <&3; \
    printf '%s\n' \"\$line\"; \
    printf '%s\n' \"\$line\" | grep -Eq '\"t\"[[:space:]]*:[[:space:]]*\"StatusResp\"' \
  " >/dev/null
done

echo "OK: split Raft trio and gateway deployed (env=${ENV_NAME}, release=${release_id})"
