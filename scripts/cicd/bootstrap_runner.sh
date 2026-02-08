#!/usr/bin/env bash
set -euo pipefail

# One-time bootstrap for the self-hosted runner host:
# - installs build deps (gcc/make/pkg-config)
# - installs rustup toolchain for the runner user
# - installs the privileged deploy hook + sudoers entry
#
# Intended to be run on the target EC2 instance as an admin user with sudo:
#   ./scripts/cicd/bootstrap_runner.sh

if ! command -v sudo >/dev/null 2>&1; then
  echo "ERROR: sudo is required" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

echo "Installing system packages (Debian/Ubuntu)"
sudo apt-get update -y
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential \
  pkg-config \
  ca-certificates \
  curl \
  python3

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
  "$HOME/.cargo/bin/cargo" --version; \
  "$HOME/.cargo/bin/rustc" --version; \
'

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
