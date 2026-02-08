#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TODO_FILE="$ROOT/docs/adventures_todo.md"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 1
fi

mapfile -t adventure_ids < <(rg '^## [0-9]+\)' "$TODO_FILE" | sed -E 's/^## [0-9]+\) `([^`]+)`.*/\1/')

expected=(01 02 03 04 05 06 07 08 09 10)

for adventure_id in "${adventure_ids[@]}"; do
  run_dir="$ROOT/protoadventures/party_runs/$adventure_id"
  proto_file="$ROOT/protoadventures/$adventure_id.md"

  have=0
  missing=()
  for n in "${expected[@]}"; do
    if [[ -f "$run_dir/run-$n.md" ]]; then
      have=$((have + 1))
    else
      missing+=("run-$n")
    fi
  done

  proto="no"
  rooms="0"
  if [[ -f "$proto_file" ]]; then
    proto="yes"
    rooms="$(rg --no-filename -c '^### .*R_' "$proto_file" || true)"
  fi

  printf "%-32s runs %2d/10 proto %s rooms %2s" "$adventure_id" "$have" "$proto" "$rooms"
  if ((${#missing[@]} > 0)); then
    printf " missing: %s" "${missing[*]}"
  fi
  echo
done
