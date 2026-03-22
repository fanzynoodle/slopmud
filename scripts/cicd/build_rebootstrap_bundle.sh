#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out_path="${1:-${repo_root}/tmp/rebootstrap.tgz}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

install -d -m 0755 "$tmpdir"
cp "${repo_root}/scripts/cicd/restore_onebox_stack.sh" "$tmpdir/restore_onebox_stack.sh"
cp "${repo_root}/scripts/tls_cache_ssm.sh" "$tmpdir/tls_cache_ssm.sh"
cp "${repo_root}/scripts/certbot_deploy_hook_slopmud.sh" "$tmpdir/certbot_deploy_hook_slopmud.sh"
cat >"$tmpdir/run.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/restore_onebox_stack.sh" "$@"
EOF
chmod 0755 "$tmpdir/run.sh" "$tmpdir/restore_onebox_stack.sh" "$tmpdir/tls_cache_ssm.sh" "$tmpdir/certbot_deploy_hook_slopmud.sh"

mkdir -p "$(dirname "$out_path")"
tar -czf "$out_path" -C "$tmpdir" .
echo "$out_path"
