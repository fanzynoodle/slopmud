#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${GITHUB_OUTPUT:-}" ]]; then
  echo "GITHUB_OUTPUT is not set; this script must run in a GitHub Actions step context."
  exit 2
fi

track="${TRACK:-${DEPLOY_ENV:-${GITHUB_REF_NAME:-dev}}}"
track="${track#refs/heads/}"

run_all=0
run_core=0
run_ws=0
reason="scoped-no-changes"
changed_files=()

# Non-dev tracks or manual dispatch should run all e2e suites by default.
if [[ "$track" != "dev" ]]; then
  run_all=1
  reason="non-dev-track"
elif [[ "${GITHUB_EVENT_NAME:-}" != "push" ]]; then
  if [[ "${FORCE_SCOPED_E2E:-0}" == "1" ]]; then
    reason="manual-scoped"
  else
    run_all=1
    reason="workflow-dispatch"
  fi
fi

if [[ "$run_all" -eq 0 ]]; then
  head="${GITHUB_SHA:-$(git rev-parse HEAD)}"
  base="${GITHUB_EVENT_BEFORE:-}"

  if [[ -z "$base" || "$base" == "0000000000000000000000000000000000000000" ]]; then
    base="$(git rev-parse "${head}^" 2>/dev/null || true)"
  elif ! git cat-file -e "$base^{commit}" >/dev/null 2>&1; then
    base=""
  fi

  if [[ -z "$base" ]] || ! git cat-file -e "$base^{commit}" >/dev/null 2>&1; then
    base="$(git rev-list --max-parents=0 HEAD | tail -n 1)"
  fi

  mapfile -t changed_files < <(git diff --name-only "$base" "$head" | sort -u)

  if ((${#changed_files[@]} == 0)); then
    reason="no-diff-range"
  else
    reason="path-scoped"
    for file in "${changed_files[@]}"; do
      case "$file" in
        apps/slopmud/*|apps/shard_01/*|apps/combat_refs/*|apps/bot_party/*|apps/sbc_deciderd/*|apps/sbc_enforcerd/*|apps/sbc_metricsd/*|apps/sbc_raftd/*|scripts/e2e_local.py|scripts/e2e_party_run.py)
          run_core=1
          ;;
        apps/ws_gateway/*|scripts/e2e_ws*.py|apps/ws_gateway/src/bin/e2e_ws.rs)
          run_ws=1
          ;;
        .github/workflows/*|scripts/cicd/*|Cargo.toml|Cargo.lock|Justfile|justfile|Makefile)
          run_all=1
          reason="infra-and-tools-change"
          ;;
      esac
      if [[ "$run_all" -eq 1 ]]; then
        break
      fi
    done
  fi
fi

if [[ "$run_all" -eq 1 ]]; then
  run_core=1
  run_ws=1
fi

if (( run_core == 0 && run_ws == 0 )); then
  reason="${reason}-no-targeted-changes"
fi

changed_count="${#changed_files[@]}"
if (( changed_count > 0 )); then
  changed_csv="$(printf '%s\n' "${changed_files[@]}" | tr '\n' ',' | sed 's/,$//')"
else
  changed_csv=""
fi

if [[ "$run_core" -eq 1 ]]; then
  echo "run_e2e_core=1" >> "$GITHUB_OUTPUT"
else
  echo "run_e2e_core=0" >> "$GITHUB_OUTPUT"
fi

if [[ "$run_ws" -eq 1 ]]; then
  echo "run_e2e_ws=1" >> "$GITHUB_OUTPUT"
else
  echo "run_e2e_ws=0" >> "$GITHUB_OUTPUT"
fi

echo "changed_count=${changed_count}" >> "$GITHUB_OUTPUT"
echo "changed_files=${changed_csv}" >> "$GITHUB_OUTPUT"
echo "scope_reason=${reason}" >> "$GITHUB_OUTPUT"
echo "::notice::ci-scope track=$track reason=$reason e2e_core=$run_core e2e_ws=$run_ws changed_count=$changed_count"
