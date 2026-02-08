#!/usr/bin/env bash
set -euo pipefail

# certbot sets RENEWED_LINEAGE to the live/ directory for the renewed cert.
: "${RENEWED_LINEAGE:?missing RENEWED_LINEAGE}"

# Copy cert material to a stable path readable by the slopmud service user.
dst_dir=/etc/slopmud/tls
install -d -o slopmud -g slopmud -m 0750 "${dst_dir}"
install -o slopmud -g slopmud -m 0640 "${RENEWED_LINEAGE}/fullchain.pem" "${dst_dir}/fullchain.pem"
install -o slopmud -g slopmud -m 0640 "${RENEWED_LINEAGE}/privkey.pem" "${dst_dir}/privkey.pem"

# Best-effort restart so the app picks up the new cert.
systemctl restart slopmud-web 2>/dev/null || true

