#!/usr/bin/env python3
"""
Generate minimal stub area files (room graphs) under `world/areas/`.

This is intentionally conservative: it only writes files that don't exist unless
`--overwrite` is specified.

The stubs are meant to be edited by humans. They include:
  - all overworld portal rooms for the zone (P_*)
  - a single anchor room (prefer HUB_* key room if present)
  - any key_rooms listed in `world/zones/<zone_id>.yaml`
  - basic exits so everything is reachable from start_room
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
OVERWORLD_YAML = REPO_ROOT / "world" / "overworld.yaml"
ZONES_DIR = REPO_ROOT / "world" / "zones"
AREAS_DIR = REPO_ROOT / "world" / "areas"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def _slug_to_anchor(zone_id: str) -> str:
    s = re.sub(r"[^A-Za-z0-9]+", "_", zone_id).strip("_").upper()
    return f"R_{s}_ANCHOR_01"


def _pick_anchor_key_room(key_rooms: list[dict]) -> dict | None:
    # Prefer a HUB_* room if present; otherwise take the first key room.
    for kr in key_rooms:
        tags = kr.get("tags") or []
        if isinstance(tags, list) and any(isinstance(t, str) and t.startswith("HUB_") for t in tags):
            return kr
    return key_rooms[0] if key_rooms else None


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(OVERWORLD_YAML))
    ap.add_argument("--zones-dir", default=str(ZONES_DIR))
    ap.add_argument("--areas-dir", default=str(AREAS_DIR))
    ap.add_argument("--zone-id", action="append", default=[], help="Generate only this zone_id (repeatable)")
    ap.add_argument("--all", action="store_true", help="Generate for every zone in overworld.yaml")
    ap.add_argument("--overwrite", action="store_true")
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zones = overworld.get("zones") or []
    portals = overworld.get("portals") or []
    edges = overworld.get("edges") or []

    zone_by_id: dict[str, dict] = {z["id"]: z for z in zones}
    portal_by_id: dict[str, dict] = {p["id"]: p for p in portals}

    edge_len: dict[tuple[str, str], int] = {}
    for e in edges:
        a = e.get("a")
        b = e.get("b")
        ln = e.get("len")
        if isinstance(a, str) and isinstance(b, str) and isinstance(ln, int):
            edge_len[tuple(sorted((a, b)))] = ln

    if args.all:
        zone_ids = list(zone_by_id.keys())
    else:
        zone_ids = list(args.zone_id)

    if not zone_ids:
        err("error: pass --all or at least one --zone-id")
        return 2

    zones_dir = Path(args.zones_dir)
    areas_dir = Path(args.areas_dir)
    areas_dir.mkdir(parents=True, exist_ok=True)

    wrote = 0
    skipped = 0
    for zone_id in zone_ids:
        if zone_id not in zone_by_id:
            err(f"error: unknown zone_id: {zone_id}")
            return 2

        out_path = areas_dir / f"{zone_id}.yaml"
        if out_path.exists() and not args.overwrite:
            skipped += 1
            continue

        zone_name = zone_by_id[zone_id]["name"]

        zone_shape_path = zones_dir / f"{zone_id}.yaml"
        if not zone_shape_path.exists():
            err(f"error: missing zone shape file: {zone_shape_path.relative_to(REPO_ROOT)}")
            return 2
        zone_shape = yaml.safe_load(zone_shape_path.read_text(encoding="utf-8")) or {}

        clusters_raw = zone_shape.get("clusters") or []
        cluster_ids: list[str] = []
        for c in clusters_raw:
            if isinstance(c, str):
                cluster_ids.append(c)
            elif isinstance(c, dict) and isinstance(c.get("id"), str):
                cluster_ids.append(c["id"])
        if not cluster_ids:
            err(f"error: no clusters in zone shape: {zone_shape_path.relative_to(REPO_ROOT)}")
            return 2

        key_rooms_raw = zone_shape.get("key_rooms") or []
        key_rooms: list[dict] = [kr for kr in key_rooms_raw if isinstance(kr, dict) and isinstance(kr.get("id"), str)]

        anchor_kr = _pick_anchor_key_room(key_rooms)
        if anchor_kr:
            anchor_id = anchor_kr["id"]
            anchor_cluster = anchor_kr.get("cluster") or cluster_ids[0]
            anchor_tags = anchor_kr.get("tags") or []
        else:
            anchor_id = _slug_to_anchor(zone_id)
            anchor_cluster = cluster_ids[0]
            anchor_tags = []

        my_portals = [p for p in portals if p.get("zone_id") == zone_id]
        if not my_portals:
            err(f"error: zone has no portals in overworld: {zone_id}")
            return 2

        rooms: list[dict] = []

        # Portal rooms first.
        for p in sorted(my_portals, key=lambda x: x.get("id") or ""):
            pid = p["id"]
            other = p.get("connects_to")
            if not isinstance(other, str) or other not in portal_by_id:
                err(f"error: portal {pid} invalid connects_to: {other!r}")
                return 2
            ln = edge_len.get(tuple(sorted((pid, other))))
            if ln is None:
                err(f"error: missing edge len for portal pair {pid} <-> {other}")
                return 2
            other_zone = portal_by_id[other].get("zone_name") or "Unknown Zone"
            cluster = p.get("cluster_hint") or cluster_ids[0]
            rooms.append(
                {
                    "id": pid,
                    "name": f"{other_zone} Transfer",
                    "cluster": cluster,
                    "tags": [f"portal.{pid}"],
                    "desc": f"Portal stub to {other_zone}.",
                    "exits": [
                        {"dir": "out", "to": other, "len": ln},
                        {"dir": "in", "to": anchor_id, "len": 1},
                    ],
                }
            )

        # Anchor room.
        if anchor_id not in {r["id"] for r in rooms}:
            rooms.append(
                {
                    "id": anchor_id,
                    "name": f"{zone_name} Anchor",
                    "cluster": anchor_cluster,
                    "tags": anchor_tags if isinstance(anchor_tags, list) else [],
                    "desc": f"Anchor room for {zone_name}. TODO: flesh out room graph.",
                    "exits": [],
                }
            )

        # Key rooms.
        for kr in key_rooms:
            rid = kr["id"]
            if rid == anchor_id:
                continue
            cluster = kr.get("cluster") or cluster_ids[0]
            tags = kr.get("tags") or []
            rooms.append(
                {
                    "id": rid,
                    "name": rid,
                    "cluster": cluster,
                    "tags": tags if isinstance(tags, list) else [],
                    "desc": f"Key room stub: {rid}. TODO: write original prose.",
                    "exits": [{"dir": "hub", "to": anchor_id, "len": 1}],
                }
            )

        # Wire anchor exits to everything else in-file.
        room_by_id: dict[str, dict] = {r["id"]: r for r in rooms}
        anchor_room = room_by_id[anchor_id]
        exits = anchor_room.get("exits") or []
        for r in rooms:
            if r["id"] == anchor_id:
                continue
            exits.append({"dir": f"to_{r['id']}", "to": r["id"], "len": 1})
        anchor_room["exits"] = exits

        out = {
            "version": 1,
            "zone_id": zone_id,
            "zone_name": zone_name,
            "area_id": zone_shape.get("area_id") or "A01",
            "level_band": zone_shape.get("level_band"),
            "start_room": my_portals[0]["id"],
            "rooms": rooms,
        }

        text = yaml.safe_dump(out, sort_keys=False)
        out_path.write_text(text, encoding="utf-8")
        wrote += 1

    sys.stdout.write(f"ok: wrote {wrote} area file(s), skipped {skipped} (exists)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))
