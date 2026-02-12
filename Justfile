set shell := ["bash", "-ceu"]

# SSH target defaults
user := "admin"
ssh_opts := "-o StrictHostKeyChecking=accept-new"

help:
  @just --list

# --- Local sanity checks ---

fmt:
  cargo fmt --all

fmt-check:
  cargo fmt --all -- --check

clippy:
  cargo clippy --workspace --all-targets

py-check:
  python3 -m compileall -q scripts

check:
  just fmt-check
  just py-check
  RUSTFLAGS='-D warnings' cargo build -q --workspace --all-targets
  RUSTFLAGS='-D warnings' cargo test -q
  just world-validate
  just proto-lint
  just proto-coverage
  # Some sandboxed/dev environments block `socket()` syscalls (EPERM), which makes
  # local end-to-end tests impossible even on 127.0.0.1. In that case, skip e2e.
  if python3 -c $'import errno, socket, sys\ntry:\n    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n    s.close()\nexcept OSError as e:\n    if e.errno == errno.EPERM:\n        sys.exit(10)\n    print(f\"error: socket probe failed: {e}\", file=sys.stderr)\n    sys.exit(11)\nsys.exit(0)\n'; then just e2e-local; just e2e-party; just e2e-ws; just e2e-groups; else rc=$?; if [ $rc -eq 10 ]; then echo "skipping e2e (TCP sockets not permitted in this environment)"; else exit $rc; fi; fi

e2e-groups:
  python3 scripts/e2e_groups_raft.py

# Load env/<name>.env into the recipe environment.
_with-env env cmd:
  bash -ceu 'set -o pipefail; set -a; source "env/{{env}}.env"; set +a; {{cmd}}'

# --- Certbot (remote) ---

# Install certbot + Route53 DNS plugin (uses instance role creds).
certbot-install env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    just certbot-install-host "${HOST}" "${SSH_USER}" "${SSH_PORT}"; \
  '

certbot-install-host host ssh_user="admin" ssh_port="22":
  ssh {{ssh_opts}} -p {{ssh_port}} {{ssh_user}}@{{host}} ' \
    if command -v apt-get >/dev/null 2>&1; then \
      sudo apt-get update; \
      sudo apt-get install -y certbot python3-certbot-dns-route53; \
    elif command -v dnf >/dev/null 2>&1; then \
      sudo dnf -y install certbot python3-certbot-dns-route53 || { \
        sudo dnf -y install certbot python3-pip; \
        sudo python3 -m pip install --upgrade pip; \
        sudo python3 -m pip install certbot-dns-route53; \
      }; \
    else \
      echo "Unsupported OS (need apt-get or dnf)"; \
      exit 2; \
    fi; \
    certbot --version || true; \
    sudo systemctl enable --now certbot.timer 2>/dev/null || true; \
  '

# Issue/renew a certificate via DNS-01 (Route53). Idempotent; safe to re-run.
certbot-issue email env="prd" domain="":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    domain="{{domain}}"; \
    if [ -z "${domain}" ]; then domain="${DOMAIN}"; fi; \
    just certbot-issue-host "${HOST}" "${domain}" "{{email}}" "${SSH_USER}" "${SSH_PORT}"; \
  '

certbot-issue-host host domain email ssh_user="admin" ssh_port="22":
  ssh {{ssh_opts}} -p {{ssh_port}} {{ssh_user}}@{{host}} ' \
    sudo certbot certonly \
      --dns-route53 \
      -d {{domain}} \
      --non-interactive --agree-tos \
      -m {{email}} \
      --keep-until-expiring; \
    sudo systemctl enable --now certbot.timer 2>/dev/null || true; \
    sudo certbot certificates || true; \
  '

# Issue/renew a certificate for DOMAIN + www.DOMAIN (DNS-01 via Route53).
#
# If `email` is empty, certbot will register without an email (still supports auto-renew via certbot.timer).
# NOTE: `staging=1` means "Let's Encrypt test cert" (certbot `--test-cert`), not the slopmud `stg` environment.
certbot-issue-web email="" env="prd" domain="" staging="0":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    base="{{domain}}"; \
    if [ -z "${base}" ]; then base="${DOMAIN}"; fi; \
    email="{{email}}"; \
    dst_dir="${TLS_DST_DIR:-/etc/slopmud/tls}"; \
    svc="${WEB_SERVICE_NAME:-slopmud-web}"; \
    cert_name="${CERTBOT_CERT_NAME:-${base}}"; \
    args=""; \
    if [ "{{staging}}" = "1" ]; then args="--test-cert"; fi; \
    emargs="--register-unsafely-without-email"; \
    if [ -n "${email}" ]; then emargs="-m ${email}"; fi; \
    # Domains: if CERTBOT_DOMAINS is set (space-separated), use it; otherwise default to base + www.base.\n    dargs=\"\"; \
    if [ -n \"${CERTBOT_DOMAINS:-}\" ]; then \
      for d in ${CERTBOT_DOMAINS}; do dargs=\"$dargs -d $d\"; done; \
    else \
      dargs=\"-d ${base} -d www.${base}\"; \
    fi; \
    hook="bash -ceu '\'' \
      dst=\"'\"'\"${dst_dir}\"'\"'\"\"; \
      svc=\"'\"'\"${svc}\"'\"'\"\"; \
      install -d -o slopmud -g slopmud -m 0750 \"${dst}\"; \
      install -o slopmud -g slopmud -m 0640 \"${RENEWED_LINEAGE}/fullchain.pem\" \"${dst}/fullchain.pem\"; \
      install -o slopmud -g slopmud -m 0640 \"${RENEWED_LINEAGE}/privkey.pem\" \"${dst}/privkey.pem\"; \
      systemctl restart \"${svc}\" 2>/dev/null || true; \
    '\''"; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      set -euo pipefail; \
      sudo certbot certonly --dns-route53 \
        ${dargs} \
        --non-interactive --agree-tos \
        ${emargs} \
        --cert-name \"${cert_name}\" \
        --deploy-hook \"${hook}\" \
        --keep-until-expiring ${args}; \
      sudo systemctl enable --now certbot.timer 2>/dev/null || true; \
      sudo certbot certificates | sed -n \"1,200p\" || true; \
    "; \
  '

# Install an env-scoped certbot deploy hook that copies renewed certs into TLS_DST_DIR
# (readable by the slopmud user) and restarts WEB_SERVICE_NAME.
#
# The hook checks CERTBOT_CERT_NAME (or DOMAIN) against RENEWED_LINEAGE, so multiple
# envs/certs can coexist on one host without commingling.
certbot-hook-install env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    dst_dir="${TLS_DST_DIR:-/etc/slopmud/tls}"; \
    svc="${WEB_SERVICE_NAME:-slopmud-web}"; \
    cert_name="${CERTBOT_CERT_NAME:-${DOMAIN}}"; \
    hook_name="slopmud-${ENV_NAME:-{{env}}}.sh"; \
    tmp_hook="$(mktemp)"; \
    trap "rm -f \"${tmp_hook}\"" EXIT; \
    { \
      printf "%s\\n" "#!/usr/bin/env bash"; \
      printf "%s\\n" "set -euo pipefail"; \
      printf "\\n"; \
      printf "%s\\n" ": \"\\${RENEWED_LINEAGE:?missing RENEWED_LINEAGE}\""; \
      printf "\\n"; \
      printf "%s\\n" "expected=\"${cert_name}\""; \
      printf "%s\\n" "if [[ \"\\$(basename \"\\${RENEWED_LINEAGE}\")\" != \"\\${expected}\" ]]; then"; \
      printf "%s\\n" "  exit 0"; \
      printf "%s\\n" "fi"; \
      printf "\\n"; \
      printf "%s\\n" "dst_dir=\"${dst_dir}\""; \
      printf "%s\\n" "svc=\"${svc}\""; \
      printf "\\n"; \
      printf "%s\\n" "install -d -o slopmud -g slopmud -m 0750 \"\\${dst_dir}\""; \
      printf "%s\\n" "install -o slopmud -g slopmud -m 0640 \"\\${RENEWED_LINEAGE}/fullchain.pem\" \"\\${dst_dir}/fullchain.pem\""; \
      printf "%s\\n" "install -o slopmud -g slopmud -m 0640 \"\\${RENEWED_LINEAGE}/privkey.pem\" \"\\${dst_dir}/privkey.pem\""; \
      printf "\\n"; \
      printf "%s\\n" "systemctl restart \"\\${svc}\" 2>/dev/null || true"; \
    } >"${tmp_hook}"; \
    scp -P "${SSH_PORT}" {{ssh_opts}} "${tmp_hook}" "${SSH_USER}@${HOST}:/tmp/${hook_name}"; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      set -euo pipefail; \
      if ! id -u slopmud >/dev/null 2>&1; then \
        sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
      fi; \
      sudo install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy; \
      sudo install -m 0755 -o root -g root \"/tmp/${hook_name}\" \"/etc/letsencrypt/renewal-hooks/deploy/${hook_name}\"; \
      sudo rm -f \"/tmp/${hook_name}\"; \
      sudo rm -f /etc/letsencrypt/renewal-hooks/deploy/slopmud.sh 2>/dev/null || true; \
    "; \
  '

# Copy current cert material into /etc/slopmud/tls (same logic as the renew hook).
certbot-tls-sync env="prd" domain="":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    base="{{domain}}"; \
    if [ -z "${base}" ]; then base="${DOMAIN}"; fi; \
    dst_dir="${TLS_DST_DIR:-/etc/slopmud/tls}"; \
    svc="${WEB_SERVICE_NAME:-slopmud-web}"; \
    cert_name="${CERTBOT_CERT_NAME:-${base}}"; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      set -euo pipefail; \
      if ! id -u slopmud >/dev/null 2>&1; then \
        sudo useradd --system --home \"${REMOTE_ROOT}\" --create-home --shell /usr/sbin/nologin slopmud; \
      fi; \
      sudo test -r \"/etc/letsencrypt/live/${cert_name}/fullchain.pem\"; \
      sudo test -r \"/etc/letsencrypt/live/${cert_name}/privkey.pem\"; \
      sudo install -d -o slopmud -g slopmud -m 0750 \"${dst_dir}\"; \
      sudo install -o slopmud -g slopmud -m 0640 \"/etc/letsencrypt/live/${cert_name}/fullchain.pem\" \"${dst_dir}/fullchain.pem\"; \
      sudo install -o slopmud -g slopmud -m 0640 \"/etc/letsencrypt/live/${cert_name}/privkey.pem\" \"${dst_dir}/privkey.pem\"; \
      sudo systemctl restart \"${svc}\" 2>/dev/null || true; \
    "; \
  '

certbot-renew env="prd" dry_run="0":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    just certbot-renew-host "${HOST}" "${SSH_USER}" "${SSH_PORT}" "{{dry_run}}"; \
  '

certbot-renew-host host ssh_user="admin" ssh_port="22" dry_run="0":
  ssh {{ssh_opts}} -p {{ssh_port}} {{ssh_user}}@{{host}} ' \
    if [ "{{dry_run}}" = "1" ]; then \
      sudo certbot renew --dry-run; \
    else \
      sudo certbot renew; \
    fi; \
    # Individual certs may have per-cert deploy hooks that restart the right service.\n    true; \
  '

certbot-status env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    just certbot-status-host "${HOST}" "${SSH_USER}" "${SSH_PORT}"; \
  '

certbot-status-host host ssh_user="admin" ssh_port="22":
  ssh {{ssh_opts}} -p {{ssh_port}} {{ssh_user}}@{{host}} ' \
    sudo certbot certificates || true; \
    (systemctl status certbot.timer --no-pager || true); \
  '

# Common typos
cerbot-install env="prd":
  just certbot-install {{env}}

cerbot-renew env="prd" dry_run="0":
  just certbot-renew {{env}} {{dry_run}}

# End-to-end: install certbot, issue slopmud.com + www, sync certs, deploy HTTPS-enabled web, and verify.
https-setup email env="prd":
  just certbot-install {{env}}
  just certbot-issue-web {{email}} {{env}}
  just certbot-tls-sync {{env}}
  just deploy {{env}}
  just https-smoke {{env}}

https-smoke env="prd" domain="":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    base="{{domain}}"; \
    if [ -z "${base}" ]; then base="${DOMAIN}"; fi; \
    port="${HTTPS_BIND##*:}"; \
    # Allow overriding via domain arg that already contains :port.\n    if [[ "${base}" == *:* ]]; then port=""; fi; \
    hp=""; if [[ -n "${port}" && "${port}" != "443" ]]; then hp=":${port}"; fi; \
    curl -fsS "https://${base}${hp}/healthz" | sed -n "1p"; \
    curl -fsS "https://www.${base}${hp}/healthz" | sed -n "1p"; \
  '

# --- Local dev: static web server (serves ./web_homepage by default) ---

web-build:
  cargo build -p static_web

web-run:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p static_web -- --bind 0.0.0.0:4943 --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

web-run-local:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p static_web -- --bind 127.0.0.1:4943 --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

# --- Local dev: web server with auth endpoints (Google SSO) ---

web-sso-build:
  cargo build -p slopmud_web

web-sso-run:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p slopmud_web -- --bind 0.0.0.0:4942 --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

web-sso-run-local:
  # Matches the common local Google redirect: http://localhost:4942/auth/google/callback
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p slopmud_web -- --bind 127.0.0.1:4942 --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

# --- Local dev: multi-agent helpers (git worktree + per-agent env/ports) ---

# Create/update an env file with an isolated port block for an agent.
agent-env NAME="" BASE_PORT="5040" OUT="env/agent.env":
  bash -ceu ' \
    if [ -z "{{NAME}}" ]; then echo "missing: NAME"; exit 2; fi; \
    ./scripts/mk_agent_env.sh "{{NAME}}" "{{BASE_PORT}}" "{{OUT}}"; \
  '

# One-shot: create a dedicated git worktree + write env/agent.env inside it.
agent-worktree dir name base_port="5040" branch="":
  bash -ceu ' \
    if [ -z "{{dir}}" ] || [ -z "{{name}}" ]; then echo "usage: just agent-worktree /tmp/a a 5040"; exit 2; fi; \
    br="{{branch}}"; if [ -z "$br" ]; then br="agent-{{name}}"; fi; \
    git worktree add "{{dir}}" -b "$br"; \
    chmod +x ./scripts/mk_agent_env.sh; \
    ./scripts/mk_agent_env.sh "{{name}}" "{{base_port}}" "{{dir}}/env/agent.env"; \
  '

# Smoke test: start N local agent stacks concurrently and verify their port ranges do not collide.
# Positional args: `just agent-ports-smoke 4940 100 4`
agent-ports-smoke base_port="4940" step="100" n="4":
  bash -ceu ' \
    ./scripts/agent_ports_smoke.sh "{{base_port}}" "{{step}}" "{{n}}"; \
  '

# Run the reference combat suite against a local shard + ws_gateway, using an env file for ports.
combat-refs env_file="env/dev.env" suite="reference/combats/suite.json":
  bash -ceu ' \
    ./scripts/combat_refs_smoke.sh "{{env_file}}" "{{suite}}"; \
  '

# Local admin JSON helper (password resets / admin caps) against the running broker.
adminctl env_file="env/dev.env" cmd="list-accounts":
  bash -ceu ' \
    set -a; source "{{env_file}}"; set +a; \
    export SLOPMUD_ADMIN_ADDR="${SLOPMUD_ADMIN_BIND}"; \
    cargo run -p slopmud_adminctl -- {{cmd}}; \
  '

# Run shard + broker for an arbitrary env file.
local-run env_file="env/dev.env":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "{{env_file}}"; set +a; \
    cargo run -p shard_01 & \
    shard_pid=$!; \
    trap "kill $shard_pid 2>/dev/null || true" EXIT; \
    cargo run -p slopmud; \
  '

# Run static_web for an arbitrary env file (web UI + /ws -> broker).
local-web env_file="env/dev.env":
  bash -ceu ' \
    set -a; source "{{env_file}}"; set +a; \
    if [ -z "${STATIC_WEB_BIND:-}" ]; then \
      p="${SLOPMUD_BIND##*:}"; \
      STATIC_WEB_BIND="127.0.0.1:$((p + 3))"; \
    fi; \
    cargo run -p static_web -- --bind "${STATIC_WEB_BIND}" --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

# Run slopmud_web for an arbitrary env file (auth endpoints + oauth callback + /ws -> broker).
local-web-sso env_file="env/dev.env":
  bash -ceu ' \
    set -a; source "{{env_file}}"; set +a; \
    if [ -z "${OAUTH_WEB_BIND:-}" ]; then \
      p="${SLOPMUD_BIND##*:}"; \
      OAUTH_WEB_BIND="127.0.0.1:$((p + 2))"; \
    fi; \
    cargo run -p slopmud_web -- --bind "${OAUTH_WEB_BIND}" --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

# Run shard + broker in background, and static_web in the foreground (Ctrl+C to stop).
local-all env_file="env/dev.env":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "{{env_file}}"; set +a; \
    if [ -z "${STATIC_WEB_BIND:-}" ]; then \
      p="${SLOPMUD_BIND##*:}"; \
      STATIC_WEB_BIND="127.0.0.1:$((p + 3))"; \
    fi; \
    cargo run -p shard_01 & shard_pid=$!; \
    trap "kill $shard_pid 2>/dev/null || true" EXIT; \
    sleep 0.6; \
    cargo run -p slopmud & broker_pid=$!; \
    trap "kill $broker_pid 2>/dev/null || true; kill $shard_pid 2>/dev/null || true" EXIT; \
    sleep 0.6; \
    cargo run -p static_web -- --bind "${STATIC_WEB_BIND}" --dir web_homepage --session-tcp-addr "${SESSION_TCP_ADDR}"; \
  '

# --- Local dev: slopmud server ---

slopmud-build:
  cargo build -p slopmud

slopmud-run:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p slopmud; \
  '

ws-build:
  cargo build -p ws_gateway

ws-run-local:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    WS_BIND=127.0.0.1:4100 cargo run -p ws_gateway; \
  '

bot-party-run-local:
  WS_URL=ws://127.0.0.1:4100/v1/json BOTS=2 cargo run -p bot_party

e2e-local:
  python3 scripts/e2e_local.py

e2e-party:
  python3 scripts/e2e_party_run.py

e2e-web-local:
  python3 scripts/e2e_web_selenium_local.py

e2e-web-sso-rbac-local:
  python3 scripts/e2e_web_sso_rbac_selenium_local.py

# Proves internal_oidc registration (/register with password twice) can complete a slopmud_web OIDC flow.
e2e-web-slopsso-register-local:
  python3 scripts/e2e_web_slopsso_register_local.py

# Selenium: click gate->SlopSSO redirect, register (password twice) in IdP, return, create via SSO, connect.
e2e-web-slopsso-register-selenium-local:
  python3 scripts/e2e_web_slopsso_register_selenium_local.py

# Local browser demo: slopmud_web + internal_oidc ("slopsso") on a random free 52xx port block.
local-slopsso-demo:
  ./scripts/local_slopsso_demo.sh

proto-lint:
  python3 scripts/protoadventure_lint.py

proto-coverage:
  python3 scripts/proto_coverage.py

# --- World authoring: overworld + zone shapes ---

overworld-export:
  python3 scripts/overworld_export.py

overworld-validate:
  python3 scripts/overworld_validate.py

zones-stubgen:
  python3 scripts/zone_shape_stubgen.py

zones-annotate-proto zone_id="":
  bash -ceu ' \
    if [ -n "{{zone_id}}" ]; then \
      python3 scripts/zones_annotate_proto.py "{{zone_id}}"; \
    else \
      python3 scripts/zones_annotate_proto.py; \
    fi; \
  '

areas-validate:
  python3 scripts/areas_validate.py

area-files-validate:
  python3 scripts/area_files_validate.py

area-files-report zone_id="" fmt="md":
  bash -ceu ' \
    args=(--format "{{fmt}}"); \
    if [ -n "{{zone_id}}" ]; then args+=(--zone-id "{{zone_id}}"); fi; \
    python3 scripts/area_files_report.py "${args[@]}"; \
  '

area-files-stubgen zone_id="":
  bash -ceu ' \
    if [ -n "{{zone_id}}" ]; then \
      python3 scripts/area_files_stubgen.py --zone-id "{{zone_id}}"; \
    else \
      python3 scripts/area_files_stubgen.py --all; \
    fi; \
  '

area-files-budgetfill zone_id="":
  bash -ceu ' \
    if [ -n "{{zone_id}}" ]; then \
      python3 scripts/area_files_budgetfill.py --zone-id "{{zone_id}}"; \
    else \
      python3 scripts/area_files_budgetfill.py --all; \
    fi; \
  '

area-files-themify zone_id="":
  bash -ceu ' \
    if [ -n "{{zone_id}}" ]; then \
      python3 scripts/area_files_themify.py --zone-id "{{zone_id}}"; \
    else \
      python3 scripts/area_files_themify.py --all; \
    fi; \
  '

world-validate:
  just overworld-validate
  just areas-validate
  just area-files-validate

area-lock zone_id claimed_by note="":
  python3 scripts/area_lock.py lock "{{zone_id}}" --by "{{claimed_by}}" --note "{{note}}"

area-unlock zone_id claimed_by force="0":
  bash -ceu ' \
    args=(unlock "{{zone_id}}" --by "{{claimed_by}}"); \
    if [ "{{force}}" = "1" ]; then args+=(--force); fi; \
    python3 scripts/area_lock.py "${args[@]}"; \
  '

area-lock-status zone_id="":
  bash -ceu ' \
    if [ -n "{{zone_id}}" ]; then \
      python3 scripts/area_lock.py status "{{zone_id}}"; \
    else \
      python3 scripts/area_lock.py status; \
    fi; \
  '

e2e-ws:
  RUSTFLAGS='-D warnings' cargo build -q -p shard_01 -p ws_gateway -p bot_party
  RUSTFLAGS='-D warnings' cargo build -q -p ws_gateway --bin e2e_ws
  ./target/debug/e2e_ws

# --- Local dev: shard ---

shard-build:
  cargo build -p shard_01

shard-run:
  bash -ceu ' \
    set -a; source env/dev.env; set +a; \
    cargo run -p shard_01; \
  '

# Run shard_01 in the background and the session broker in the foreground.
dev-run:
  bash -ceu ' \
    set -o pipefail; \
    set -a; source env/dev.env; set +a; \
    cargo run -p shard_01 & \
    shard_pid=$!; \
    trap "kill $shard_pid 2>/dev/null || true" EXIT; \
    cargo run -p slopmud; \
  '

# --- Deploy: static_web + web_homepage to a host (prd/stg/dev env files) ---

deploy env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    bin="$(basename "${REMOTE_BIN}")"; \
    if [ "${bin}" = "slopmud_web" ]; then \
      ./scripts/deploy_slopmud_web.sh "env/{{env}}.env"; \
    else \
      ./scripts/deploy_static_web.sh "env/{{env}}.env"; \
    fi; \
  '

# Deploy web with OAuth endpoints (slopmud_web). Uses the same env file keys as deploy_static_web.sh.
deploy-web-sso env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_slopmud_web.sh "env/{{env}}.env"; \
  '

web-install env="prd":
  just deploy {{env}}

web-restart env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" "sudo systemctl restart slopmud-web"; \
  '

web-logs env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" "sudo journalctl -u slopmud-web -f --no-pager"; \
  '

deploy-status env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} ${SSH_USER}@${HOST} -p ${SSH_PORT} " \
      sudo systemctl status slopmud-web --no-pager || true; \
      echo; \
      sudo ss -lntp | sed -n \"1,12p\"; \
      echo; \
      sudo ss -lntp | grep -n \":80\\b\" || true; \
      echo; \
      sudo journalctl -u slopmud-web -n 50 --no-pager || true; \
    "; \
  '

# Legacy nginx-based deploy (kept around for reference).
deploy-nginx env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_homepage.sh "env/{{env}}.env"; \
  '

# --- Deploy: slopmud service (dev/prd) ---

deploy-slopmud env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_slopmud.sh "env/{{env}}.env"; \
  '

deploy-shard env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_shard_01.sh "env/{{env}}.env"; \
  '

deploy-slopmud-all:
  just deploy-slopmud dev
  just deploy-slopmud prd

# --- Deploy: SBC (raft/enforcer/metrics/decider) ---

deploy-sbc env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_sbc.sh "env/{{env}}.env"; \
  '

sbc-status env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      sudo systemctl status sbc-raftd --no-pager || true; \
      echo; \
      sudo systemctl status sbc-metricsd --no-pager || true; \
      echo; \
      sudo systemctl status sbc-enforcerd --no-pager || true; \
      echo; \
      curl -fsSL http://127.0.0.1:9911/status | sed -n \"1,200p\" || true; \
    "; \
  '

sbc-logs env="prd" unit="sbc-enforcerd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" "sudo journalctl -u {{unit}} -f --no-pager"; \
  '

# --- Deploy: internal_oidc service (prd) ---

deploy-oidc env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    if [ "${ENABLED:-1}" != "1" ]; then echo "{{env}} disabled (ENABLED=${ENABLED:-})"; exit 0; fi; \
    ./scripts/deploy_internal_oidc.sh "env/{{env}}.env"; \
  '

deploy-slopmud-status env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} ${SSH_USER}@${HOST} -p ${SSH_PORT} " \
      sudo systemctl status ${SLOPMUD_APP_NAME}.service --no-pager || true; \
      echo; \
      sudo ss -lntp | grep -n \":${SLOPMUD_BIND##*:}\\\\b\" || true; \
      echo; \
      sudo journalctl -u ${SLOPMUD_APP_NAME}.service -n 80 --no-pager || true; \
    "; \
  '

# --- GitHub Actions: self-hosted runner (remote) ---
#
# Registers a runner on the target host. You must provide a short-lived registration token.
# Example:
#   just gha-runner-install https://github.com/<owner>/<repo> <token> prd
gha-runner-install url token env="prd" name="" labels="" runner_dir="/opt/actions-runner" runner_user="ghrunner":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    just gha-runner-install-host "${HOST}" "{{env}}" "{{url}}" "{{token}}" "{{name}}" "{{labels}}" "${SSH_USER}" "${SSH_PORT}" "{{runner_dir}}" "{{runner_user}}"; \
  '

gha-runner-install-host host env_name url token name labels ssh_user="admin" ssh_port="22" runner_dir="/opt/actions-runner" runner_user="ghrunner":
  ssh {{ssh_opts}} -p {{ssh_port}} {{ssh_user}}@{{host}} " \
    set -euo pipefail; \
    env_name='{{env_name}}'; \
    url='{{url}}'; \
    token='{{token}}'; \
    name='{{name}}'; \
    labels='{{labels}}'; \
    runner_dir='{{runner_dir}}'; \
    runner_user='{{runner_user}}'; \
    if [ -z \"\$name\" ]; then name=\"slopmud-\${env_name}-\$(hostname)\"; fi; \
    if ! command -v sudo >/dev/null 2>&1; then echo 'sudo is required'; exit 2; fi; \
    if ! command -v curl >/dev/null 2>&1; then \
      if command -v apt-get >/dev/null 2>&1; then sudo apt-get update && sudo apt-get install -y curl ca-certificates; \
      elif command -v dnf >/dev/null 2>&1; then sudo dnf -y install curl ca-certificates; \
      else echo 'Need curl (apt-get or dnf)'; exit 2; fi; \
    fi; \
    if ! command -v tar >/dev/null 2>&1; then \
      if command -v apt-get >/dev/null 2>&1; then sudo apt-get update && sudo apt-get install -y tar; \
      elif command -v dnf >/dev/null 2>&1; then sudo dnf -y install tar; \
      else echo 'Need tar (apt-get or dnf)'; exit 2; fi; \
    fi; \
    if ! command -v python3 >/dev/null 2>&1; then \
      if command -v apt-get >/dev/null 2>&1; then sudo apt-get update && sudo apt-get install -y python3; \
      elif command -v dnf >/dev/null 2>&1; then sudo dnf -y install python3; \
      else echo 'Need python3 (apt-get or dnf)'; exit 2; fi; \
    fi; \
    arch=\$(uname -m); \
    case \"\$arch\" in \
      x86_64) platform='linux-x64' ;; \
      aarch64|arm64) platform='linux-arm64' ;; \
      *) echo \"Unsupported arch: \$arch\"; exit 2 ;; \
    esac; \
    if ! id -u \"\$runner_user\" >/dev/null 2>&1; then \
      sudo useradd --system --home \"\$runner_dir\" --create-home --shell /usr/sbin/nologin \"\$runner_user\"; \
    fi; \
    sudo install -d -m 0755 \"\$runner_dir\"; \
    sudo chown -R \"\$runner_user:\$runner_user\" \"\$runner_dir\"; \
    if [ ! -x \"\$runner_dir/config.sh\" ]; then \
      dl_url=\$(GHA_PLATFORM=\"\$platform\" python3 -c 'import json,os,urllib.request; platform=os.environ[\"GHA_PLATFORM\"]; req=urllib.request.Request(\"https://api.github.com/repos/actions/runner/releases/latest\", headers={\"User-Agent\":\"slopmud-gha-runner-bootstrap\"}); rel=json.load(urllib.request.urlopen(req, timeout=30)); needle=f\"actions-runner-{platform}-\"; urls=[a.get(\"browser_download_url\",\"\") for a in rel.get(\"assets\",[])]; match=[u for u in urls if needle in u and u.endswith(\".tar.gz\")][0]; print(match)'); \
      sudo -u \"\$runner_user\" -H bash -lc \" \
        set -euo pipefail; \
        cd '\$runner_dir'; \
        curl -fsSL '\$dl_url' -o runner.tgz; \
        tar xzf runner.tgz; \
        rm -f runner.tgz; \
      \"; \
    fi; \
    sudo -u \"\$runner_user\" -H env \
      GHA_RUNNER_DIR=\"\$runner_dir\" \
      GHA_URL=\"\$url\" GHA_TOKEN=\"\$token\" GHA_NAME=\"\$name\" GHA_LABELS=\"\$labels\" \
      bash -lc ' \
        set -euo pipefail; \
        cd \"\$GHA_RUNNER_DIR\"; \
        cmd=(./config.sh --unattended --replace --url \"\$GHA_URL\" --token \"\$GHA_TOKEN\" --name \"\$GHA_NAME\" --work _work); \
        if [ -n \"\$GHA_LABELS\" ]; then cmd+=(--labels \"\$GHA_LABELS\"); fi; \
        \"\${cmd[@]}\"; \
      '; \
    (cd \"\$runner_dir\" && sudo ./svc.sh install \"\$runner_user\" >/dev/null 2>&1) || true; \
    (cd \"\$runner_dir\" && sudo ./svc.sh start) || true; \
    (cd \"\$runner_dir\" && sudo ./svc.sh status) || true; \
  "

gha-runner-status env="prd" runner_dir="/opt/actions-runner":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      set -euo pipefail; \
      cd {{runner_dir}} 2>/dev/null || { echo \"missing runner dir: {{runner_dir}}\"; exit 2; }; \
      sudo ./svc.sh status || true; \
      echo; \
      systemctl list-units \"actions.runner*\" --no-pager || true; \
    "; \
  '

gha-runner-logs env="prd":
  bash -ceu ' \
    set -o pipefail; \
    set -a; source "env/{{env}}.env"; set +a; \
    ssh {{ssh_opts}} -p "${SSH_PORT}" "${SSH_USER}@${HOST}" " \
      set -euo pipefail; \
      systemctl list-units \"actions.runner*\" --no-pager || true; \
      echo; \
      sudo journalctl -u \"actions.runner*\" -n 200 --no-pager || true; \
    "; \
  '

# Mint a short-lived GitHub Actions runner registration token for a repo.
# Requires either:
# - `gh auth login` (preferred), or
# - `GITHUB_TOKEN` with `admin:repo_hook`/`repo` + `actions:write` for the repo.
gha-runner-token repo:
  bash -ceu ' \
    set -o pipefail; \
    if command -v gh >/dev/null 2>&1; then \
      gh api -X POST "repos/{{repo}}/actions/runners/registration-token" --jq ".token"; \
    else \
      : "${GITHUB_TOKEN:?set GITHUB_TOKEN or install/authenticate gh}"; \
      curl -fsSL -X POST \
        -H "Authorization: Bearer ${GITHUB_TOKEN}" \
        -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/{{repo}}/actions/runners/registration-token" \
        | python3 -c "import json,sys; print(json.load(sys.stdin)[\"token\"])"; \
    fi; \
  '

# Convenience: install/register a runner for a repo by minting a token locally.
gha-runner-install-repo repo env="prd" name="" labels="" runner_dir="/opt/actions-runner" runner_user="ghrunner":
  bash -ceu ' \
    set -o pipefail; \
    token="$(just gha-runner-token {{repo}})"; \
    just gha-runner-install "https://github.com/{{repo}}" "${token}" {{env}} "{{name}}" "{{labels}}" "{{runner_dir}}" "{{runner_user}}"; \
  '
