#!/usr/bin/env bash
set -euo pipefail

# Builds release binaries and packages them into an "asset" tarball under:
#   assets/<track>/<sha>/
#
# Output: prints the path to the created tarball.

track="${TRACK:-dev}"
clean_build="${CLEAN_BUILD:-0}"
build_shard="${BUILD_SHARD:-1}"
build_walbackupd="${BUILD_WALBACKUPD:-1}"
build_static_web="${BUILD_STATIC_WEB:-1}"
build_slopmud_web="${BUILD_SLOPMUD_WEB:-1}"
build_internal_oidc="${BUILD_INTERNAL_OIDC:-1}"
build_adminctl="${BUILD_SLOPMUD_ADMINCTL:-1}"
assets_root="${ASSETS_ROOT:-assets}"
assets_env_dir="${ASSETS_ENV_DIR:-${PWD}/env}"
assets_env_files="${ASSETS_ENV_FILES:-}"
assets_env_required="${ASSETS_ENV_REQUIRED:-0}"
sha="${GITHUB_SHA:-}"
build_profile="${SLOPMUD_CARGO_PROFILE:-}"

if [[ -z "$build_profile" ]]; then
  case "$track" in
    dev|sandbox) build_profile="devdeploy" ;;
    *) build_profile="release" ;;
  esac
fi

if ! [[ "$build_profile" =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo "ERROR: SLOPMUD_CARGO_PROFILE must contain only letters, numbers, underscore, or dash" >&2
  exit 2
fi

case "$build_profile" in
  dev) profile_target_dir="debug" ;;
  release) profile_target_dir="release" ;;
  *) profile_target_dir="$build_profile" ;;
esac
export SLOPMUD_CARGO_PROFILE="$build_profile"

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

echo "Building (track=${track}, profile=${build_profile}, clean=${clean_build}, sha=${sha})" >&2
# Keep stdout clean so callers can safely capture the artifact path.
# Build inside Debian 12 (bookworm) so artifacts are compatible with mudbox hosts.
build_packages=(slopmud)

if [[ "$build_shard" == "1" ]]; then
  build_packages+=(shard_01)
else
  echo "Skipping shard_01 build (BUILD_SHARD=${build_shard})" >&2
fi

if [[ "$build_walbackupd" == "1" ]]; then
  build_packages+=(slopmud_walbackupd)
else
  echo "Skipping slopmud_walbackupd build (BUILD_WALBACKUPD=${build_walbackupd})" >&2
fi

if [[ "$build_static_web" == "1" ]]; then
  build_packages+=(static_web)
else
  echo "Skipping static_web build (BUILD_STATIC_WEB=${build_static_web})" >&2
fi

if [[ "$build_slopmud_web" == "1" ]]; then
  build_packages+=(slopmud_web)
else
  echo "Skipping slopmud_web build (BUILD_SLOPMUD_WEB=${build_slopmud_web})" >&2
fi

if [[ "$build_internal_oidc" == "1" ]]; then
  build_packages+=(internal_oidc)
else
  echo "Skipping internal_oidc build (BUILD_INTERNAL_OIDC=${build_internal_oidc})" >&2
fi

if [[ "$build_adminctl" == "1" ]]; then
  build_packages+=(slopmud_adminctl)
else
  echo "Skipping slopmud_adminctl build (BUILD_SLOPMUD_ADMINCTL=${build_adminctl})" >&2
fi

echo "Building Cargo packages: ${build_packages[*]}" >&2
./scripts/build_bookworm_release.sh "${build_packages[@]}" 1>&2

bin_src="${repo_root}/target/${profile_target_dir}/slopmud"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at ${bin_src}" >&2
  exit 2
fi
bin_shard_src="${repo_root}/target/${profile_target_dir}/shard_01"
if [[ "$build_shard" == "1" ]]; then
  if [[ ! -x "$bin_shard_src" ]]; then
    echo "ERROR: expected binary at ${bin_shard_src}" >&2
    exit 2
  fi
fi
bin_walbackupd_src="${repo_root}/target/${profile_target_dir}/slopmud_walbackupd"
if [[ "$build_walbackupd" == "1" && ! -x "$bin_walbackupd_src" ]]; then
  echo "ERROR: expected binary at ${bin_walbackupd_src}" >&2
  exit 2
fi

bin_static_web_src="${repo_root}/target/${profile_target_dir}/static_web"
if [[ "$build_static_web" == "1" && ! -x "$bin_static_web_src" ]]; then
  echo "ERROR: expected binary at ${bin_static_web_src}" >&2
  exit 2
fi

bin_slopmud_web_src="${repo_root}/target/${profile_target_dir}/slopmud_web"
if [[ "$build_slopmud_web" == "1" && ! -x "$bin_slopmud_web_src" ]]; then
  echo "ERROR: expected binary at ${bin_slopmud_web_src}" >&2
  exit 2
fi

bin_internal_oidc_src="${repo_root}/target/${profile_target_dir}/internal_oidc"
if [[ "$build_internal_oidc" == "1" && ! -x "$bin_internal_oidc_src" ]]; then
  echo "ERROR: expected binary at ${bin_internal_oidc_src}" >&2
  exit 2
fi
bin_adminctl_src="${repo_root}/target/${profile_target_dir}/slopmud_adminctl"
if [[ "$build_adminctl" == "1" && ! -x "$bin_adminctl_src" ]]; then
  echo "ERROR: expected binary at ${bin_adminctl_src}" >&2
  exit 2
fi

cp -f "$bin_src" "${out_dir}/bin/slopmud"
if [[ "$build_shard" == "1" ]]; then
  cp -f "$bin_shard_src" "${out_dir}/bin/shard_01"
fi
if [[ "$build_walbackupd" == "1" ]]; then
  cp -f "$bin_walbackupd_src" "${out_dir}/bin/slopmud_walbackupd"
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
if [[ "$build_adminctl" == "1" ]]; then
  cp -f "$bin_adminctl_src" "${out_dir}/bin/slopmud_adminctl"
fi

if [[ -n "$assets_env_files" ]]; then
  if [[ ! -d "$assets_env_dir" ]]; then
    echo "ERROR: missing explicit env dir for asset bundle: ${assets_env_dir}" >&2
    echo "ASSETS_ENV_FILES was set, so ASSETS_ENV_DIR must point at a readable env directory." >&2
    exit 2
  fi
  for env_name in $assets_env_files; do
    if [[ ! -f "${assets_env_dir}/${env_name}" ]]; then
      echo "ERROR: missing env file for asset bundle: ${assets_env_dir}/${env_name}" >&2
      exit 2
    fi
    cp -a "${assets_env_dir}/${env_name}" "${out_dir}/env/${env_name}"
  done
elif [[ -d "$assets_env_dir" ]]; then
  cp -a "${assets_env_dir}/." "${out_dir}/env/"
else
  case "$assets_env_required" in
    1|true|TRUE|yes|YES|on|ON)
      echo "ERROR: missing required env dir for asset bundle: ${assets_env_dir}" >&2
      echo "Unset ASSETS_ENV_REQUIRED or set ASSETS_ENV_DIR=/path/to/env for this build." >&2
      exit 2
      ;;
    *)
      echo "WARN: missing optional env dir for asset bundle: ${assets_env_dir}; continuing with empty env bundle" >&2
      ;;
  esac
fi

cp -a "${repo_root}/web_homepage/." "${out_dir}/web_homepage/"
mkdir -p "${out_dir}/scripts"
cp -f "${repo_root}/scripts/restore_wal_backup.sh" "${out_dir}/scripts/restore_wal_backup.sh"
chmod 0755 "${out_dir}/scripts/restore_wal_backup.sh"

cat >"${out_dir}/BUILD_INFO.txt" <<EOF
sha=${sha}
track=${track}
profile=${build_profile}
clean_build=${clean_build}
built_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cargo=$(cargo --version 2>/dev/null || true)
rustc=$(rustc --version 2>/dev/null || true)
EOF

tarball="${out_dir}/artifact.tgz"
tar -C "$out_dir" -czf "$tarball" bin env web_homepage scripts BUILD_INFO.txt

echo "$tarball"
