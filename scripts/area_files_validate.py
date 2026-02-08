#!/usr/bin/env python3
"""
Validate engine-facing "area files" (room graphs) under `world/areas/`.

These files are the next step after zone shaping (`world/zones/*.yaml`):
they define concrete rooms + exits, including overworld portal exits.

Checks (v1):
  - filename stem matches `zone_id` and exists in `world/overworld.yaml`
  - room ids are unique
  - every room has a valid `cluster` from `world/zones/<zone_id>.yaml`
  - every exit points to:
      - a room id in-file, OR
      - an overworld portal id, OR
      - a sealed placeholder (`state: sealed` and/or `opens_area: A##`)
  - every overworld portal in the zone exists as a room id (P_*)
  - portal rooms have an exit to their `connects_to` portal with matching `len`
  - cluster room counts are compared to zone shape budgets (warn if under target)
"""

from __future__ import annotations

import argparse
import sys
from collections import defaultdict, deque
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
OVERWORLD_YAML = REPO_ROOT / "world" / "overworld.yaml"
ZONES_DIR = REPO_ROOT / "world" / "zones"
AREAS_DIR = REPO_ROOT / "world" / "areas"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def warn(msg: str) -> None:
    sys.stderr.write("warning: " + msg.rstrip() + "\n")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(OVERWORLD_YAML))
    ap.add_argument("--zones-dir", default=str(ZONES_DIR))
    ap.add_argument("--areas-dir", default=str(AREAS_DIR))
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zones = overworld.get("zones") or []
    portals = overworld.get("portals") or []
    edges = overworld.get("edges") or []

    zone_by_id: dict[str, dict] = {z["id"]: z for z in zones}
    portal_by_id: dict[str, dict] = {p["id"]: p for p in portals}
    portal_ids: set[str] = set(portal_by_id.keys())

    edge_len: dict[tuple[str, str], int] = {}
    for e in edges:
        a = e.get("a")
        b = e.get("b")
        ln = e.get("len")
        if not isinstance(a, str) or not isinstance(b, str) or not isinstance(ln, int):
            continue
        edge_len[tuple(sorted((a, b)))] = ln

    areas_dir = Path(args.areas_dir)
    if not areas_dir.exists():
        sys.stdout.write(f"no areas dir: {areas_dir.relative_to(REPO_ROOT)}\n")
        return 0

    area_files = sorted(areas_dir.glob("*.yaml"))
    if not area_files:
        sys.stdout.write("no area files\n")
        return 0

    ok_count = 0
    for path in area_files:
        rel = path.relative_to(REPO_ROOT)
        doc = yaml.safe_load(path.read_text(encoding="utf-8"))
        if not isinstance(doc, dict):
            err(f"{rel}: expected mapping at top level")
            return 2

        zone_id = doc.get("zone_id")
        if not isinstance(zone_id, str) or not zone_id:
            err(f"{rel}: missing zone_id")
            return 2
        if zone_id != path.stem:
            err(f"{rel}: zone_id {zone_id!r} must match filename stem {path.stem!r}")
            return 2
        if zone_id not in zone_by_id:
            err(f"{rel}: unknown zone_id in overworld.yaml: {zone_id}")
            return 2

        expected_zone_name = zone_by_id[zone_id]["name"]
        zone_name = doc.get("zone_name")
        if zone_name is not None and zone_name != expected_zone_name:
            err(f"{rel}: zone_name {zone_name!r} does not match overworld {expected_zone_name!r}")
            return 2

        # Load the corresponding zone shape for cluster list + budgets.
        zone_shape_path = Path(args.zones_dir) / f"{zone_id}.yaml"
        if not zone_shape_path.exists():
            err(f"{rel}: missing zone shape file: {zone_shape_path.relative_to(REPO_ROOT)}")
            return 2
        zone_shape = yaml.safe_load(zone_shape_path.read_text(encoding="utf-8"))
        clusters_raw = (zone_shape or {}).get("clusters") or []
        cluster_ids: list[str] = []
        cluster_budget: dict[str, int] = {}
        for c in clusters_raw:
            if isinstance(c, str):
                cluster_ids.append(c)
            elif isinstance(c, dict) and isinstance(c.get("id"), str):
                cid = c["id"]
                cluster_ids.append(cid)
                if isinstance(c.get("rooms"), int):
                    cluster_budget[cid] = int(c["rooms"])
            else:
                err(f"{rel}: invalid cluster entry in zone shape: {c!r}")
                return 2
        cluster_set = set(cluster_ids)
        if not cluster_set:
            err(f"{rel}: zone shape has no clusters: {zone_shape_path.relative_to(REPO_ROOT)}")
            return 2

        rooms = doc.get("rooms")
        if not isinstance(rooms, list) or not rooms:
            err(f"{rel}: missing rooms list")
            return 2

        room_by_id: dict[str, dict] = {}
        rooms_by_cluster: dict[str, list[str]] = defaultdict(list)
        for r in rooms:
            if not isinstance(r, dict):
                err(f"{rel}: room entries must be mappings: {r!r}")
                return 2
            rid = r.get("id")
            if not isinstance(rid, str) or not rid:
                err(f"{rel}: room missing id: {r!r}")
                return 2
            if rid in room_by_id:
                err(f"{rel}: duplicate room id: {rid}")
                return 2
            cluster = r.get("cluster")
            if not isinstance(cluster, str) or cluster not in cluster_set:
                err(f"{rel}: room {rid}: unknown cluster: {cluster!r}")
                return 2
            room_by_id[rid] = r
            rooms_by_cluster[cluster].append(rid)

        start_room = doc.get("start_room")
        if start_room is not None:
            if not isinstance(start_room, str) or start_room not in room_by_id:
                err(f"{rel}: start_room must be a room id in this file: {start_room!r}")
                return 2

        # Exit validation.
        for rid, r in room_by_id.items():
            exits = r.get("exits") or []
            if exits is None:
                exits = []
            if not isinstance(exits, list):
                err(f"{rel}: room {rid}: exits must be a list")
                return 2
            for ex in exits:
                if not isinstance(ex, dict):
                    err(f"{rel}: room {rid}: exit entries must be mappings: {ex!r}")
                    return 2
                d = ex.get("dir")
                to = ex.get("to")
                if not isinstance(d, str) or not d.strip():
                    err(f"{rel}: room {rid}: exit missing dir: {ex!r}")
                    return 2
                if not isinstance(to, str) or not to.strip():
                    err(f"{rel}: room {rid}: exit missing to: {ex!r}")
                    return 2
                ln = ex.get("len", 1)
                if not isinstance(ln, int) or ln < 1:
                    err(f"{rel}: room {rid}: exit len must be int >= 1: {ex!r}")
                    return 2

                if to in room_by_id:
                    continue
                if to in portal_ids:
                    continue

                # Sealed placeholder exits (A02+ etc).
                state = ex.get("state")
                opens_area = ex.get("opens_area")
                if state == "sealed" or (isinstance(opens_area, str) and opens_area.startswith("A")):
                    continue

                err(f"{rel}: room {rid}: exit points to unknown target (not room/portal/sealed): {to!r}")
                return 2

        # Portal room validation (portal ids that belong to this zone).
        my_portals = [p for p in portals if p.get("zone_id") == zone_id]
        for p in my_portals:
            pid = p["id"]
            if pid not in room_by_id:
                err(f"{rel}: missing portal room for {pid} (room id must exist)")
                return 2
            pr = room_by_id[pid]
            ph = p.get("cluster_hint")
            if isinstance(ph, str) and pr.get("cluster") != ph:
                err(f"{rel}: portal room {pid}: cluster {pr.get('cluster')!r} must match overworld cluster_hint {ph!r}")
                return 2
            other = p.get("connects_to")
            if not isinstance(other, str) or other not in portal_ids:
                err(f"{rel}: portal {pid}: invalid connects_to: {other!r}")
                return 2

            expected_len = edge_len.get(tuple(sorted((pid, other))))
            if expected_len is None:
                err(f"{rel}: missing overworld edge length for portal pair {pid} <-> {other}")
                return 2

            exits = pr.get("exits") or []
            if not isinstance(exits, list):
                err(f"{rel}: portal room {pid}: exits must be a list")
                return 2
            ok = False
            for ex in exits:
                if not isinstance(ex, dict):
                    continue
                if ex.get("to") != other:
                    continue
                ln = ex.get("len", 1)
                if ln != expected_len:
                    err(f"{rel}: portal room {pid}: exit to {other} must have len={expected_len} (got {ln})")
                    return 2
                ok = True
                break
            if not ok:
                err(f"{rel}: portal room {pid}: missing exit to connects_to portal {other}")
                return 2

        # Cluster budget warnings (not hard failures).
        for cid, target in sorted(cluster_budget.items()):
            got = len(rooms_by_cluster.get(cid, []))
            if got < target:
                warn(f"{rel}: cluster {cid} under target rooms: got {got}, target {target}")

        # Reachability (warn only).
        if isinstance(start_room, str) and start_room in room_by_id:
            q = deque([start_room])
            seen: set[str] = {start_room}
            while q:
                cur = q.popleft()
                for ex in (room_by_id[cur].get("exits") or []):
                    if not isinstance(ex, dict):
                        continue
                    to = ex.get("to")
                    if not isinstance(to, str) or to not in room_by_id:
                        continue
                    if to not in seen:
                        seen.add(to)
                        q.append(to)
            if len(seen) != len(room_by_id):
                unreachable = sorted(set(room_by_id.keys()) - seen)
                warn(f"{rel}: unreachable rooms from start_room ({start_room}): {len(unreachable)}")

        ok_count += 1

    sys.stdout.write(f"ok: validated {ok_count} area file(s)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__('sys').argv[1:]))

