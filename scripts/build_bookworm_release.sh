#!/usr/bin/env bash
set -euo pipefail

pkg="${1:-}"
if [[ -z "${pkg}" ]]; then
  echo "USAGE: $0 <cargo-package-name>" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if command -v podman >/dev/null 2>&1; then
  # Needs a Cargo new enough for edition=2024.
  image="${SLOPMUD_BUILD_IMAGE:-docker.io/rust:1.89-bookworm}"
  # Build inside Debian 12 (bookworm) so the produced binary runs on the mudbox
  # (Debian 12 ships an older glibc than many dev machines).
  podman run --rm \
    --userns=keep-id \
    -e CARGO_HOME=/cargo \
    -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -v "${HOME}/.cargo:/cargo:Z" \
    -v "${repo_root}:/work:Z" \
    -w /work \
    "${image}" \
    bash -lc "/usr/local/cargo/bin/cargo build -p \"${pkg}\" --release"
else
  echo "podman not found; falling back to local build (may produce a binary incompatible with Debian 12)" >&2
  cargo build -p "${pkg}" --release
fi
