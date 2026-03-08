#!/usr/bin/env bash
set -euo pipefail

# Packages the instance rebootstrap helper into a small tarball that user_data
# can fetch from a stable S3 key.

assets_root="${ASSETS_ROOT:-assets}"
bundle_dir="${assets_root}/bootstrap/mudbox"
out_dir="${bundle_dir}/bundle"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

rm -rf "$out_dir"
mkdir -p "$out_dir"

cp -f "scripts/cicd/restore_onebox_stack.sh" "${out_dir}/restore_onebox_stack.sh"
cat >"${out_dir}/run.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/restore_onebox_stack.sh" "$@"
EOF
chmod 0755 "${out_dir}/restore_onebox_stack.sh" "${out_dir}/run.sh"

tarball="${bundle_dir}/rebootstrap.tgz"
mkdir -p "$bundle_dir"
tar -C "$out_dir" -czf "$tarball" run.sh restore_onebox_stack.sh

echo "$tarball"
