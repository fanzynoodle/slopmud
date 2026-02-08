#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


CLASSES = [
    "fighter",
    "rogue",
    "cleric",
    "wizard",
    "ranger",
    "paladin",
    "bard",
    "druid",
    "barbarian",
    "warlock",
    "sorcerer",
    "monk",
]


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def parse_adventure_ids(todo_text: str) -> list[str]:
    # Matches: ## 01) `q1-first-day-on-gaia`
    ids: list[str] = []
    for line in todo_text.splitlines():
        m = re.match(r"^##\s+\d+\)\s+`([^`]+)`\s*$", line)
        if m:
            ids.append(m.group(1))
    return ids


def parse_front_matter(lines: list[str]) -> list[str]:
    if not lines or lines[0].strip() != "---":
        return []
    try:
        end = lines.index("---", 1)
    except ValueError:
        return []

    fm = lines[1:end]

    classes: list[str] = []
    i = 0
    while i < len(fm):
        line = fm[i].rstrip("\n")

        if not line.startswith("party_classes:"):
            i += 1
            continue

        value = line.split(":", 1)[1].strip()
        if value.startswith("["):
            # Bracket list: ["a", "b"] or [a, b]
            inner = value.strip()
            if inner.startswith("["):
                inner = inner[1:]
            if inner.endswith("]"):
                inner = inner[:-1]
            for part in inner.split(","):
                token = part.strip().strip('"').strip("'")
                if token:
                    classes.append(token)
            i += 1
            continue

        # Multiline list:
        # party_classes:
        #   - fighter
        i += 1
        while i < len(fm):
            li = fm[i].strip()
            if not li.startswith("- "):
                break
            token = li[2:].strip().strip('"').strip("'")
            if token:
                classes.append(token)
            i += 1
        continue

    return classes


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    todo_file = root / "docs" / "adventures_todo.md"

    todo_text = read_text(todo_file)
    adventure_ids = parse_adventure_ids(todo_text)
    if not adventure_ids:
        print(f"error: no adventure ids found in {todo_file}", file=sys.stderr)
        return 1

    for adventure_id in adventure_ids:
        run_dir = root / "protoadventures" / "party_runs" / adventure_id
        seen: set[str] = set()

        for run_file in sorted(run_dir.glob("run-*.md")):
            lines = read_text(run_file).splitlines()
            for cls in parse_front_matter(lines):
                seen.add(cls)

        missing = [cls for cls in CLASSES if cls not in seen]
        seen_count = len([cls for cls in CLASSES if cls in seen])
        missing_text = ", ".join(missing) if missing else "-"
        print(f"{adventure_id:<32} classes {seen_count:2d}/12 missing: {missing_text}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

