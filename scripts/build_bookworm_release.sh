#!/usr/bin/env bash
set -euo pipefail

pkg="${1:-}"
if [[ -z "${pkg}" ]]; then
  echo "USAGE: $0 <cargo-package-name>" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_sha="$(git -C "${repo_root}" rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
if git -C "${repo_root}" diff --quiet --ignore-submodules HEAD 2>/dev/null; then
  git_dirty="0"
else
  git_dirty="1"
fi
build_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
build_unix="$(date -u +%s)"
profile="release"

if command -v podman >/dev/null 2>&1; then
  # Needs a Cargo new enough for edition=2024.
  image="${SLOPMUD_BUILD_IMAGE:-docker.io/rust:1.89-bookworm}"
  # Build inside Debian 12 (bookworm) so the produced binary runs on the mudbox
  # (Debian 12 ships an older glibc than many dev machines).
  podman run --rm \
    --userns=keep-id \
    -e CARGO_HOME=/cargo \
    -e SLOPMUD_GIT_SHA="${git_sha}" \
    -e SLOPMUD_GIT_DIRTY="${git_dirty}" \
    -e SLOPMUD_BUILD_UTC="${build_utc}" \
    -e SLOPMUD_BUILD_UNIX="${build_unix}" \
    -e SLOPMUD_PROFILE="${profile}" \
    -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -v "${HOME}/.cargo:/cargo:Z" \
    -v "${repo_root}:/work:Z" \
    -w /work \
    "${image}" \
    bash -lc "/usr/local/cargo/bin/cargo build -p \"${pkg}\" --release"
else
  echo "podman not found; falling back to local build (may produce a binary incompatible with Debian 12)" >&2
  export SLOPMUD_GIT_SHA="${git_sha}"
  export SLOPMUD_GIT_DIRTY="${git_dirty}"
  export SLOPMUD_BUILD_UTC="${build_utc}"
  export SLOPMUD_BUILD_UNIX="${build_unix}"
  export SLOPMUD_PROFILE="${profile}"
  cargo build -p "${pkg}" --release
fi
