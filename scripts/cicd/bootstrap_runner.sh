#!/usr/bin/env bash
set -euo pipefail

# One-time bootstrap for the self-hosted runner host:
# - installs build deps (gcc/make/pkg-config/git)
# - installs rustup toolchain for the runner user
# - installs the privileged deploy hook + sudoers entry
# - enables a small swapfile so release Rust builds fit on the smallest mud box
#
# Intended to be run on the target EC2 instance as an admin user with sudo:
#   ./scripts/cicd/bootstrap_runner.sh

if ! command -v sudo >/dev/null 2>&1; then
  echo "ERROR: sudo is required" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

swapfile_path="${RUNNER_SWAPFILE_PATH:-/swapfile}"
swapfile_mb="${RUNNER_SWAPFILE_MB:-1024}"

echo "Installing system packages (Debian/Ubuntu)"
sudo apt-get update -y
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential \
  git \
  pkg-config \
  libssl-dev \
  ca-certificates \
  curl \
  python3 \
  awscli \
  ripgrep

echo "Ensuring runner swapfile (${swapfile_path}, ${swapfile_mb} MiB)"
if ! sudo /sbin/swapon --show=NAME --noheadings | grep -qx "${swapfile_path}"; then
  if [[ ! -f "${swapfile_path}" ]]; then
    sudo fallocate -l "${swapfile_mb}M" "${swapfile_path}"
    sudo chmod 600 "${swapfile_path}"
    sudo /sbin/mkswap "${swapfile_path}" >/dev/null
  fi
  sudo /sbin/swapon "${swapfile_path}"
fi
if ! sudo grep -qF "${swapfile_path} none swap sw 0 0" /etc/fstab; then
  echo "${swapfile_path} none swap sw 0 0" | sudo tee -a /etc/fstab >/dev/null
fi

if ! id -u ghrunner >/dev/null 2>&1; then
  echo "ERROR: expected runner user 'ghrunner' to exist on this host" >&2
  exit 2
fi

echo "Installing rustup for ghrunner (if missing)"
sudo -u ghrunner -H bash -lc ' \
  set -euo pipefail; \
  if [ ! -x "$HOME/.cargo/bin/rustup" ]; then \
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal; \
  fi; \
  "$HOME/.cargo/bin/rustup" toolchain install stable --profile minimal; \
  "$HOME/.cargo/bin/rustup" default stable; \
  "$HOME/.cargo/bin/rustup" component add rustfmt; \
  "$HOME/.cargo/bin/cargo" --version; \
  "$HOME/.cargo/bin/rustc" --version; \
'

echo "Installing just for ghrunner (if missing)"
just_path="$(
  sudo -u ghrunner -H bash -lc ' \
    set -euo pipefail; \
    source "$HOME/.cargo/env"; \
    if ! command -v just >/dev/null 2>&1; then \
      cargo install just --locked; \
    fi; \
    command -v just; \
  '
)"
sudo ln -sf "$just_path" /usr/local/bin/just

echo "Installing deploy hook to /usr/local/bin/slopmud-shuttle-assets"
sudo install -m 0755 scripts/cicd/slopmud-shuttle-assets /usr/local/bin/slopmud-shuttle-assets

echo "Creating /opt/slopmud/assets"
sudo mkdir -p /opt/slopmud/assets

echo "Installing sudoers rule for ghrunner -> slopmud-shuttle-assets"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
cat >"$tmp" <<'EOF'
ghrunner ALL=(root) NOPASSWD: /usr/local/bin/slopmud-shuttle-assets
EOF
sudo install -m 0440 "$tmp" /etc/sudoers.d/ghrunner-slopmud-shuttle-assets
sudo visudo -cf /etc/sudoers.d/ghrunner-slopmud-shuttle-assets >/dev/null

echo "Bootstrap complete."
