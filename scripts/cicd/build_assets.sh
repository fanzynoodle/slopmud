#!/usr/bin/env bash
set -euo pipefail

# Builds a release binary and packages it into an "asset" tarball under:
#   assets/<track>/<sha>/
#
# Output: prints the path to the created tarball.

track="${TRACK:-dev}"
clean_build="${CLEAN_BUILD:-0}"
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
target_dir="${assets_root}/.cargo-target/${track}"

if [[ "$clean_build" == "1" ]]; then
  rm -rf "$target_dir"
fi

mkdir -p "${out_dir}/bin"

export CARGO_TARGET_DIR="$repo_root/$target_dir"

echo "Building (track=${track}, clean=${clean_build}, sha=${sha})"
cargo build -p slopmud --release

bin_src="${CARGO_TARGET_DIR}/release/slopmud"
if [[ ! -x "$bin_src" ]]; then
  echo "ERROR: expected binary at ${bin_src}" >&2
  exit 2
fi

cp -f "$bin_src" "${out_dir}/bin/slopmud"

cat >"${out_dir}/BUILD_INFO.txt" <<EOF
sha=${sha}
track=${track}
clean_build=${clean_build}
built_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cargo=$(cargo --version 2>/dev/null || true)
rustc=$(rustc --version 2>/dev/null || true)
EOF

tarball="${out_dir}/artifact.tgz"
tar -C "$out_dir" -czf "$tarball" bin BUILD_INFO.txt

echo "$tarball"
