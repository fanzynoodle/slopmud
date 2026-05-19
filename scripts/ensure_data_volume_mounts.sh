#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  ensure_data_volume_mounts.sh /path/to/env.env [gateway|raft|all]

Formats and mounts the attached non-root EBS data volume at
SLOPMUD_STATE_DIR or /opt/slopmud/state. Raft nodes are reached through
ProxyJump via GATEWAY_HOST/HOST.
EOF
}

env_file="${1:-}"
mode="${2:-all}"
if [[ -z "$env_file" ]]; then
  usage
  exit 2
fi
if [[ ! -f "$env_file" ]]; then
  echo "ERROR: env file not found: $env_file" >&2
  exit 2
fi
case "$mode" in
  gateway | raft | all) ;;
  *)
    usage
    exit 2
    ;;
esac

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

gateway_host="${GATEWAY_HOST:-${HOST:-}}"
: "${gateway_host:?missing GATEWAY_HOST or HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"

state_dir="${SLOPMUD_STATE_DIR:-${REMOTE_ROOT}/state}"

split_csv() {
  local raw="$1"
  local -n out_ref="$2"
  IFS=',' read -r -a out_ref <<<"$raw"
  local i
  for i in "${!out_ref[@]}"; do
    out_ref[$i]="$(printf '%s' "${out_ref[$i]}" | xargs)"
  done
}

remote_script='
set -euo pipefail
export PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:${PATH:-}"

mountpoint="${SLOPMUD_STATE_DIR:?missing SLOPMUD_STATE_DIR}"
remote_root="${REMOTE_ROOT:?missing REMOTE_ROOT}"

if command -v findmnt >/dev/null 2>&1 && command -v mkfs.ext4 >/dev/null 2>&1 && command -v blkid >/dev/null 2>&1; then
  true
elif command -v apt-get >/dev/null 2>&1; then
  sudo DEBIAN_FRONTEND=noninteractive apt-get update -y >/dev/null
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y util-linux e2fsprogs >/dev/null
elif command -v dnf >/dev/null 2>&1; then
  sudo dnf -y install util-linux e2fsprogs >/dev/null
fi

if ! id -u slopmud >/dev/null 2>&1; then
  sudo useradd --system --home "${remote_root}" --create-home --shell /usr/sbin/nologin slopmud
fi

if findmnt -rn --mountpoint "${mountpoint}" >/dev/null 2>&1; then
  sudo chown -R slopmud:slopmud "${mountpoint}"
  echo "already mounted: ${mountpoint}"
  exit 0
fi

root_src="$(findmnt -n -o SOURCE /)"
root_disk="$(lsblk -no PKNAME "${root_src}" 2>/dev/null | head -n1 || true)"
if [ -z "${root_disk}" ]; then
  root_disk="$(basename "$(readlink -f "${root_src}")")"
fi

pick_device() {
  local deadline=$((SECONDS + 60))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    for dev in /dev/disk/by-id/nvme-Amazon_Elastic_Block_Store_vol* /dev/nvme*n1 /dev/xvd[f-z] /dev/sd[f-z]; do
      [ -e "${dev}" ] || continue
      real="$(readlink -f "${dev}")"
      [ -b "${real}" ] || continue
      [ "$(lsblk -dnro TYPE "${real}" 2>/dev/null | head -n1)" = "disk" ] || continue
      name="$(basename "${real}")"
      [ "${name}" != "${root_disk}" ] || continue
      if lsblk -nrpo MOUNTPOINT "${real}" | grep -Eq "^/$|^/boot"; then
        continue
      fi
      echo "${real}"
      return 0
    done
    sleep 1
  done
  return 1
}

device="$(pick_device)" || {
  echo "ERROR: no non-root data volume found" >&2
  lsblk -o NAME,TYPE,SIZE,MOUNTPOINT,FSTYPE >&2 || true
  exit 1
}

if ! sudo blkid "${device}" >/dev/null 2>&1; then
  sudo mkfs.ext4 -F -L slopmud-state "${device}" >/dev/null
fi

uuid="$(sudo blkid -s UUID -o value "${device}")"
sudo install -d -o slopmud -g slopmud -m 0750 "${mountpoint}"
tmp_fstab="$(mktemp)"
grep -v "[[:space:]]${mountpoint}[[:space:]]" /etc/fstab >"${tmp_fstab}" || true
printf "UUID=%s %s ext4 defaults,nofail 0 2\n" "${uuid}" "${mountpoint}" >>"${tmp_fstab}"
sudo install -m 0644 -o root -g root "${tmp_fstab}" /etc/fstab
rm -f "${tmp_fstab}"
sudo mount "${device}" "${mountpoint}"
sudo install -d -o slopmud -g slopmud -m 0750 \
  "${mountpoint}/nearline_scrollback" \
  "${mountpoint}/google_oauth" \
  "${mountpoint}/blob_spool"
sudo chown -R slopmud:slopmud "${mountpoint}"
echo "mounted ${device} at ${mountpoint}"
'

shell_quote() {
  printf '%q' "$1"
}

ensure_gateway() {
  echo "Ensuring gateway data volume mount on ${gateway_host}"
  ssh -o StrictHostKeyChecking=accept-new -p "$SSH_PORT" "${SSH_USER}@${gateway_host}" \
    "SLOPMUD_STATE_DIR=$(shell_quote "$state_dir") REMOTE_ROOT=$(shell_quote "$REMOTE_ROOT") bash -se" \
    <<<"$remote_script"
}

ensure_raft() {
  : "${SHARD_NODE_HOSTS:?missing SHARD_NODE_HOSTS in env file}"
  split_csv "$SHARD_NODE_HOSTS" node_hosts
  if [[ "${#node_hosts[@]}" != "3" ]]; then
    echo "ERROR: SHARD_NODE_HOSTS must contain exactly three comma-separated values" >&2
    exit 2
  fi
  proxy_jump="${SSH_USER}@${gateway_host}"
  if [[ "$SSH_PORT" != "22" ]]; then
    proxy_jump="${proxy_jump}:${SSH_PORT}"
  fi
  raft_ssh_user="${RAFT_SSH_USER:-$SSH_USER}"
  raft_ssh_port="${RAFT_SSH_PORT:-22}"
  for host in "${node_hosts[@]}"; do
    echo "Ensuring Raft data volume mount on ${host}"
    ssh -o StrictHostKeyChecking=accept-new -o "ProxyJump=${proxy_jump}" -p "$raft_ssh_port" "${raft_ssh_user}@${host}" \
      "SLOPMUD_STATE_DIR=$(shell_quote "$state_dir") REMOTE_ROOT=$(shell_quote "$REMOTE_ROOT") bash -se" \
      <<<"$remote_script"
  done
}

case "$mode" in
  gateway) ensure_gateway ;;
  raft) ensure_raft ;;
  all)
    ensure_gateway
    ensure_raft
    ;;
esac
