#!/usr/bin/env python3
"""
Generate stub zone "shape" YAML files under `world/zones/` from:

- `docs/zone_beats.md` (clusters, room budgets, level bands)
- `world/overworld.yaml` (zone IDs, anchors, portals + coords)

This is authoring scaffolding, not a final area format.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OVERWORLD = REPO_ROOT / "world" / "overworld.yaml"
DEFAULT_ZONE_BEATS = REPO_ROOT / "docs" / "zone_beats.md"
DEFAULT_ZONES_DIR = REPO_ROOT / "world" / "zones"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def yaml_quote(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def parse_zone_beats(path: Path) -> dict[str, dict]:
    """
    Return zone_name -> {level_band: [min,max], target_rooms: int, clusters: [{id, rooms}, ...]}.
    """

    text = path.read_text(encoding="utf-8")
    zone_name: str | None = None
    zone_meta: dict[str, dict] = {}

    for line in text.splitlines():
        if line.startswith("### "):
            title = line[len("### ") :].strip()
            # Title ends with "(Lx-Ly, N rooms)".
            m = re.search(r"\(([^()]*)\)\s*$", title)
            if not m:
                zone_name = None
                continue
            trailer = m.group(1)
            name = re.sub(r"\s*\([^()]*\)\s*$", "", title).strip()
            lm = re.search(r"\bL(\d+)\s*-\s*L(\d+)\b", trailer)
            rm = re.search(r"\b(\d+)\s*rooms\b", trailer)
            if not lm or not rm:
                zone_name = None
                continue
            zone_name = name
            zone_meta[zone_name] = {
                "level_band": [int(lm.group(1)), int(lm.group(2))],
                "target_rooms": int(rm.group(1)),
                "clusters": [],
            }
            continue

        if zone_name and line.lstrip().startswith("| CL_"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) < 2:
                continue
            cid = cells[0]
            try:
                rooms = int(cells[1])
            except ValueError:
                continue
            zone_meta[zone_name]["clusters"].append({"id": cid, "rooms": rooms})

    return zone_meta


def compute_bounds(portals: list[dict], margin: int) -> dict:
    xs = [int(p["pos"]["x"]) for p in portals]
    ys = [int(p["pos"]["y"]) for p in portals]
    min_x = min(xs) - margin
    max_x = max(xs) + margin
    min_y = min(ys) - margin
    max_y = max(ys) + margin
    return {"min": {"x": min_x, "y": min_y}, "max": {"x": max_x, "y": max_y}}


def dump_zone_yaml(*, zone: dict, beats: dict, zone_portals: list[dict], bounds: dict) -> str:
    out: list[str] = []
    out.append("version: 1")
    out.append(f"zone_id: {zone['id']}")
    out.append(f"zone_name: {yaml_quote(zone['name'])}")
    out.append("")
    out.append("area_id: A01")
    out.append(f"level_band: [{beats['level_band'][0]}, {beats['level_band'][1]}]")
    out.append(f"target_rooms: {beats['target_rooms']}")
    out.append("")
    out.append("anchor:")
    out.append(f"  x: {zone['anchor']['x']}")
    out.append(f"  y: {zone['anchor']['y']}")
    out.append("")
    out.append("# Global cartesian bounds for planning and validation only.")
    out.append("bounds:")
    out.append("  min:")
    out.append(f"    x: {bounds['min']['x']}")
    out.append(f"    y: {bounds['min']['y']}")
    out.append("  max:")
    out.append(f"    x: {bounds['max']['x']}")
    out.append(f"    y: {bounds['max']['y']}")
    out.append("")
    out.append("clusters:")
    for c in beats["clusters"]:
        out.append(f"  - id: {c['id']}")
        out.append(f"    rooms: {c['rooms']}")
    out.append("")
    out.append("portals:")
    for p in zone_portals:
        out.append(f"  - id: {p['id']}")
        out.append(f"    cluster: {p['cluster_hint']}")
    out.append("")
    out.append("")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(DEFAULT_OVERWORLD))
    ap.add_argument("--zone-beats", default=str(DEFAULT_ZONE_BEATS))
    ap.add_argument("--zones-dir", default=str(DEFAULT_ZONES_DIR))
    ap.add_argument("--margin", type=int, default=2)
    ap.add_argument("--overwrite", action="store_true")
    ap.add_argument("--only", action="append", default=[], help="zone_id to generate (repeatable)")
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zones = overworld.get("zones") or []
    portals = overworld.get("portals") or []
    zone_beats = parse_zone_beats(Path(args.zone_beats))

    zones_dir = Path(args.zones_dir)
    zones_dir.mkdir(parents=True, exist_ok=True)

    only: set[str] = set(args.only or [])
    wrote = 0
    skipped = 0

    for z in zones:
        zid = z["id"]
        if only and zid not in only:
            continue
        zname = z["name"]
        beats = zone_beats.get(zname)
        if beats is None:
            err(f"missing zone beats entry for zone name: {zname!r}")
            return 2
        zone_portals = [p for p in portals if p["zone_id"] == zid]
        if not zone_portals:
            err(f"zone has no portals in overworld.yaml (unexpected): {zid} ({zname})")
            return 2
        bounds = compute_bounds(zone_portals, margin=int(args.margin))

        out_path = zones_dir / f"{zid}.yaml"
        if out_path.exists() and not args.overwrite:
            skipped += 1
            continue

        out_text = dump_zone_yaml(zone=z, beats=beats, zone_portals=zone_portals, bounds=bounds)
        out_path.write_text(out_text, encoding="utf-8")
        wrote += 1

    sys.stdout.write(f"ok: wrote {wrote}, skipped {skipped} (existing)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__('sys').argv[1:]))

