#!/usr/bin/env bash
set -euo pipefail

# certbot sets RENEWED_LINEAGE to the live/ directory for the renewed cert.
: "${RENEWED_LINEAGE:?missing RENEWED_LINEAGE}"

expected="${CERTBOT_CERT_NAME:-slopmud.com}"
dst_dir="${TLS_DST_DIR:-/etc/slopmud/tls}"
svc="${WEB_SERVICE_NAME:-slopmud-web}"

# Ignore renewals for other cert names on shared hosts.
if [[ "$(basename "${RENEWED_LINEAGE}")" != "${expected}" ]]; then
  exit 0
fi

# Copy cert material to a stable path readable by the slopmud service user.
install -d -o slopmud -g slopmud -m 0750 "${dst_dir}"
install -o slopmud -g slopmud -m 0640 "${RENEWED_LINEAGE}/fullchain.pem" "${dst_dir}/fullchain.pem"
install -o slopmud -g slopmud -m 0640 "${RENEWED_LINEAGE}/privkey.pem" "${dst_dir}/privkey.pem"

# Best-effort restart so the app picks up the new cert.
systemctl restart "${svc}" 2>/dev/null || true
