#!/usr/bin/env python3
"""
Report basic "maturity" metrics for `world/areas/*.yaml`.

This is intentionally coarse. It helps authors pick what to work on next by
surfacing how much of a zone is still placeholder/filler scaffolding.

Definitions (by room tags):
- portal room: id starts with "P_"
- setpiece room: any tag starts with "setpiece."
- filler room: any tag starts with "filler." or "seed."
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
AREAS_DIR = REPO_ROOT / "world" / "areas"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


@dataclass(frozen=True)
class AreaStats:
    zone_id: str
    zone_name: str
    rooms_total: int
    rooms_portal: int
    rooms_setpiece: int
    rooms_filler: int

    @property
    def filler_pct(self) -> float:
        return 0.0 if self.rooms_total == 0 else (100.0 * self.rooms_filler / self.rooms_total)


def _room_tags(room: dict) -> list[str]:
    tags = room.get("tags") or []
    if not isinstance(tags, list):
        return []
    out: list[str] = []
    for t in tags:
        if isinstance(t, str) and t:
            out.append(t)
    return out


def _count_rooms(doc: dict) -> AreaStats:
    zone_id = doc.get("zone_id")
    zone_name = doc.get("zone_name")
    if not isinstance(zone_id, str) or not zone_id:
        raise ValueError("missing zone_id")
    if not isinstance(zone_name, str) or not zone_name:
        zone_name = zone_id

    rooms = doc.get("rooms") or []
    if not isinstance(rooms, list):
        raise ValueError("rooms must be a list")

    total = 0
    portal = 0
    setpiece = 0
    filler = 0
    for r in rooms:
        if not isinstance(r, dict):
            continue
        rid = r.get("id")
        if not isinstance(rid, str) or not rid:
            continue
        total += 1
        if rid.startswith("P_"):
            portal += 1
        tags = _room_tags(r)
        if any(t.startswith("setpiece.") for t in tags):
            setpiece += 1
        if any(t.startswith("filler.") or t.startswith("seed.") for t in tags):
            filler += 1

    return AreaStats(
        zone_id=zone_id,
        zone_name=zone_name,
        rooms_total=total,
        rooms_portal=portal,
        rooms_setpiece=setpiece,
        rooms_filler=filler,
    )


def _fmt_md_table(stats: list[AreaStats]) -> str:
    lines: list[str] = []
    lines.append("| zone_id | rooms | portals | setpieces | filler | filler% |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: |")
    for s in stats:
        lines.append(
            f"| `{s.zone_id}` | {s.rooms_total} | {s.rooms_portal} | {s.rooms_setpiece} | {s.rooms_filler} | {s.filler_pct:.1f} |"
        )
    return "\n".join(lines) + "\n"


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--areas-dir", default=str(AREAS_DIR))
    ap.add_argument("--zone-id", action="append", default=[], help="Filter to this zone_id (repeatable)")
    ap.add_argument("--format", choices=["md", "tsv"], default="md")
    args = ap.parse_args(argv)

    areas_dir = Path(args.areas_dir)
    if not areas_dir.exists():
        err(f"error: missing areas dir: {areas_dir}")
        return 2

    files = sorted(areas_dir.glob("*.yaml"))
    if args.zone_id:
        want = set(args.zone_id)
        files = [p for p in files if p.stem in want]
        missing = sorted(want - {p.stem for p in files})
        if missing:
            err("error: missing area files for zone_id: " + ", ".join(missing))
            return 2

    stats: list[AreaStats] = []
    for p in files:
        doc = yaml.safe_load(p.read_text(encoding="utf-8"))
        if not isinstance(doc, dict):
            err(f"warning: skipping non-mapping yaml: {p.relative_to(REPO_ROOT)}")
            continue
        try:
            stats.append(_count_rooms(doc))
        except Exception as e:
            err(f"error: {p.relative_to(REPO_ROOT)}: {e}")
            return 2

    if args.format == "md":
        sys.stdout.write(_fmt_md_table(stats))
        return 0

    # TSV
    sys.stdout.write("zone_id\trooms\tportals\tsetpieces\tfiller\tfiller_pct\n")
    for s in stats:
        sys.stdout.write(
            f"{s.zone_id}\t{s.rooms_total}\t{s.rooms_portal}\t{s.rooms_setpiece}\t{s.rooms_filler}\t{s.filler_pct:.1f}\n"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))

