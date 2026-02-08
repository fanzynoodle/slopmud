#!/usr/bin/env python3
"""
Generate draft zone "shape" YAML files under `world/zones/`.

Inputs:
  - `world/overworld.yaml` (zone ids + portals + coordinates)
  - `docs/zone_beats.md`   (official cluster IDs + level bands + room budgets)

Outputs:
  - `world/zones/<zone_id>.yaml`

These files are planning artifacts (not engine data). The required structure is
validated by `scripts/areas_validate.py`.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
OVERWORLD_YAML = REPO_ROOT / "world" / "overworld.yaml"
ZONE_BEATS_MD = REPO_ROOT / "docs" / "zone_beats.md"
ZONES_DIR = REPO_ROOT / "world" / "zones"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


_LEVEL_ROOMS_RE = re.compile(r"\(L(\d+)-(\d+),\s*(\d+)\s+rooms\)")


def load_zone_beats() -> dict[str, dict]:
    """
    Parse `docs/zone_beats.md` into:
      zone_name -> { level_band: [min,max] | None, target_rooms: int | None, clusters: [CL_*] }

    This intentionally stays lightweight and table-driven (cluster IDs are the source of truth).
    """

    text = ZONE_BEATS_MD.read_text(encoding="utf-8")

    zone_name: str | None = None
    out: dict[str, dict] = {}

    for line in text.splitlines():
        if line.startswith("### "):
            title = line[len("### ") :].strip()
            # Strip trailing "(...)" from the visible zone name.
            zone_name = re.sub(r"\s*\(.*\)\s*$", "", title).strip()
            out.setdefault(zone_name, {"level_band": None, "target_rooms": None, "clusters": []})

            m = _LEVEL_ROOMS_RE.search(title)
            if m:
                out[zone_name]["level_band"] = [int(m.group(1)), int(m.group(2))]
                out[zone_name]["target_rooms"] = int(m.group(3))
            continue

        if zone_name and line.lstrip().startswith("| CL_"):
            # Example row:
            # | CL_SEWERS_ENTRY | 15 | entry tunnels from Town |
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if cells and cells[0].startswith("CL_"):
                out[zone_name]["clusters"].append(cells[0])

    return out


def compute_bounds(portals: list[dict]) -> dict:
    xs = [int(p["pos"]["x"]) for p in portals]
    ys = [int(p["pos"]["y"]) for p in portals]
    if not xs or not ys:
        # Zones without portals shouldn't exist, but keep output valid.
        return {"min": {"x": 0, "y": 0}, "max": {"x": 0, "y": 0}}

    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)

    # Keep bounds non-degenerate and leave a small pad for authoring convenience.
    pad = 1
    min_x -= pad
    max_x += pad
    min_y -= pad
    max_y += pad
    if min_x == max_x:
        min_x -= 1
        max_x += 1
    if min_y == max_y:
        min_y -= 1
        max_y += 1

    return {"min": {"x": min_x, "y": min_y}, "max": {"x": max_x, "y": max_y}}


def write_zone_file(*, path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = yaml.safe_dump(data, sort_keys=False)
    path.write_text(text, encoding="utf-8")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(OVERWORLD_YAML))
    ap.add_argument("--zone-beats", default=str(ZONE_BEATS_MD))
    ap.add_argument("--zones-dir", default=str(ZONES_DIR))
    ap.add_argument("--zone-id", action="append", default=[], help="Generate only this zone_id (repeatable)")
    ap.add_argument("--all", action="store_true", help="Generate for every zone in overworld.yaml")
    ap.add_argument("--overwrite", action="store_true")
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zone_by_id: dict[str, dict] = {z["id"]: z for z in (overworld.get("zones") or [])}
    portals = overworld.get("portals") or []

    if not zone_by_id:
        err("error: no zones found in overworld.yaml")
        return 2

    # Allow caller to override paths via args, but keep single-source parsing.
    global ZONE_BEATS_MD  # noqa: PLW0603 - intentional: reuse helper with new path
    ZONE_BEATS_MD = Path(args.zone_beats)
    beats = load_zone_beats()

    if args.all:
        zone_ids = list(zone_by_id.keys())
    else:
        zone_ids = list(args.zone_id)

    if not zone_ids:
        err("error: pass --all or at least one --zone-id")
        return 2

    zones_dir = Path(args.zones_dir)
    zones_dir.mkdir(parents=True, exist_ok=True)

    wrote = 0
    skipped = 0
    for zone_id in zone_ids:
        if zone_id not in zone_by_id:
            err(f"error: unknown zone_id: {zone_id}")
            return 2

        zone_name = zone_by_id[zone_id]["name"]
        if zone_name not in beats:
            err(f"error: zone name {zone_name!r} not found in docs/zone_beats.md")
            return 2
        clusters = beats[zone_name]["clusters"]
        if not clusters:
            err(f"error: no clusters parsed for zone {zone_name!r} in docs/zone_beats.md")
            return 2

        zone_portals = [p for p in portals if p["zone_id"] == zone_id]
        portal_ids = [p["id"] for p in zone_portals]
        if not portal_ids:
            err(f"error: no portals for zone_id {zone_id} in overworld.yaml")
            return 2

        out_path = zones_dir / f"{zone_id}.yaml"
        if out_path.exists() and not args.overwrite:
            skipped += 1
            continue

        out = {
            "version": 1,
            "zone_id": zone_id,
            "zone_name": zone_name,
            "area_id": "A01",
            "level_band": beats[zone_name]["level_band"],
            "target_rooms": beats[zone_name]["target_rooms"],
            "bounds": compute_bounds(zone_portals),
            "portals": portal_ids,
            "clusters": clusters,
        }

        write_zone_file(path=out_path, data=out)
        wrote += 1

    sys.stdout.write(f"ok: wrote {wrote} zone file(s), skipped {skipped} (exists)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__('sys').argv[1:]))

