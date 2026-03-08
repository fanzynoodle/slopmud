#!/usr/bin/env bash
set -euo pipefail

# Builds release binaries and packages them into an "asset" tarball under:
#   assets/<track>/<sha>/
#
# Output: prints the path to the created tarball.

track="${TRACK:-dev}"
clean_build="${CLEAN_BUILD:-0}"
build_shard="${BUILD_SHARD:-1}"
build_web_stack="${BUILD_WEB_STACK:-1}"
assets_root="${ASSETS_ROOT:-assets}"
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

out_dir="${assets_root}/${track}/${sha}"
target_dir="${BUILD_ASSETS_TARGET_DIR:-${repo_root}/target}"

if [[ "$clean_build" == "1" ]]; then
  rm -rf "$target_dir"
fi

mkdir -p "${assets_root}/${track}"

mkdir -p "${out_dir}/bin"
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

if [[ "$build_web_stack" == "1" ]]; then
  echo "Building internal_oidc (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh internal_oidc 1>&2
  echo "Building static_web (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh static_web 1>&2
  echo "Building slopmud_web (track=${track}, clean=${clean_build}, sha=${sha})" >&2
  ./scripts/build_bookworm_release.sh slopmud_web 1>&2
else
  echo "Skipping web stack build (BUILD_WEB_STACK=${build_web_stack})" >&2
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

bin_oidc_src="${repo_root}/target/release/internal_oidc"
bin_static_web_src="${repo_root}/target/release/static_web"
bin_slopmud_web_src="${repo_root}/target/release/slopmud_web"
if [[ "$build_web_stack" == "1" ]]; then
  if [[ ! -x "$bin_oidc_src" ]]; then
    echo "ERROR: expected binary at ${bin_oidc_src}" >&2
    exit 2
  fi
  if [[ ! -x "$bin_static_web_src" ]]; then
    echo "ERROR: expected binary at ${bin_static_web_src}" >&2
    exit 2
  fi
  if [[ ! -x "$bin_slopmud_web_src" ]]; then
    echo "ERROR: expected binary at ${bin_slopmud_web_src}" >&2
    exit 2
  fi
fi

cp -f "$bin_src" "${out_dir}/bin/slopmud"
if [[ "$build_shard" == "1" ]]; then
  cp -f "$bin_shard_src" "${out_dir}/bin/shard_01"
fi
if [[ "$build_web_stack" == "1" ]]; then
  cp -f "$bin_oidc_src" "${out_dir}/bin/internal_oidc"
  cp -f "$bin_static_web_src" "${out_dir}/bin/static_web"
  cp -f "$bin_slopmud_web_src" "${out_dir}/bin/slopmud_web"
fi

env_prefix="$track"
if [[ "$track" == "prod" ]]; then
  env_prefix="prd"
fi

if [[ -d "${repo_root}/env" ]]; then
  env_matches=()
  if [[ -f "${repo_root}/env/${env_prefix}.env" ]]; then
    env_matches+=("${repo_root}/env/${env_prefix}.env")
  fi
  shopt -s nullglob
  for env_path in "${repo_root}/env/${env_prefix}-"*.env; do
    env_matches+=("$env_path")
  done
  shopt -u nullglob
  if [[ "${#env_matches[@]}" -eq 0 ]]; then
    echo "WARN: no env files matched env/${env_prefix}.env or env/${env_prefix}-*.env; replacement bootstrap will lack an env bundle" >&2
  else
    for env_path in "${env_matches[@]}"; do
      cp -f "$env_path" "${out_dir}/env/$(basename "$env_path")"
    done
  fi
else
  echo "WARN: env directory is missing; replacement bootstrap will lack an env bundle" >&2
fi

if [[ -d "${repo_root}/web_homepage" ]]; then
  cp -a "${repo_root}/web_homepage" "${out_dir}/web_homepage"
else
  echo "ERROR: expected directory at ${repo_root}/web_homepage" >&2
  exit 2
fi

cat >"${out_dir}/BUILD_INFO.txt" <<EOF
sha=${sha}
track=${track}
clean_build=${clean_build}
build_web_stack=${build_web_stack}
built_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cargo=$(cargo --version 2>/dev/null || true)
rustc=$(rustc --version 2>/dev/null || true)
EOF

tarball="${out_dir}/artifact.tgz"
tar -C "$out_dir" -czf "$tarball" bin env web_homepage BUILD_INFO.txt

echo "$tarball"
