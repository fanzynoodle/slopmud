#!/usr/bin/env bash
set -euo pipefail

env_file="${1:-env/prd.env}"
if [[ ! -f "$env_file" ]]; then
  echo "ERROR: env file not found: $env_file" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

: "${HOST:?missing HOST in env file}"
: "${SSH_USER:?missing SSH_USER in env file}"
: "${SSH_PORT:?missing SSH_PORT in env file}"
: "${REMOTE_WEB:?missing REMOTE_WEB in env file}"
: "${DOMAIN:?missing DOMAIN in env file}"

ssh_opts=(-o StrictHostKeyChecking=accept-new)
ssh_port_opt=(-p "$SSH_PORT")
scp_port_opt=(-P "$SSH_PORT")

echo "Deploying web_homepage -> ${SSH_USER}@${HOST}:${REMOTE_WEB}"

ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  if command -v apt-get >/dev/null 2>&1; then \
    sudo DEBIAN_FRONTEND=noninteractive apt-get update -y; \
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y nginx rsync; \
  elif command -v dnf >/dev/null 2>&1; then \
    sudo dnf -y install nginx rsync; \
    sudo systemctl enable --now nginx; \
  else \
    echo 'Unsupported OS (need apt-get or dnf)'; exit 2; \
  fi; \
  sudo mkdir -p \"${REMOTE_WEB}\"; \
  sudo chown -R \"${SSH_USER}:${SSH_USER}\" \"${REMOTE_WEB}\" \
"

rsync -az --delete --exclude README.md -e "ssh ${ssh_opts[*]} ${ssh_port_opt[*]}" web_homepage/ "${SSH_USER}@${HOST}:${REMOTE_WEB}/"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
cat >"$tmp" <<EOF
server {
  listen 80 default_server;
  listen [::]:80 default_server;

  server_name ${DOMAIN} www.${DOMAIN} mud.${DOMAIN};

  root ${REMOTE_WEB};
  index index.html;

  location / {
    try_files \$uri \$uri/ /index.html;
  }
}
EOF

scp "${ssh_opts[@]}" "${scp_port_opt[@]}" "$tmp" "${SSH_USER}@${HOST}:/tmp/slopmud.nginx"

ssh "${ssh_opts[@]}" "${ssh_port_opt[@]}" "${SSH_USER}@${HOST}" "\
  set -euo pipefail; \
  sudo mv /tmp/slopmud.nginx /etc/nginx/sites-available/slopmud; \
  sudo ln -sf /etc/nginx/sites-available/slopmud /etc/nginx/sites-enabled/slopmud; \
  sudo rm -f /etc/nginx/sites-enabled/default; \
  sudo nginx -t; \
  sudo systemctl enable --now nginx; \
  sudo systemctl reload nginx \
"

echo "Smoke test (direct IP, Host header = ${DOMAIN})"
curl -fsSL -H "Host: ${DOMAIN}" "http://${HOST}/" | sed -n '1,20p'

echo "Smoke test (DNS if delegated)"
curl -fsSL "http://${DOMAIN}/" | sed -n '1,20p' || true
