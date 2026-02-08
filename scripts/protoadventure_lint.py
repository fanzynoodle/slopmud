#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOM_ID_RE = re.compile(r"R_[A-Z0-9_]+")


@dataclass(frozen=True)
class Exit:
    src: str
    dir: str
    dst: str
    raw: str


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def parse_adventure_ids(todo_text: str) -> list[str]:
    # Matches: ## 01) `q1-first-day-on-gaia`
    ids: list[str] = []
    for line in todo_text.splitlines():
        m = re.match(r"^##\s+\d+\)\s+`([^`]+)`\s*$", line)
        if m:
            ids.append(m.group(1))
    return ids


def extract_room_id_from_heading(h: str) -> str | None:
    # Mirrors the shard-side heuristic: scan for "R_" and consume [A-Z0-9_]+.
    bs = h.encode("utf-8", errors="ignore")
    for i in range(0, max(0, len(bs) - 1)):
        if bs[i : i + 2] != b"R_":
            continue
        j = i + 2
        while j < len(bs):
            c = chr(bs[j])
            if c.isalnum() or c == "_":
                j += 1
            else:
                break
        if j > i + 2:
            token = h[i:j]
            # Only accept all-caps room ids; avoids matching prose "R_" substrings.
            if ROOM_ID_RE.fullmatch(token):
                return token
    return None


def parse_exit_tokens(rest: str) -> list[tuple[str, str]]:
    # Parses: "east -> `R_FOO`" and comma-separated variants.
    out: list[tuple[str, str]] = []
    for part in rest.split(","):
        p = part.strip()
        if not p:
            continue
        if "->" not in p:
            continue
        lhs, rhs = p.split("->", 1)
        dir_token = lhs.strip()
        m = ROOM_ID_RE.search(rhs)
        if not m:
            continue
        out.append((dir_token, m.group(0)))
    return out


def lint_proto_file(path: Path) -> tuple[list[str], list[str]]:
    errors: list[str] = []
    warnings: list[str] = []

    text = read_text(path)
    lines = text.splitlines()

    rooms: dict[str, int] = {}
    room_order: list[str] = []
    room_has_exits: dict[str, bool] = {}
    exits: list[Exit] = []

    cur_room: str | None = None
    in_exits_block = False

    for idx, raw in enumerate(lines, start=1):
        line = raw.rstrip("\n")

        if line.startswith("### "):
            rid = extract_room_id_from_heading(line[len("### ") :])
            if rid is not None:
                cur_room = rid
                in_exits_block = False
                if rid in rooms:
                    errors.append(f"{path}: duplicate room id {rid} (first at line {rooms[rid]})")
                else:
                    rooms[rid] = idx
                    room_order.append(rid)
                    room_has_exits[rid] = False
            else:
                # Non-room heading; do not clear cur_room, so rooms can be separated by subheadings.
                in_exits_block = False
            continue

        if cur_room is None:
            continue

        t = line.strip()
        if t.lower().startswith("- exits:"):
            rest = t.split(":", 1)[1].strip()
            toks = parse_exit_tokens(rest)
            for d, dst in toks:
                exits.append(Exit(src=cur_room, dir=d, dst=dst, raw=line))
            room_has_exits[cur_room] = True if toks else room_has_exits[cur_room]
            in_exits_block = True
            continue

        if in_exits_block:
            # Support multi-line exits block:
            # - exits:
            #   - north -> R_FOO
            if t.startswith("- "):
                rest = t[2:].strip()
                toks = parse_exit_tokens(rest)
                if toks:
                    for d, dst in toks:
                        exits.append(Exit(src=cur_room, dir=d, dst=dst, raw=line))
                    room_has_exits[cur_room] = True
                    continue
                # Any other bullet ends the exits block.
                in_exits_block = False
            elif t.startswith("-"):
                in_exits_block = False

    if not room_order:
        errors.append(f"{path}: no room headings found (need '### R_*' headings)")
        return errors, warnings

    known = set(room_order)
    unknown: list[str] = []
    for ex in exits:
        if ex.dst not in known:
            unknown.append(f"{path}:{rooms.get(ex.src, 1)}: {ex.src} exit '{ex.dir}' -> {ex.dst} (unknown)")

    if unknown:
        errors.extend(unknown)

    # Reachability: warn on rooms that exist but cannot be reached from the first room.
    adj: dict[str, list[str]] = {rid: [] for rid in room_order}
    for ex in exits:
        if ex.dst in known:
            adj[ex.src].append(ex.dst)

    start_room = room_order[0]
    seen: set[str] = set()
    stack: list[str] = [start_room]
    while stack:
        rid = stack.pop()
        if rid in seen:
            continue
        seen.add(rid)
        stack.extend(adj.get(rid, []))

    unreachable = [rid for rid in room_order if rid not in seen]
    if unreachable:
        short = ", ".join(unreachable[:8]) + (" ..." if len(unreachable) > 8 else "")
        warnings.append(f"{path}: unreachable rooms from {start_room}: {short}")

    no_exit_rooms = [rid for rid in room_order if not room_has_exits.get(rid, False)]
    if no_exit_rooms:
        # Commonly ok for a terminal room; warn only.
        short = ", ".join(no_exit_rooms[:8]) + (" ..." if len(no_exit_rooms) > 8 else "")
        warnings.append(f"{path}: rooms without exits: {short}")

    return errors, warnings


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    todo_file = root / "docs" / "adventures_todo.md"
    todo_text = read_text(todo_file)
    adventure_ids = parse_adventure_ids(todo_text)
    if not adventure_ids:
        print(f"error: no adventure ids found in {todo_file}", file=sys.stderr)
        return 2

    all_errors: list[str] = []
    all_warnings: list[str] = []

    for adventure_id in adventure_ids:
        path = root / "protoadventures" / f"{adventure_id}.md"
        if not path.exists():
            all_errors.append(f"{path}: missing protoadventure file")
            continue
        errs, warns = lint_proto_file(path)
        all_errors.extend(errs)
        all_warnings.extend(warns)

    if all_warnings:
        print("warnings:")
        for w in all_warnings:
            print(f" - {w}")

    if all_errors:
        print("errors:", file=sys.stderr)
        for e in all_errors:
            print(f" - {e}", file=sys.stderr)
        return 1

    print(f"ok: {len(adventure_ids)} protoadventures linted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
