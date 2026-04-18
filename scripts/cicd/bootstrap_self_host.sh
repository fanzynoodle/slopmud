#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  bootstrap_self_host.sh [env] [github_repo] [github_token_ssm] [runner_labels]

Bootstraps a fresh host locally from a checked-out slopmud repo:
- installs build/runtime dependencies
- installs rust for the current user
- bootstraps the GitHub Actions runner host + registers a runner when token access is configured
- builds and installs shard, broker, internal_oidc, landing web, and OAuth web services

Arguments:
  env                Deployment env/track. Default: prd
  github_repo        Repo in owner/name form for the self-hosted runner. Default: empty (skip runner registration)
  github_token_ssm   Optional SSM parameter name holding a GitHub token that can mint runner registration tokens.
  runner_labels      Optional custom runner labels. Default: mud
EOF
}

env_name="${1:-prd}"
github_repo="${2:-}"
github_token_ssm="${3:-}"
runner_labels="${4:-mud}"

if [[ "${env_name}" == "-h" || "${env_name}" == "--help" ]]; then
  usage
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

run_as_current_user() {
  local target_user home_dir
  if [[ "${SUDO_USER:-}" != "" && "${SUDO_USER}" != "root" ]]; then
    target_user="${SUDO_USER}"
  else
    target_user="${USER:-root}"
  fi

  home_dir="$(getent passwd "${target_user}" | cut -d: -f6)"
  if [[ -z "${home_dir}" ]]; then
    echo "ERROR: could not determine home directory for ${target_user}" >&2
    exit 2
  fi

  if [[ "${target_user}" == "${USER:-root}" ]]; then
    HOME="${home_dir}" bash -lc "$1"
  else
    sudo -u "${target_user}" -H env HOME="${home_dir}" bash -lc "$1"
  fi
}

activate_rust_toolchain() {
  local target_user home_dir
  if [[ "${SUDO_USER:-}" != "" && "${SUDO_USER}" != "root" ]]; then
    target_user="${SUDO_USER}"
  else
    target_user="${USER:-root}"
  fi

  home_dir="$(getent passwd "${target_user}" | cut -d: -f6)"
  if [[ -z "${home_dir}" || ! -x "${home_dir}/.cargo/bin/cargo" ]]; then
    return 0
  fi

  export HOME="${home_dir}"
  export CARGO_HOME="${home_dir}/.cargo"
  export RUSTUP_HOME="${home_dir}/.rustup"
  export PATH="${CARGO_HOME}/bin:${PATH}"
}

install_system_packages() {
  echo "Installing host bootstrap packages"
  if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update -y
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
      awscli \
      build-essential \
      ca-certificates \
      curl \
      git \
      jq \
      libssl-dev \
      pkg-config \
      python3 \
      ripgrep \
      rsync
  elif command -v dnf >/dev/null 2>&1; then
    sudo dnf -y install \
      awscli \
      ca-certificates \
      curl \
      gcc \
      gcc-c++ \
      git \
      jq \
      make \
      openssl-devel \
      pkgconf-pkg-config \
      python3 \
      ripgrep \
      rsync \
      tar
  else
    echo "ERROR: unsupported OS (need apt-get or dnf)" >&2
    exit 2
  fi
}

ensure_rust_toolchain() {
  echo "Ensuring Rust toolchain for local builds"
  run_as_current_user '
    set -euo pipefail
    if [[ ! -x "$HOME/.cargo/bin/rustup" ]]; then
      curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    fi
    source "$HOME/.cargo/env"
    rustup toolchain install stable --profile minimal
    rustup default stable
    cargo --version
    rustc --version
  '
  activate_rust_toolchain
}

wait_for_listen() {
  local port="$1"
  local label="$2"
  local i
  for i in $(seq 1 40); do
    if sudo ss -lnt | grep -qE ":${port}\\b"; then
      return 0
    fi
    sleep 0.25
  done
  echo "ERROR: ${label} is not listening on port ${port}" >&2
  sudo ss -lntp || true
  return 1
}

ensure_slopmud_layout() {
  local remote_root="$1"
  local remote_bin_dir="$2"
  shift 2

  if ! id -u slopmud >/dev/null 2>&1; then
    echo "Creating system user: slopmud"
    sudo useradd --system --home "${remote_root}" --create-home --shell /usr/sbin/nologin slopmud
  fi

  sudo mkdir -p "${remote_root}" "${remote_bin_dir}"
  local path
  for path in "$@"; do
    [[ -n "${path}" ]] || continue
    sudo mkdir -p "${path}"
  done
  sudo chown -R slopmud:slopmud "${remote_root}"
}

ensure_parent_owned_by_slopmud() {
  local path="${1:-}"
  [[ -n "${path}" ]] || return 0
  sudo mkdir -p "$(dirname "${path}")"
  sudo chown -R slopmud:slopmud "$(dirname "${path}")"
}

ensure_dir_owned_by_slopmud() {
  local path="${1:-}"
  [[ -n "${path}" ]] || return 0
  sudo mkdir -p "${path}"
  sudo chown -R slopmud:slopmud "${path}"
}

resolve_tls() {
  local https_bind="${1:-}"
  local tls_cert="${2:-}"
  local tls_key="${3:-}"

  if [[ -z "${https_bind}" ]]; then
    printf '%s\n%s\n%s\n' "" "${tls_cert}" "${tls_key}"
    return 0
  fi

  if [[ -n "${tls_cert}" && -n "${tls_key}" && -f "${tls_cert}" && -f "${tls_key}" ]]; then
    printf '%s\n%s\n%s\n' "${https_bind}" "${tls_cert}" "${tls_key}"
    return 0
  fi

  echo "WARN: TLS files missing for ${https_bind}; starting HTTP-only until certs are present" >&2
  printf '%s\n%s\n%s\n' "" "" ""
}

install_github_runner_local() {
  local env_name="$1"
  local repo="$2"
  local token_ssm="$3"
  local labels="$4"

  if [[ -z "${repo}" ]]; then
    echo "Skipping GitHub runner registration: repo not configured"
    return 0
  fi

  local github_token="${GITHUB_TOKEN:-}"
  if [[ -z "${github_token}" && -n "${token_ssm}" ]]; then
    echo "Fetching GitHub runner bootstrap token from SSM: ${token_ssm}"
    github_token="$(
      aws ssm get-parameter \
        --region "${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}}" \
        --name "${token_ssm}" \
        --with-decryption \
        --query Parameter.Value \
        --output text
    )" || github_token=""
  fi

  if [[ -z "${github_token}" ]]; then
    echo "WARN: GitHub runner token is unavailable; skipping runner registration" >&2
    return 0
  fi

  ./scripts/cicd/bootstrap_runner.sh

  local runner_user="ghrunner"
  local runner_dir="/opt/actions-runner"
  local arch platform dl_url reg_token runner_name

  arch="$(uname -m)"
  case "${arch}" in
    x86_64) platform="linux-x64" ;;
    aarch64|arm64) platform="linux-arm64" ;;
    *)
      echo "ERROR: unsupported runner arch: ${arch}" >&2
      return 2
      ;;
  esac

  sudo install -d -m 0755 "${runner_dir}"
  sudo chown -R "${runner_user}:${runner_user}" "${runner_dir}"

  if [[ ! -x "${runner_dir}/config.sh" ]]; then
    dl_url="$(
      GHA_PLATFORM="${platform}" python3 - <<'PY'
import json
import os
import urllib.request

platform = os.environ["GHA_PLATFORM"]
req = urllib.request.Request(
    "https://api.github.com/repos/actions/runner/releases/latest",
    headers={"User-Agent": "slopmud-gha-runner-bootstrap"},
)
with urllib.request.urlopen(req, timeout=30) as resp:
    rel = json.load(resp)
needle = f"actions-runner-{platform}-"
for asset in rel.get("assets", []):
    url = asset.get("browser_download_url", "")
    if needle in url and url.endswith(".tar.gz"):
        print(url)
        break
else:
    raise SystemExit("no matching runner download URL found")
PY
    )"
    sudo -u "${runner_user}" -H bash -lc " \
      set -euo pipefail; \
      cd '${runner_dir}'; \
      curl -fsSL '${dl_url}' -o runner.tgz; \
      tar xzf runner.tgz; \
      rm -f runner.tgz; \
    "
  fi

  reg_token="$(
    curl -fsSL -X POST \
      -H "Authorization: Bearer ${github_token}" \
      -H "Accept: application/vnd.github+json" \
      "https://api.github.com/repos/${repo}/actions/runners/registration-token" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])'
  )"
  runner_name="slopmud-${env_name}-$(hostname)"

  sudo -u "${runner_user}" -H env \
    GHA_RUNNER_DIR="${runner_dir}" \
    GHA_URL="https://github.com/${repo}" \
    GHA_TOKEN="${reg_token}" \
    GHA_NAME="${runner_name}" \
    GHA_LABELS="${labels}" \
    bash -lc '
      set -euo pipefail
      cd "${GHA_RUNNER_DIR}"
      cmd=(./config.sh --unattended --replace --url "${GHA_URL}" --token "${GHA_TOKEN}" --name "${GHA_NAME}" --work _work)
      if [[ -n "${GHA_LABELS}" ]]; then
        cmd+=(--labels "${GHA_LABELS}")
      fi
      "${cmd[@]}"
    '

  (cd "${runner_dir}" && sudo ./svc.sh install "${runner_user}" >/dev/null 2>&1) || true
  (cd "${runner_dir}" && sudo ./svc.sh start) || true
  (cd "${runner_dir}" && sudo ./svc.sh status) || true
}

deploy_slopmud_local() {
  local env_file="$1"
  (
    set -euo pipefail
    set -a
    # shellcheck disable=SC1090
    source "${env_file}"
    set +a

    if [[ "${ENABLED:-1}" != "1" ]]; then
      echo "Skipping disabled env: ${env_file}"
      exit 0
    fi

    : "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
    : "${SLOPMUD_APP_NAME:?missing SLOPMUD_APP_NAME in env file}"
    : "${SLOPMUD_REMOTE_BIN:?missing SLOPMUD_REMOTE_BIN in env file}"
    : "${SLOPMUD_BIND:?missing SLOPMUD_BIND in env file}"
    : "${NODE_ID:?missing NODE_ID in env file}"

    local remote_bin_dir tmp_unit unit_name port
    remote_bin_dir="$(dirname "${SLOPMUD_REMOTE_BIN}")"

    ./scripts/build_bookworm_release.sh slopmud
    ensure_slopmud_layout "${REMOTE_ROOT}" "${remote_bin_dir}"
    ensure_parent_owned_by_slopmud "${SLOPMUD_ACCOUNTS_PATH:-}"
    ensure_parent_owned_by_slopmud "${SLOPMUD_BANS_PATH:-}"
    ensure_dir_owned_by_slopmud "${SLOPMUD_GOOGLE_OAUTH_DIR:-}"
    ensure_dir_owned_by_slopmud "${SLOPMUD_EVENTLOG_SPOOL_DIR:-}"

    sudo install -m 0755 -o root -g root target/release/slopmud "${SLOPMUD_REMOTE_BIN}"

    tmp_unit="$(mktemp)"
    trap 'rm -f "${tmp_unit}"' EXIT
    cat >"${tmp_unit}" <<EOF
[Unit]
Description=slopmud service (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=slopmud=info
Environment=NODE_ID=${NODE_ID}
Environment=SLOPMUD_BIND=${SLOPMUD_BIND}
EOF

    if [[ -n "${SHARD_ADDR:-}" ]]; then
      echo "Environment=SHARD_ADDR=${SHARD_ADDR}" >>"${tmp_unit}"
    elif [[ -n "${SHARD_BIND:-}" ]]; then
      echo "Environment=SHARD_ADDR=${SHARD_BIND}" >>"${tmp_unit}"
    fi

    if [[ -n "${SLOPMUD_OIDC_TOKEN_URL:-}" ]]; then
      {
        echo "Environment=SLOPMUD_OIDC_TOKEN_URL=${SLOPMUD_OIDC_TOKEN_URL}"
        [[ -n "${SLOPMUD_OIDC_CLIENT_ID:-}" ]] && echo "Environment=SLOPMUD_OIDC_CLIENT_ID=${SLOPMUD_OIDC_CLIENT_ID}"
        [[ -n "${SLOPMUD_OIDC_CLIENT_SECRET:-}" ]] && echo "Environment=SLOPMUD_OIDC_CLIENT_SECRET=${SLOPMUD_OIDC_CLIENT_SECRET}"
        [[ -n "${SLOPMUD_OIDC_SCOPE:-}" ]] && echo "Environment=SLOPMUD_OIDC_SCOPE=${SLOPMUD_OIDC_SCOPE}"
      } >>"${tmp_unit}"
    fi

    [[ -n "${SLOPMUD_GOOGLE_OAUTH_DIR:-}" ]] && echo "Environment=SLOPMUD_GOOGLE_OAUTH_DIR=${SLOPMUD_GOOGLE_OAUTH_DIR}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_GOOGLE_AUTH_BASE_URL:-}" ]] && echo "Environment=SLOPMUD_GOOGLE_AUTH_BASE_URL=${SLOPMUD_GOOGLE_AUTH_BASE_URL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_WEBAUTH_JWT_SECRET:-}" ]] && echo "Environment=SLOPMUD_WEBAUTH_JWT_SECRET=${SLOPMUD_WEBAUTH_JWT_SECRET}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_ACCOUNTS_PATH:-}" ]] && echo "Environment=SLOPMUD_ACCOUNTS_PATH=${SLOPMUD_ACCOUNTS_PATH}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_LOCALE:-}" ]] && echo "Environment=SLOPMUD_LOCALE=${SLOPMUD_LOCALE}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_ADMIN_BIND:-}" ]] && echo "Environment=SLOPMUD_ADMIN_BIND=${SLOPMUD_ADMIN_BIND}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_BANS_PATH:-}" ]] && echo "Environment=SLOPMUD_BANS_PATH=${SLOPMUD_BANS_PATH}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_ENABLED:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_ENABLED=${SLOPMUD_EVENTLOG_ENABLED}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_SPOOL_DIR:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_SPOOL_DIR=${SLOPMUD_EVENTLOG_SPOOL_DIR}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S=${SLOPMUD_EVENTLOG_FLUSH_INTERVAL_S}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_S3_BUCKET:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_S3_BUCKET=${SLOPMUD_EVENTLOG_S3_BUCKET}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_S3_PREFIX:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_S3_PREFIX=${SLOPMUD_EVENTLOG_S3_PREFIX}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_UPLOAD_ENABLED:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_UPLOAD_ENABLED=${SLOPMUD_EVENTLOG_UPLOAD_ENABLED}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL=${SLOPMUD_EVENTLOG_UPLOAD_DELETE_LOCAL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S=${SLOPMUD_EVENTLOG_UPLOAD_SCAN_INTERVAL_S}" >>"${tmp_unit}"

    cat >>"${tmp_unit}" <<EOF
ExecStart=${SLOPMUD_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

    unit_name="${SLOPMUD_APP_NAME}.service"
    sudo install -m 0644 "${tmp_unit}" "/etc/systemd/system/${unit_name}"
    sudo systemctl daemon-reload
    sudo systemctl enable --now "${unit_name}"
    sudo systemctl restart "${unit_name}"

    port="${SLOPMUD_BIND##*:}"
    wait_for_listen "${port}" "${unit_name}"
  )
}

deploy_shard_local() {
  local env_file="$1"
  (
    set -euo pipefail
    set -a
    # shellcheck disable=SC1090
    source "${env_file}"
    set +a

    if [[ "${ENABLED:-1}" != "1" ]]; then
      echo "Skipping disabled env: ${env_file}"
      exit 0
    fi

    : "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
    : "${SHARD_APP_NAME:?missing SHARD_APP_NAME in env file}"
    : "${SHARD_REMOTE_BIN:?missing SHARD_REMOTE_BIN in env file}"
    : "${SHARD_BIND:?missing SHARD_BIND in env file}"

    local remote_bin_dir tmp_unit unit_name exec_start port
    remote_bin_dir="$(dirname "${SHARD_REMOTE_BIN}")"

    ./scripts/build_bookworm_release.sh shard_01
    ensure_slopmud_layout "${REMOTE_ROOT}" "${remote_bin_dir}"
    sudo install -m 0755 -o root -g root target/release/shard_01 "${SHARD_REMOTE_BIN}"

    tmp_unit="$(mktemp)"
    trap 'rm -f "${tmp_unit}"' EXIT
    cat >"${tmp_unit}" <<EOF
[Unit]
Description=slopmud shard_01 (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=shard_01=info
Environment=SHARD_BIND=${SHARD_BIND}
EOF

    [[ -n "${OPENAI_API_BASE:-}" ]] && echo "Environment=OPENAI_API_BASE=${OPENAI_API_BASE}" >>"${tmp_unit}"
    [[ -n "${OPENAI_PING_MODEL:-}" ]] && echo "Environment=OPENAI_PING_MODEL=${OPENAI_PING_MODEL}" >>"${tmp_unit}"
    [[ -n "${OPENAI_API_KEY_SSM:-}" ]] && echo "Environment=OPENAI_API_KEY_SSM=${OPENAI_API_KEY_SSM}" >>"${tmp_unit}"

    exec_start="${SHARD_REMOTE_BIN}"
    if [[ -n "${OPENAI_API_KEY_SSM:-}" ]]; then
      exec_start="/bin/bash -ceu ' \
        export OPENAI_API_KEY=\"\$(aws ssm get-parameter --region ${AWS_REGION:-${AWS_DEFAULT_REGION:-us-east-1}} --name \"\${OPENAI_API_KEY_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
        exec \"${SHARD_REMOTE_BIN}\"; \
      '"
    fi

    cat >>"${tmp_unit}" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

    unit_name="${SHARD_APP_NAME}.service"
    sudo install -m 0644 "${tmp_unit}" "/etc/systemd/system/${unit_name}"
    sudo systemctl daemon-reload
    sudo systemctl enable --now "${unit_name}"
    sudo systemctl restart "${unit_name}"

    port="${SHARD_BIND##*:}"
    wait_for_listen "${port}" "${unit_name}"
  )
}

deploy_internal_oidc_local() {
  local env_file="$1"
  (
    set -euo pipefail
    set -a
    # shellcheck disable=SC1090
    source "${env_file}"
    set +a

    if [[ "${ENABLED:-1}" != "1" ]]; then
      echo "Skipping disabled env: ${env_file}"
      exit 0
    fi

    if [[ -z "${OIDC_APP_NAME:-}" ]]; then
      echo "Skipping OIDC deploy for ${env_file}: no OIDC_APP_NAME"
      exit 0
    fi

    : "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
    : "${OIDC_REMOTE_BIN:?missing OIDC_REMOTE_BIN in env file}"
    : "${OIDC_BIND:?missing OIDC_BIND in env file}"
    : "${OIDC_ISSUER:?missing OIDC_ISSUER in env file}"
    : "${OIDC_CLIENT_ID:?missing OIDC_CLIENT_ID in env file}"
    : "${OIDC_CLIENT_SECRET:?missing OIDC_CLIENT_SECRET in env file}"
    : "${OIDC_ED25519_SEED_B64:?missing OIDC_ED25519_SEED_B64 in env file}"

    local remote_bin_dir tmp_unit unit_name port tls_bits oidc_https_bind oidc_tls_cert oidc_tls_key
    remote_bin_dir="$(dirname "${OIDC_REMOTE_BIN}")"

    ./scripts/build_bookworm_release.sh internal_oidc
    ensure_slopmud_layout "${REMOTE_ROOT}" "${remote_bin_dir}"
    ensure_parent_owned_by_slopmud "${OIDC_USERS_PATH:-}"
    sudo install -m 0755 -o root -g root target/release/internal_oidc "${OIDC_REMOTE_BIN}"

    mapfile -t tls_bits < <(resolve_tls "${OIDC_HTTPS_BIND:-}" "${OIDC_TLS_CERT:-}" "${OIDC_TLS_KEY:-}")
    oidc_https_bind="${tls_bits[0]}"
    oidc_tls_cert="${tls_bits[1]}"
    oidc_tls_key="${tls_bits[2]}"

    tmp_unit="$(mktemp)"
    trap 'rm -f "${tmp_unit}"' EXIT
    cat >"${tmp_unit}" <<EOF
[Unit]
Description=internal_oidc (env: ${ENV_NAME:-unknown})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=RUST_LOG=internal_oidc=info
Environment=OIDC_BIND=${OIDC_BIND}
Environment=OIDC_HTTPS_BIND=${oidc_https_bind}
Environment=OIDC_TLS_CERT=${oidc_tls_cert}
Environment=OIDC_TLS_KEY=${oidc_tls_key}
Environment=OIDC_ISSUER=${OIDC_ISSUER}
Environment=OIDC_CLIENT_ID=${OIDC_CLIENT_ID}
Environment=OIDC_CLIENT_SECRET=${OIDC_CLIENT_SECRET}
Environment=OIDC_ED25519_SEED_B64=${OIDC_ED25519_SEED_B64}
Environment=OIDC_TOKEN_TTL_S=${OIDC_TOKEN_TTL_S:-}
Environment=OIDC_AUTH_CODE_TTL_S=${OIDC_AUTH_CODE_TTL_S:-}
Environment=OIDC_USERS_PATH=${OIDC_USERS_PATH:-}
Environment=OIDC_ALLOWED_REDIRECT_URIS=${OIDC_ALLOWED_REDIRECT_URIS:-}
Environment=OIDC_ALLOW_PLAINTEXT_PASSWORDS=${OIDC_ALLOW_PLAINTEXT_PASSWORDS:-}
Environment=OIDC_ALLOW_REGISTRATION=${OIDC_ALLOW_REGISTRATION:-}
Environment=OIDC_ALLOW_PASSWORD_RESET=${OIDC_ALLOW_PASSWORD_RESET:-}
ExecStart=${OIDC_REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

    unit_name="${OIDC_APP_NAME}.service"
    sudo install -m 0644 "${tmp_unit}" "/etc/systemd/system/${unit_name}"
    sudo systemctl daemon-reload
    sudo systemctl enable --now "${unit_name}"
    sudo systemctl restart "${unit_name}"

    port="${OIDC_BIND##*:}"
    wait_for_listen "${port}" "${unit_name}"
  )
}

deploy_static_web_local() {
  local env_file="$1"
  (
    set -euo pipefail
    set -a
    # shellcheck disable=SC1090
    source "${env_file}"
    set +a

    if [[ "${ENABLED:-1}" != "1" ]]; then
      echo "Skipping disabled env: ${env_file}"
      exit 0
    fi

    : "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
    : "${REMOTE_BIN:?missing REMOTE_BIN in env file}"
    : "${REMOTE_WEB:?missing REMOTE_WEB in env file}"
    : "${DOMAIN:?missing DOMAIN in env file}"
    : "${HTTP_BIND:?missing HTTP_BIND in env file}"

    local remote_bin_dir tmp_unit unit_name port tls_bits https_bind tls_cert tls_key
    remote_bin_dir="$(dirname "${REMOTE_BIN}")"

    ./scripts/build_bookworm_release.sh static_web
    ensure_slopmud_layout "${REMOTE_ROOT}" "${remote_bin_dir}" "${REMOTE_WEB}"
    sudo rsync -a --delete --exclude README.md web_homepage/ "${REMOTE_WEB}/"
    sudo chown -R slopmud:slopmud "${REMOTE_WEB}"
    sudo install -m 0755 -o root -g root target/release/static_web "${REMOTE_BIN}"

    mapfile -t tls_bits < <(resolve_tls "${HTTPS_BIND:-}" "${TLS_CERT:-}" "${TLS_KEY:-}")
    https_bind="${tls_bits[0]}"
    tls_cert="${tls_bits[1]}"
    tls_key="${tls_bits[2]}"

    tmp_unit="$(mktemp)"
    trap 'rm -f "${tmp_unit}"' EXIT
    cat >"${tmp_unit}" <<EOF
[Unit]
Description=slopmud static web server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=BIND=${HTTP_BIND}
Environment=STATIC_DIR=${REMOTE_WEB}
Environment=HTTPS_BIND=${https_bind}
Environment=TLS_CERT=${tls_cert}
Environment=TLS_KEY=${tls_key}
EOF

    [[ -n "${SESSION_TCP_ADDR:-}" ]] && echo "Environment=SESSION_TCP_ADDR=${SESSION_TCP_ADDR}" >>"${tmp_unit}"
    [[ -n "${STATIC_WEB_ENABLE_WS:-}" ]] && echo "Environment=STATIC_WEB_ENABLE_WS=${STATIC_WEB_ENABLE_WS}" >>"${tmp_unit}"

    cat >>"${tmp_unit}" <<EOF
ExecStart=${REMOTE_BIN}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

    unit_name="${WEB_SERVICE_NAME:-slopmud-web}.service"
    sudo install -m 0644 "${tmp_unit}" "/etc/systemd/system/${unit_name}"
    sudo systemctl daemon-reload
    sudo systemctl disable --now nginx 2>/dev/null || true
    sudo systemctl enable --now "${unit_name}"
    sudo systemctl restart "${unit_name}"

    port="${HTTP_BIND##*:}"
    wait_for_listen "${port}" "${unit_name}"
    curl -fsS -H "Host: ${DOMAIN}" "http://127.0.0.1:${port}/healthz" >/dev/null
  )
}

deploy_slopmud_web_local() {
  local env_file="$1"
  (
    set -euo pipefail
    set -a
    # shellcheck disable=SC1090
    source "${env_file}"
    set +a

    if [[ "${ENABLED:-1}" != "1" ]]; then
      echo "Skipping disabled env: ${env_file}"
      exit 0
    fi

    : "${REMOTE_ROOT:?missing REMOTE_ROOT in env file}"
    : "${REMOTE_BIN:?missing REMOTE_BIN in env file}"
    : "${REMOTE_WEB:?missing REMOTE_WEB in env file}"
    : "${DOMAIN:?missing DOMAIN in env file}"
    : "${HTTP_BIND:?missing HTTP_BIND in env file}"

    local remote_bin_dir tmp_unit unit_name port tls_bits https_bind tls_cert tls_key exec_start
    remote_bin_dir="$(dirname "${REMOTE_BIN}")"

    ./scripts/build_bookworm_release.sh slopmud_web
    ensure_slopmud_layout "${REMOTE_ROOT}" "${remote_bin_dir}" "${REMOTE_WEB}" "${REMOTE_ROOT}/env"
    ensure_dir_owned_by_slopmud "${SLOPMUD_GOOGLE_OAUTH_DIR:-}"
    ensure_parent_owned_by_slopmud "${SLOPMUD_ACCOUNTS_PATH:-}"
    sudo rsync -a --delete --exclude README.md web_homepage/ "${REMOTE_WEB}/"
    sudo chown -R slopmud:slopmud "${REMOTE_WEB}"
    sudo install -m 0755 -o root -g root target/release/slopmud_web "${REMOTE_BIN}"

    mapfile -t tls_bits < <(resolve_tls "${HTTPS_BIND:-}" "${TLS_CERT:-}" "${TLS_KEY:-}")
    https_bind="${tls_bits[0]}"
    tls_cert="${tls_bits[1]}"
    tls_key="${tls_bits[2]}"

    tmp_unit="$(mktemp)"
    trap 'rm -f "${tmp_unit}"' EXIT
    cat >"${tmp_unit}" <<EOF
[Unit]
Description=slopmud web server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=slopmud
Group=slopmud
WorkingDirectory=${REMOTE_ROOT}
Environment=BIND=${HTTP_BIND}
Environment=STATIC_DIR=${REMOTE_WEB}
Environment=HTTPS_BIND=${https_bind}
Environment=TLS_CERT=${tls_cert}
Environment=TLS_KEY=${tls_key}
EOF

    [[ -n "${SESSION_TCP_ADDR:-}" ]] && echo "Environment=SESSION_TCP_ADDR=${SESSION_TCP_ADDR}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_GOOGLE_OAUTH_DIR:-}" ]] && echo "Environment=SLOPMUD_GOOGLE_OAUTH_DIR=${SLOPMUD_GOOGLE_OAUTH_DIR}" >>"${tmp_unit}"
    [[ -n "${GOOGLE_OAUTH_CLIENT_ID:-}" ]] && echo "Environment=GOOGLE_OAUTH_CLIENT_ID=${GOOGLE_OAUTH_CLIENT_ID}" >>"${tmp_unit}"
    [[ -n "${GOOGLE_OAUTH_CLIENT_SECRET:-}" ]] && echo "Environment=GOOGLE_OAUTH_CLIENT_SECRET=${GOOGLE_OAUTH_CLIENT_SECRET}" >>"${tmp_unit}"
    [[ -n "${GOOGLE_OAUTH_REDIRECT_URI:-}" ]] && echo "Environment=GOOGLE_OAUTH_REDIRECT_URI=${GOOGLE_OAUTH_REDIRECT_URI}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_AUTH_URL:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_AUTH_URL=${SLOPMUD_OIDC_SSO_AUTH_URL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_TOKEN_URL:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_TOKEN_URL=${SLOPMUD_OIDC_SSO_TOKEN_URL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_USERINFO_URL:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_USERINFO_URL=${SLOPMUD_OIDC_SSO_USERINFO_URL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_CLIENT_ID:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_CLIENT_ID=${SLOPMUD_OIDC_SSO_CLIENT_ID}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_CLIENT_SECRET:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_CLIENT_SECRET=${SLOPMUD_OIDC_SSO_CLIENT_SECRET}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_REDIRECT_URI:-}" ]] && echo "Environment=SLOPMUD_OIDC_SSO_REDIRECT_URI=${SLOPMUD_OIDC_SSO_REDIRECT_URI}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_OIDC_SSO_SCOPE:-}" ]] && echo "Environment=\"SLOPMUD_OIDC_SSO_SCOPE=${SLOPMUD_OIDC_SSO_SCOPE}\"" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_WEBAUTH_JWT_SECRET:-}" ]] && echo "Environment=SLOPMUD_WEBAUTH_JWT_SECRET=${SLOPMUD_WEBAUTH_JWT_SECRET}" >>"${tmp_unit}"
    [[ -n "${GOOGLE_OAUTH_CLIENT_ID_SSM:-}" ]] && echo "Environment=GOOGLE_OAUTH_CLIENT_ID_SSM=${GOOGLE_OAUTH_CLIENT_ID_SSM}" >>"${tmp_unit}"
    [[ -n "${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}" ]] && echo "Environment=GOOGLE_OAUTH_CLIENT_SECRET_SSM=${GOOGLE_OAUTH_CLIENT_SECRET_SSM}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_ENABLED:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_ENABLED=${SLOPMUD_COMPLIANCE_ENABLED}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM=${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON=${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_EMAIL_MODE:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_EMAIL_MODE=${SLOPMUD_COMPLIANCE_EMAIL_MODE}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_EMAIL_FROM:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_EMAIL_FROM=${SLOPMUD_COMPLIANCE_EMAIL_FROM}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SMTP_HOST:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SMTP_HOST=${SLOPMUD_COMPLIANCE_SMTP_HOST}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PORT:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PORT=${SLOPMUD_COMPLIANCE_SMTP_PORT}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SMTP_USERNAME:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SMTP_USERNAME=${SLOPMUD_COMPLIANCE_SMTP_USERNAME}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PASSWORD=${SLOPMUD_COMPLIANCE_SMTP_PASSWORD}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM=${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED=${SLOPMUD_COMPLIANCE_PUBLIC_LOG_ENABLED}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL=${SLOPMUD_COMPLIANCE_PUBLIC_LOG_REDACT_EMAIL}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_SESSION_TTL_S:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_SESSION_TTL_S=${SLOPMUD_COMPLIANCE_SESSION_TTL_S}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_PRESIGN_TTL_S:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_PRESIGN_TTL_S=${SLOPMUD_COMPLIANCE_PRESIGN_TTL_S}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_COMPLIANCE_LOOKBACK_DAYS:-}" ]] && echo "Environment=SLOPMUD_COMPLIANCE_LOOKBACK_DAYS=${SLOPMUD_COMPLIANCE_LOOKBACK_DAYS}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_S3_BUCKET:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_S3_BUCKET=${SLOPMUD_EVENTLOG_S3_BUCKET}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_EVENTLOG_S3_PREFIX:-}" ]] && echo "Environment=SLOPMUD_EVENTLOG_S3_PREFIX=${SLOPMUD_EVENTLOG_S3_PREFIX}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_ACCOUNTS_PATH:-}" ]] && echo "Environment=SLOPMUD_ACCOUNTS_PATH=${SLOPMUD_ACCOUNTS_PATH}" >>"${tmp_unit}"
    [[ -n "${SLOPMUD_ADMIN_ADDR:-}" ]] && echo "Environment=SLOPMUD_ADMIN_ADDR=${SLOPMUD_ADMIN_ADDR}" >>"${tmp_unit}"

    exec_start="${REMOTE_BIN}"
    if [[ -n "${GOOGLE_OAUTH_CLIENT_ID_SSM:-}" || -n "${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}" || -n "${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}" ]]; then
      exec_start="/bin/bash -ceu ' \
        if [[ -n \"\${GOOGLE_OAUTH_CLIENT_ID_SSM:-}\" ]]; then \
          export GOOGLE_OAUTH_CLIENT_ID=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_ID_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
        fi; \
        if [[ -n \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM:-}\" ]]; then \
          export GOOGLE_OAUTH_CLIENT_SECRET=\"\$(aws ssm get-parameter --name \"\${GOOGLE_OAUTH_CLIENT_SECRET_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
        fi; \
        if [[ -n \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM:-}\" ]]; then \
          export SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_PORTAL_CONFIG_JSON_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
        fi; \
        if [[ -n \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM:-}\" ]]; then \
          export SLOPMUD_COMPLIANCE_SMTP_PASSWORD=\"\$(aws ssm get-parameter --name \"\${SLOPMUD_COMPLIANCE_SMTP_PASSWORD_SSM}\" --with-decryption --query Parameter.Value --output text)\"; \
        fi; \
        exec \"${REMOTE_BIN}\"; \
      '"
    fi

    cat >>"${tmp_unit}" <<EOF
ExecStart=${exec_start}
Restart=always
RestartSec=2
NoNewPrivileges=true
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOF

    unit_name="${WEB_SERVICE_NAME:-slopmud-web}.service"
    sudo install -m 0644 "${tmp_unit}" "/etc/systemd/system/${unit_name}"
    sudo systemctl daemon-reload
    sudo systemctl disable --now nginx 2>/dev/null || true
    sudo systemctl enable --now "${unit_name}"
    sudo systemctl restart "${unit_name}"

    port="${HTTP_BIND##*:}"
    wait_for_listen "${port}" "${unit_name}"
    curl -fsS -H "Host: ${DOMAIN}" "http://127.0.0.1:${port}/healthz" >/dev/null
  )
}

resolve_env_file() {
  local key="$1"
  if [[ -f "env/${key}.env" ]]; then
    printf 'env/%s.env\n' "${key}"
    return 0
  fi
  return 1
}

synthesize_oauth_env_file() {
  local oauth_env_file="env/${env_name}-oauth.env"
  if [[ -f "${oauth_env_file}" ]]; then
    return 0
  fi

  cat >"${oauth_env_file}" <<EOF
# Generated during first-boot bootstrap when env/${env_name}-oauth.env is not present in Git.
source "\$(dirname "\${BASH_SOURCE[0]}")/${env_name}.env"

DOMAIN=\${HOST}
REMOTE_BIN=/opt/slopmud/bin/slopmud_web
HTTP_BIND=0.0.0.0:4282
HTTPS_BIND=0.0.0.0:4242
SESSION_TCP_ADDR=\${SLOPMUD_BIND}
WEB_SERVICE_NAME=slopmud-web-${env_name}-oauth

TLS_DST_DIR=/etc/slopmud/tls/mud
TLS_CERT=/etc/slopmud/tls/mud/fullchain.pem
TLS_KEY=/etc/slopmud/tls/mud/privkey.pem
CERTBOT_CERT_NAME=\${HOST}
CERTBOT_DOMAINS="\${HOST}"

SLOPMUD_GOOGLE_AUTH_BASE_URL=https://\${HOST}:4242
GOOGLE_OAUTH_REDIRECT_URI=https://\${HOST}:4242/auth/google/callback
EOF
}

main() {
  local base_env_file landing_env_file web_env_file

  install_system_packages
  ensure_rust_toolchain
  install_github_runner_local "${env_name}" "${github_repo}" "${github_token_ssm}" "${runner_labels}"

  base_env_file="env/${env_name}.env"
  if [[ ! -f "${base_env_file}" ]]; then
    echo "ERROR: env file not found: ${base_env_file}" >&2
    exit 2
  fi

  synthesize_oauth_env_file
  landing_env_file="$(resolve_env_file "${env_name}_landing" || resolve_env_file "${env_name}-landing" || printf '%s\n' "${base_env_file}")"
  web_env_file="$(resolve_env_file "${env_name}-oauth" || printf '%s\n' "${base_env_file}")"

  deploy_shard_local "${base_env_file}"
  deploy_internal_oidc_local "${base_env_file}"
  deploy_slopmud_local "${base_env_file}"
  deploy_static_web_local "${landing_env_file}"
  deploy_slopmud_web_local "${web_env_file}"

  echo "Bootstrap complete for env=${env_name}"
}

main "$@"
