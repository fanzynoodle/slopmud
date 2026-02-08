#!/usr/bin/env python3
"""
Local file locks for parallel zone authoring.

Why: avoid two agents drafting the same `world/zones/<zone_id>.yaml` at once.

Lock files live at: `locks/areas/<zone_id>.lock` (repo-relative).
They are intentionally not committed.
"""

from __future__ import annotations

import argparse
import datetime as dt
import os
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
LOCKS_DIR = REPO_ROOT / "locks" / "areas"


def utc_now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()


def lock_path(zone_id: str) -> Path:
    safe = zone_id.strip()
    if not safe or any(ch.isspace() for ch in safe) or "/" in safe or "\\" in safe:
        raise ValueError(f"invalid zone_id: {zone_id!r}")
    return LOCKS_DIR / f"{safe}.lock"


def write_lock_file(path: Path, zone_id: str, claimed_by: str, note: str | None) -> None:
    content_lines = [
        f"zone_id: {zone_id}",
        f"claimed_by: {claimed_by}",
        f"claimed_at_utc: {utc_now_iso()}",
    ]
    if note:
        content_lines.append(f"note: {note}")
    content = "\n".join(content_lines) + "\n"

    LOCKS_DIR.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(content)
    except Exception:
        # Best-effort cleanup; ignore failures.
        try:
            os.unlink(path)
        except OSError:
            pass
        raise


def read_lock_file(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def cmd_lock(args: argparse.Namespace) -> int:
    zone_id = args.zone_id
    claimed_by = args.by or os.getenv("USER") or "unknown"
    path = lock_path(zone_id)
    if path.exists():
        sys.stderr.write(f"lock exists: {path}\n")
        sys.stderr.write(read_lock_file(path))
        return 2
    write_lock_file(path, zone_id=zone_id, claimed_by=claimed_by, note=args.note)
    sys.stdout.write(f"locked: {zone_id} ({path})\n")
    return 0


def cmd_unlock(args: argparse.Namespace) -> int:
    zone_id = args.zone_id
    claimed_by = args.by or os.getenv("USER") or "unknown"
    path = lock_path(zone_id)
    if not path.exists():
        sys.stderr.write(f"no lock: {path}\n")
        return 2
    if args.force:
        path.unlink()
        sys.stdout.write(f"unlocked (force): {zone_id}\n")
        return 0

    data = read_lock_file(path)
    expected = f"claimed_by: {claimed_by}"
    if expected not in data:
        sys.stderr.write("refusing to unlock: lock owner mismatch\n")
        sys.stderr.write(f"expected line: {expected}\n")
        sys.stderr.write(data)
        sys.stderr.write("use --force to override\n")
        return 3

    path.unlink()
    sys.stdout.write(f"unlocked: {zone_id}\n")
    return 0


def cmd_status(args: argparse.Namespace) -> int:
    LOCKS_DIR.mkdir(parents=True, exist_ok=True)
    if args.zone_id:
        path = lock_path(args.zone_id)
        if not path.exists():
            sys.stdout.write(f"unlocked: {args.zone_id}\n")
            return 0
        sys.stdout.write(f"locked: {args.zone_id} ({path})\n")
        sys.stdout.write(read_lock_file(path))
        return 0

    locks = sorted(LOCKS_DIR.glob("*.lock"))
    if not locks:
        sys.stdout.write("no locks\n")
        return 0
    for p in locks:
        sys.stdout.write(f"{p.stem}: {p}\n")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Local zone lock helper (repo-local files).")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_lock = sub.add_parser("lock", help="Acquire a zone lock")
    p_lock.add_argument("zone_id")
    p_lock.add_argument("--by", dest="by")
    p_lock.add_argument("--note")
    p_lock.set_defaults(func=cmd_lock)

    p_unlock = sub.add_parser("unlock", help="Release a zone lock")
    p_unlock.add_argument("zone_id")
    p_unlock.add_argument("--by", dest="by")
    p_unlock.add_argument("--force", action="store_true")
    p_unlock.set_defaults(func=cmd_unlock)

    p_status = sub.add_parser("status", help="List locks (or show one)")
    p_status.add_argument("zone_id", nargs="?")
    p_status.set_defaults(func=cmd_status)

    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

