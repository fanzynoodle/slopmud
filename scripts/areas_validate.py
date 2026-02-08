#!/usr/bin/env python3
"""
Validate zone "shape" YAML files under `world/zones/`.

Checks:
  - zone_id matches filename and exists in `world/overworld.yaml`
  - portals listed are exactly the portals belonging to the zone
  - zone bounds contain all portal coordinates
  - clusters listed match official cluster IDs in `docs/zone_beats.md`
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
ZONE_BEATS_MD = REPO_ROOT / "docs" / "zone_beats.md"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def load_zone_beats_clusters() -> dict[str, list[str]]:
    """
    Parse `docs/zone_beats.md` into zone_name -> [CL_*].

    This is intentionally a lightweight parser: it keys off `### <Zone Name>`
    headings and collects `CL_` IDs from subsequent table rows.
    """

    text = ZONE_BEATS_MD.read_text(encoding="utf-8")
    zone_name: str | None = None
    clusters_by_zone: dict[str, list[str]] = {}

    for line in text.splitlines():
        if line.startswith("### "):
            # Example: "### Town: Gaia Gate (L1-L4, 120 rooms)"
            # Example: "### Factory District (Outskirts) (L7-L10, 120 rooms)"
            title = line[len("### ") :].strip()
            # Strip only the trailing "(L..-L.., N rooms)" group; keep parentheses in the actual zone name.
            zone_name = re.sub(r"\s*\([^()]*\)\s*$", "", title).strip()
            clusters_by_zone.setdefault(zone_name, [])
            continue
        if zone_name and line.lstrip().startswith("| CL_"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if cells:
                clusters_by_zone[zone_name].append(cells[0])

    return clusters_by_zone


def within(bounds: dict, x: int, y: int) -> bool:
    mn = bounds["min"]
    mx = bounds["max"]
    return mn["x"] <= x <= mx["x"] and mn["y"] <= y <= mx["y"]


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(OVERWORLD_YAML))
    ap.add_argument("--zones-dir", default=str(ZONES_DIR))
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zones = overworld.get("zones") or []
    portals = overworld.get("portals") or []
    edges = overworld.get("edges") or []

    zone_by_id: dict[str, dict] = {z["id"]: z for z in zones}
    portal_by_id: dict[str, dict] = {p["id"]: p for p in portals}

    edge_keys: set[tuple[str, str]] = {tuple(sorted((e["a"], e["b"]))) for e in edges}

    clusters_by_zone_name = load_zone_beats_clusters()

    zones_dir = Path(args.zones_dir)
    if not zones_dir.exists():
        sys.stdout.write(f"no zones dir: {zones_dir.relative_to(REPO_ROOT)}\n")
        return 0

    zone_files = sorted(zones_dir.glob("*.yaml"))
    if not zone_files:
        sys.stdout.write("no zone shape files\n")
        return 0

    for path in zone_files:
        z = yaml.safe_load(path.read_text(encoding="utf-8"))
        zone_id = z.get("zone_id")
        if zone_id != path.stem:
            err(f"{path.relative_to(REPO_ROOT)}: zone_id {zone_id!r} must match filename stem {path.stem!r}")
            return 2
        if zone_id not in zone_by_id:
            err(f"{path.relative_to(REPO_ROOT)}: unknown zone_id in overworld.yaml: {zone_id}")
            return 2
        expected_zone_name = zone_by_id[zone_id]["name"]
        zone_name = z.get("zone_name")
        if zone_name and zone_name != expected_zone_name:
            err(
                f"{path.relative_to(REPO_ROOT)}: zone_name {zone_name!r} does not match overworld {expected_zone_name!r}"
            )
            return 2

        bounds = z.get("bounds")
        if not bounds or "min" not in bounds or "max" not in bounds:
            err(f"{path.relative_to(REPO_ROOT)}: missing bounds.min/bounds.max")
            return 2

        expected_clusters = clusters_by_zone_name.get(expected_zone_name)
        if not expected_clusters:
            err(f"{path.relative_to(REPO_ROOT)}: could not find clusters for zone in docs/zone_beats.md: {expected_zone_name}")
            return 2
        expected_cluster_set = set(expected_clusters)

        # Portals: must be exact match for zone_id.
        expected_portals = sorted([p["id"] for p in portals if p["zone_id"] == zone_id])
        got_portals = z.get("portals") or []
        got_portal_ids: list[str] = []
        got_portal_records: list[dict] = []
        for item in got_portals:
            if isinstance(item, str):
                got_portal_ids.append(item)
            elif isinstance(item, dict) and "id" in item:
                got_portal_ids.append(item["id"])
                got_portal_records.append(item)
            else:
                err(f"{path.relative_to(REPO_ROOT)}: invalid portal entry: {item!r}")
                return 2
        got_portal_ids = sorted(got_portal_ids)
        if got_portal_ids != expected_portals:
            err(f"{path.relative_to(REPO_ROOT)}: portals mismatch")
            err(f"  expected: {expected_portals}")
            err(f"  got:      {got_portal_ids}")
            return 2

        # Portal coords must be within bounds; portal pairs must have an edge length.
        for pid in expected_portals:
            p = portal_by_id[pid]
            x = int(p["pos"]["x"])
            y = int(p["pos"]["y"])
            if not within(bounds, x, y):
                err(f"{path.relative_to(REPO_ROOT)}: portal {pid} at ({x},{y}) is outside bounds")
                return 2
            ch = p.get("cluster_hint")
            if ch and ch not in expected_cluster_set:
                err(f"{path.relative_to(REPO_ROOT)}: portal {pid} cluster_hint not in zone clusters: {ch}")
                return 2
            other = p["connects_to"]
            key = tuple(sorted((pid, other)))
            if key not in edge_keys:
                err(f"{path.relative_to(REPO_ROOT)}: missing edge length for portal pair {pid} <-> {other}")
                return 2

        # Clusters must match official zone beats cluster IDs (exact set).
        got_clusters = z.get("clusters") or []
        got_cluster_ids: list[str] = []
        for c in got_clusters:
            if isinstance(c, str):
                got_cluster_ids.append(c)
            elif isinstance(c, dict) and "id" in c:
                got_cluster_ids.append(c["id"])
            else:
                err(f"{path.relative_to(REPO_ROOT)}: invalid cluster entry: {c!r}")
                return 2

        if sorted(got_cluster_ids) != sorted(expected_clusters):
            err(f"{path.relative_to(REPO_ROOT)}: clusters mismatch vs docs/zone_beats.md")
            err(f"  expected: {sorted(expected_clusters)}")
            err(f"  got:      {sorted(got_cluster_ids)}")
            return 2

        # If portal entries specify a cluster, it must be a cluster in this zone.
        got_cluster_set = set(got_cluster_ids)
        for pr in got_portal_records:
            cluster = pr.get("cluster") or pr.get("cluster_hint")
            if cluster and cluster not in got_cluster_set:
                err(f"{path.relative_to(REPO_ROOT)}: portal {pr.get('id')!r} references unknown cluster: {cluster}")
                return 2

        # Optional authoring hints: validate references if present.
        cluster_edges = z.get("cluster_edges")
        if cluster_edges is not None:
            if not isinstance(cluster_edges, list):
                err(f"{path.relative_to(REPO_ROOT)}: cluster_edges must be a list")
                return 2
            for edge in cluster_edges:
                if not isinstance(edge, dict):
                    err(f"{path.relative_to(REPO_ROOT)}: cluster_edges entries must be mappings: {edge!r}")
                    return 2
                a = edge.get("a")
                b = edge.get("b")
                if not isinstance(a, str) or not isinstance(b, str):
                    err(f"{path.relative_to(REPO_ROOT)}: cluster_edges entries must have string a/b: {edge!r}")
                    return 2
                if a not in got_cluster_set or b not in got_cluster_set:
                    err(f"{path.relative_to(REPO_ROOT)}: cluster_edges references unknown cluster(s): {a!r} {b!r}")
                    return 2

        hubs = z.get("hubs")
        if hubs is not None:
            if not isinstance(hubs, list) or not all(isinstance(h, str) for h in hubs):
                err(f"{path.relative_to(REPO_ROOT)}: hubs must be a list of strings")
                return 2

        key_rooms = z.get("key_rooms")
        if key_rooms is not None:
            if not isinstance(key_rooms, list):
                err(f"{path.relative_to(REPO_ROOT)}: key_rooms must be a list")
                return 2
            for kr in key_rooms:
                if not isinstance(kr, dict):
                    err(f"{path.relative_to(REPO_ROOT)}: key_rooms entries must be mappings: {kr!r}")
                    return 2
                rid = kr.get("id")
                cluster = kr.get("cluster")
                if not isinstance(rid, str) or not isinstance(cluster, str):
                    err(f"{path.relative_to(REPO_ROOT)}: key_rooms entries must have string id/cluster: {kr!r}")
                    return 2
                if cluster not in got_cluster_set:
                    err(f"{path.relative_to(REPO_ROOT)}: key_room {rid!r} references unknown cluster: {cluster!r}")
                    return 2
                tags = kr.get("tags")
                if tags is not None and (not isinstance(tags, list) or not all(isinstance(t, str) for t in tags)):
                    err(f"{path.relative_to(REPO_ROOT)}: key_room {rid!r} tags must be a list of strings")
                    return 2

    sys.stdout.write(f"ok: validated {len(zone_files)} zone shape file(s)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))
