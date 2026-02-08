#!/usr/bin/env python3
"""
Validate the exported overworld spec (`world/overworld.yaml`).

This is a structural validator for authoring-time consistency. It is not an engine
validator yet.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PATH = REPO_ROOT / "world" / "overworld.yaml"


STARTER_ZONE_NAMES = {
    "Newbie School",
    "Town: Gaia Gate",
    "Meadowline",
    "Scrap Orchard",
}


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--path", default=str(DEFAULT_PATH))
    args = ap.parse_args(argv)

    path = Path(args.path)
    data = yaml.safe_load(path.read_text(encoding="utf-8"))

    zones = data.get("zones") or []
    portals = data.get("portals") or []
    edges = data.get("edges") or []

    zone_by_id: dict[str, dict] = {}
    for z in zones:
        zid = z.get("id")
        if not zid:
            err("zone missing id")
            return 2
        if zid in zone_by_id:
            err(f"duplicate zone id: {zid}")
            return 2
        zone_by_id[zid] = z

    portal_by_id: dict[str, dict] = {}
    for p in portals:
        pid = p.get("id")
        if not pid:
            err("portal missing id")
            return 2
        if pid in portal_by_id:
            err(f"duplicate portal id: {pid}")
            return 2
        zid = p.get("zone_id")
        if zid not in zone_by_id:
            err(f"portal {pid} references unknown zone_id: {zid!r}")
            return 2
        portal_by_id[pid] = p

    edge_keys: set[tuple[str, str]] = set()
    for e in edges:
        a = e.get("a")
        b = e.get("b")
        ln = e.get("len")
        if a not in portal_by_id or b not in portal_by_id:
            err(f"edge references unknown portal(s): {a!r} {b!r}")
            return 2
        if not isinstance(ln, int) or ln <= 0:
            err(f"edge {a} <-> {b} has invalid len: {ln!r}")
            return 2
        key = tuple(sorted((a, b)))
        if key in edge_keys:
            err(f"duplicate edge (undirected): {a} <-> {b}")
            return 2
        edge_keys.add(key)

        za = portal_by_id[a]["zone_name"]
        zb = portal_by_id[b]["zone_name"]
        if (za in STARTER_ZONE_NAMES) or (zb in STARTER_ZONE_NAMES):
            if ln != 1:
                err(f"starter-region edge must be len=1: {a} ({za}) <-> {b} ({zb}) is len={ln}")
                return 2

    # Portals must have reciprocal connects_to and a matching edge length.
    for pid, p in portal_by_id.items():
        other = p.get("connects_to")
        if other not in portal_by_id:
            err(f"portal {pid} connects_to unknown portal: {other!r}")
            return 2
        if portal_by_id[other].get("connects_to") != pid:
            err(f"portal {pid} connects_to {other} but not reciprocal")
            return 2
        key = tuple(sorted((pid, other)))
        if key not in edge_keys:
            err(f"missing edge length for portal pair: {pid} <-> {other}")
            return 2

    # Edge lengths should only exist for actual portal pairs.
    for a, b in edge_keys:
        if portal_by_id[a].get("connects_to") != b or portal_by_id[b].get("connects_to") != a:
            err(f"edge exists but portals do not connect_to each other: {a} <-> {b}")
            return 2

    sys.stdout.write(
        f"ok: {len(zones)} zones, {len(portals)} portals, {len(edges)} edges in {path.relative_to(REPO_ROOT)}\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))

