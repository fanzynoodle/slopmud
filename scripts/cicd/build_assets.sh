#!/usr/bin/env bash
set -euo pipefail

# Builds release binaries and packages them into an "asset" tarball under:
#   assets/<track>/<sha>/
#
# Output: prints the path to the created tarball.

track="${TRACK:-dev}"
clean_build="${CLEAN_BUILD:-0}"
build_shard="${BUILD_SHARD:-1}"
build_static_web="${BUILD_STATIC_WEB:-1}"
build_slopmud_web="${BUILD_SLOPMUD_WEB:-1}"
build_internal_oidc="${BUILD_INTERNAL_OIDC:-1}"
assets_root="${ASSETS_ROOT:-assets}"
assets_env_dir="${ASSETS_ENV_DIR:-${PWD}/env}"
assets_env_files="${ASSETS_ENV_FILES:-}"
sha="${GITHUB_SHA:-}"

if [[ -z "$sha" ]]; then
  sha="$(git rev-parse HEAD 2>/dev/null || true)"
fi
if [[ -z "$sha" ]]; then
  echo "ERROR: missing build SHA (set GITHUB_SHA or run in a git repo)" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

assets_env_dir="${ASSETS_ENV_DIR:-${repo_root}/env}"

out_dir="${assets_root}/${track}/${sha}"
target_dir="${BUILD_ASSETS_TARGET_DIR:-${repo_root}/target}"

if [[ "$clean_build" == "1" ]]; then
  rm -rf "$target_dir"
fi

mkdir -p "${assets_root}/${track}"
rm -rf "$out_dir"

mkdir -p "${out_dir}/bin"
mkdir -p "${out_dir}/web_homepage"
mkdir -p "${out_dir}/env"

echo "Building (track=${track}, clean=${clean_build}, sha=${sha})" >&2
# Keep stdout clean so callers can safely capture the artifact path.
# Build inside Debian 12 (bookworm) so artifacts are compatible with mudbox hosts.
./scripts/build_bookworm_release.sh slopmud 1>&2

if [[ "$build_shard" == "1" ]]; then
  echo "Building shard_01 (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh shard_01 1>&2
else
  echo "Skipping shard_01 build (BUILD_SHARD=${build_shard})" >&2
fi

if [[ "$build_static_web" == "1" ]]; then
  echo "Building static_web (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh static_web 1>&2
fi

if [[ "$build_slopmud_web" == "1" ]]; then
  echo "Building slopmud_web (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh slopmud_web 1>&2
fi

if [[ "$build_internal_oidc" == "1" ]]; then
  echo "Building internal_oidc (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh internal_oidc 1>&2
fi

bin_src="${repo_root}/target/release/slopmud"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at ${bin_src}" >&2
  exit 2
fi
bin_shard_src="${repo_root}/target/release/shard_01"
if [[ "$build_shard" == "1" ]]; then
  if [[ ! -x "$bin_shard_src" ]]; then
    echo "ERROR: expected binary at ${bin_shard_src}" >&2
    exit 2
  fi
fi

bin_static_web_src="${repo_root}/target/release/static_web"
if [[ "$build_static_web" == "1" && ! -x "$bin_static_web_src" ]]; then
  echo "ERROR: expected binary at ${bin_static_web_src}" >&2
  exit 2
fi

bin_slopmud_web_src="${repo_root}/target/release/slopmud_web"
if [[ "$build_slopmud_web" == "1" && ! -x "$bin_slopmud_web_src" ]]; then
  echo "ERROR: expected binary at ${bin_slopmud_web_src}" >&2
  exit 2
fi

bin_internal_oidc_src="${repo_root}/target/release/internal_oidc"
if [[ "$build_internal_oidc" == "1" && ! -x "$bin_internal_oidc_src" ]]; then
  echo "ERROR: expected binary at ${bin_internal_oidc_src}" >&2
  exit 2
fi

cp -f "$bin_src" "${out_dir}/bin/slopmud"
if [[ "$build_shard" == "1" ]]; then
  cp -f "$bin_shard_src" "${out_dir}/bin/shard_01"
fi
if [[ "$build_static_web" == "1" ]]; then
  cp -f "$bin_static_web_src" "${out_dir}/bin/static_web"
fi
if [[ "$build_slopmud_web" == "1" ]]; then
  cp -f "$bin_slopmud_web_src" "${out_dir}/bin/slopmud_web"
fi
if [[ "$build_internal_oidc" == "1" ]]; then
  cp -f "$bin_internal_oidc_src" "${out_dir}/bin/internal_oidc"
fi

if [[ ! -d "$assets_env_dir" ]]; then
  echo "ERROR: missing env dir for asset bundle: ${assets_env_dir}" >&2
  echo "Set ASSETS_ENV_DIR=/path/to/env when building from a worktree without env/" >&2
  exit 2
fi

if [[ -n "$assets_env_files" ]]; then
  for env_name in $assets_env_files; do
    if [[ ! -f "${assets_env_dir}/${env_name}" ]]; then
      echo "ERROR: missing env file for asset bundle: ${assets_env_dir}/${env_name}" >&2
      exit 2
    fi
    cp -a "${assets_env_dir}/${env_name}" "${out_dir}/env/${env_name}"
  done
else
  cp -a "${assets_env_dir}/." "${out_dir}/env/"
fi

cp -a "${repo_root}/web_homepage/." "${out_dir}/web_homepage/"

cat >"${out_dir}/BUILD_INFO.txt" <<EOF
sha=${sha}
track=${track}
clean_build=${clean_build}
built_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cargo=$(cargo --version 2>/dev/null || true)
rustc=$(rustc --version 2>/dev/null || true)
EOF

tarball="${out_dir}/artifact.tgz"
tar -C "$out_dir" -czf "$tarball" bin env web_homepage BUILD_INFO.txt

echo "$tarball"
