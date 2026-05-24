#!/usr/bin/env bash
set -euo pipefail

pkgs=("$@")
if ((${#pkgs[@]} == 0)); then
  echo "USAGE: $0 <cargo-package-name> [cargo-package-name ...]" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_sha="$(git -C "${repo_root}" rev-parse --short HEAD 2>/dev/null || true)"
git_dirty="0"
if ! git -C "${repo_root}" diff --quiet 2>/dev/null || ! git -C "${repo_root}" diff --cached --quiet 2>/dev/null; then
  git_dirty="1"
fi
cargo_jobs="${SLOPMUD_CARGO_BUILD_JOBS:-${CARGO_BUILD_JOBS:-}}"
if [[ -n "${cargo_jobs}" && ! "${cargo_jobs}" =~ ^[0-9]+$ ]]; then
  echo "ERROR: SLOPMUD_CARGO_BUILD_JOBS/CARGO_BUILD_JOBS must be an integer" >&2
  exit 2
fi
build_profile="${SLOPMUD_CARGO_PROFILE:-release}"
if ! [[ "${build_profile}" =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo "ERROR: SLOPMUD_CARGO_PROFILE must contain only letters, numbers, underscore, or dash" >&2
  exit 2
fi

build_cmd=(cargo build)
for pkg in "${pkgs[@]}"; do
  build_cmd+=(-p "${pkg}")
done
case "${build_profile}" in
  dev) ;;
  release) build_cmd+=(--release) ;;
  *) build_cmd+=(--profile "${build_profile}") ;;
esac
if [[ -n "${cargo_jobs}" ]]; then
  build_cmd+=(-j "${cargo_jobs}")
fi
container_build_cmd=(/usr/local/cargo/bin/cargo "${build_cmd[@]:1}")

if command -v podman >/dev/null 2>&1; then
  # Needs a Cargo new enough for edition=2024.
  image="${SLOPMUD_BUILD_IMAGE:-docker.io/rust:1.89-bookworm}"
  podman_env=()
  if [[ -n "${cargo_jobs}" ]]; then
    podman_env+=(-e "CARGO_BUILD_JOBS=${cargo_jobs}")
  fi
  # Build inside Debian 12 (bookworm) so the produced binary runs on the mudbox
  # (Debian 12 ships an older glibc than many dev machines).
  podman run --rm \
    --userns=keep-id \
    -e CARGO_HOME=/cargo \
    -e RUSTC=/usr/local/cargo/bin/rustc \
    -e SLOPMUD_GIT_SHA="${git_sha}" \
    -e SLOPMUD_GIT_DIRTY="${git_dirty}" \
    "${podman_env[@]}" \
    -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -v "${HOME}/.cargo:/cargo:Z" \
    -v "${repo_root}:/work:Z" \
    -w /work \
    "${image}" \
    bash -lc "$(printf '%q ' "${container_build_cmd[@]}")"
else
  echo "podman not found; falling back to local build (may produce a binary incompatible with Debian 12)" >&2
  SLOPMUD_GIT_SHA="${git_sha}" SLOPMUD_GIT_DIRTY="${git_dirty}" "${build_cmd[@]}"
fi
