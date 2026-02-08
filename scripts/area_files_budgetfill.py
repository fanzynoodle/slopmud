#!/usr/bin/env python3
"""
Fill area files (room graphs) under `world/areas/` up to the per-cluster room
budgets defined in `world/zones/<zone_id>.yaml`.

This is a mechanical helper intended to unblock parallel authorship:

- It appends deterministic "filler loop" rooms per under-budget cluster.
- It adds exactly one new exit from an existing anchor room in that cluster
  into the filler loop so the new rooms are reachable from `start_room`.

It does *not* attempt to make good content. Expect to refine by hand.
"""

from __future__ import annotations

import argparse
import sys
from collections import Counter, defaultdict
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
OVERWORLD_YAML = REPO_ROOT / "world" / "overworld.yaml"
ZONES_DIR = REPO_ROOT / "world" / "zones"
AREAS_DIR = REPO_ROOT / "world" / "areas"


class Dumper(yaml.SafeDumper):
    pass


def _str_presenter(dumper: yaml.SafeDumper, data: str) -> yaml.nodes.ScalarNode:
    # Keep long room prose readable (avoid single-quoted strings with embedded newlines).
    if "\n" in data:
        return dumper.represent_scalar("tag:yaml.org,2002:str", data, style="|")
    return dumper.represent_scalar("tag:yaml.org,2002:str", data)


Dumper.add_representer(str, _str_presenter)


STOP_LAST_SEGMENTS = {
    # Too-generic trailing segments: prefer a different token if possible.
    "FIELD",
    "FIELDS",
    "LOOP",
    "LOOPS",
    "RUN",
    "RUNS",
    "WING",
    "WINGS",
}


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def _singular(seg: str) -> str:
    if seg.endswith("SS"):
        return seg
    if seg.endswith("S") and len(seg) > 1:
        return seg[:-1]
    return seg


def _cluster_core(cluster_id: str) -> str:
    return cluster_id[3:] if cluster_id.startswith("CL_") else cluster_id


def _common_cluster_prefix_token(cluster_ids: list[str]) -> str:
    cores = [_cluster_core(cid) for cid in cluster_ids if isinstance(cid, str)]
    if not cores:
        return ""
    first_tokens = []
    for c in cores:
        parts = c.split("_", 1)
        first_tokens.append(parts[0] if parts else c)
    token = first_tokens[0]
    return token if all(t == token for t in first_tokens) else ""


def _zone_token_from_rooms(room_ids: list[str], cluster_prefix_token: str, zone_id: str) -> str:
    # Prefer the dominant token used in R_* ids (R_<TOKEN>_...).
    tokens: list[str] = []
    for rid in room_ids:
        if not isinstance(rid, str) or not rid.startswith("R_"):
            continue
        parts = rid.split("_")
        if len(parts) >= 2 and parts[1]:
            tokens.append(parts[1])
    if tokens:
        return Counter(tokens).most_common(1)[0][0]

    # Next-best: use the cluster prefix (e.g., MEADOW / ORCHARD / SEWERS).
    if cluster_prefix_token:
        return cluster_prefix_token

    # Fallback: slug the zone id (first token only to keep ids short-ish).
    slug = "".join(ch if ch.isalnum() else "_" for ch in zone_id.upper())
    slug = "_".join([p for p in slug.split("_") if p])
    return slug.split("_", 1)[0] if slug else "ZONE"


def _token_from_existing_ids(zone_token: str, cluster_room_ids: list[str]) -> str | None:
    """
    If a cluster already has multiple R_<ZONE>_<TOKEN>_<NN> ids, reuse TOKEN.
    Requiring >=2 avoids cases where a single key room uses a token we don't
    actually want to extend (e.g., TRAIL_01 in two different clusters).
    """
    counts: Counter[str] = Counter()
    for rid in cluster_room_ids:
        if not isinstance(rid, str) or not rid.startswith(f"R_{zone_token}_"):
            continue
        parts = rid.split("_")
        if len(parts) < 4 or parts[0] != "R" or parts[1] != zone_token:
            continue
        if not parts[-1].isdigit():
            continue
        token = "_".join(parts[2:-1]).strip("_")
        if token:
            # Avoid extending very-specific id families like VALVE1_01 / VALVE2_01.
            if token[-1].isdigit():
                continue
            counts[token] += 1

    if not counts:
        return None

    token, n = counts.most_common(1)[0]
    return token if n >= 2 else None


def _token_from_cluster(cluster_local: str) -> str:
    parts = [p for p in cluster_local.split("_") if p]
    if not parts:
        return "FILL"

    last = _singular(parts[-1])
    first = _singular(parts[0])

    if last not in STOP_LAST_SEGMENTS:
        return last
    if first not in STOP_LAST_SEGMENTS:
        return first

    # Worst case: keep the full local name (still deterministic).
    return _singular(cluster_local)


def _cluster_local_name(cluster_id: str, cluster_prefix_token: str) -> str:
    core = _cluster_core(cluster_id)
    if cluster_prefix_token and core.startswith(cluster_prefix_token + "_"):
        core = core[len(cluster_prefix_token) + 1 :]
    return core


def _pick_anchor_room(
    cid: str, room_by_id: dict[str, dict], rooms_by_cluster: dict[str, list[str]], zone_shape: dict
) -> str | None:
    # Prefer key rooms from the zone shape if they exist in the area file.
    for kr in zone_shape.get("key_rooms") or []:
        if not isinstance(kr, dict):
            continue
        if kr.get("cluster") != cid:
            continue
        rid = kr.get("id")
        if isinstance(rid, str) and rid in room_by_id:
            return rid

    # Next: prefer a portal room in this cluster.
    for rid in rooms_by_cluster.get(cid, []):
        if isinstance(rid, str) and rid.startswith("P_"):
            return rid

    # Finally: first room in the cluster.
    ids = rooms_by_cluster.get(cid, [])
    return ids[0] if ids else None


def _pick_zone_hub(room_by_id: dict[str, dict], doc: dict, zone_shape: dict) -> str | None:
    # Prefer a HUB_* key room if present.
    for kr in zone_shape.get("key_rooms") or []:
        if not isinstance(kr, dict):
            continue
        rid = kr.get("id")
        tags = kr.get("tags") or []
        if not isinstance(rid, str) or rid not in room_by_id:
            continue
        if isinstance(tags, list) and any(isinstance(t, str) and t.startswith("HUB_") for t in tags):
            return rid

    # Next: the explicit start_room if present.
    start_room = doc.get("start_room")
    if isinstance(start_room, str) and start_room in room_by_id:
        return start_room

    # Finally: first room in file.
    return next(iter(room_by_id.keys()), None)


def _max_numeric_suffix(prefix: str, existing_ids: set[str]) -> int:
    mx = 0
    for rid in existing_ids:
        if not rid.startswith(prefix):
            continue
        suf = rid[len(prefix) :]
        if suf.isdigit():
            mx = max(mx, int(suf))
    return mx


def _ensure_exit(room: dict, ex: dict) -> None:
    exits = room.get("exits")
    if exits is None:
        exits = []
    if not isinstance(exits, list):
        exits = []
    # Avoid exact duplicates.
    for e in exits:
        if isinstance(e, dict) and e.get("dir") == ex.get("dir") and e.get("to") == ex.get("to"):
            room["exits"] = exits
            return
    exits.append(ex)
    room["exits"] = exits


def _cluster_label(cluster_local: str) -> str:
    return cluster_local.replace("_", " ").title()


def budgetfill_zone(zone_id: str, overworld: dict, zones_dir: Path, areas_dir: Path, dry_run: bool) -> int:
    zone_shape_path = zones_dir / f"{zone_id}.yaml"
    if not zone_shape_path.exists():
        err(f"error: missing zone shape file: {zone_shape_path.relative_to(REPO_ROOT)}")
        return 2
    zone_shape = yaml.safe_load(zone_shape_path.read_text(encoding="utf-8")) or {}

    clusters_raw = zone_shape.get("clusters") or []
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

    if not cluster_ids:
        err(f"error: no clusters in zone shape: {zone_shape_path.relative_to(REPO_ROOT)}")
        return 2

    cluster_prefix_token = _common_cluster_prefix_token(cluster_ids)

    area_path = areas_dir / f"{zone_id}.yaml"
    if not area_path.exists():
        err(f"error: missing area file (run stubgen first): {area_path.relative_to(REPO_ROOT)}")
        return 2
    doc = yaml.safe_load(area_path.read_text(encoding="utf-8"))
    if not isinstance(doc, dict):
        err(f"error: invalid area file (expected mapping): {area_path.relative_to(REPO_ROOT)}")
        return 2

    rooms = doc.get("rooms")
    if not isinstance(rooms, list) or not rooms:
        err(f"error: {area_path.relative_to(REPO_ROOT)} missing rooms list")
        return 2

    room_by_id: dict[str, dict] = {}
    rooms_by_cluster: dict[str, list[str]] = defaultdict(list)
    for r in rooms:
        if not isinstance(r, dict):
            continue
        rid = r.get("id")
        cid = r.get("cluster")
        if isinstance(rid, str) and isinstance(cid, str):
            room_by_id[rid] = r
            rooms_by_cluster[cid].append(rid)

    existing_ids = set(room_by_id.keys())
    zone_token = _zone_token_from_rooms(list(existing_ids), cluster_prefix_token, zone_id)
    zone_hub_id = _pick_zone_hub(room_by_id, doc, zone_shape)

    added_total = 0
    for cid, target in sorted(cluster_budget.items()):
        current = len(rooms_by_cluster.get(cid, []))
        need = target - current
        if need <= 0:
            continue

        anchor_id = _pick_anchor_room(cid, room_by_id, rooms_by_cluster, zone_shape)
        cluster_local = _cluster_local_name(cid, cluster_prefix_token)

        token = _token_from_existing_ids(zone_token, rooms_by_cluster.get(cid, []))
        if token is None:
            token = _token_from_cluster(cluster_local)

        prefix = f"R_{zone_token}_{token}_"
        mx = _max_numeric_suffix(prefix, existing_ids)
        width = max(2, len(str(mx + max(1, need))))

        # If a cluster has zero rooms, create a seed anchor and connect it from a zone hub.
        if anchor_id is None:
            if not isinstance(zone_hub_id, str) or zone_hub_id not in room_by_id:
                err(f"error: {zone_id}: cannot pick zone hub to seed empty cluster {cid}")
                return 2

            seed_num = mx + 1
            seed_id = prefix + str(seed_num).zfill(width)
            while seed_id in existing_ids:
                seed_num += 1
                seed_id = prefix + str(seed_num).zfill(width)

            if not dry_run:
                hub = room_by_id[zone_hub_id]
                _ensure_exit(hub, {"dir": f"to_{token.lower()}", "to": seed_id, "len": 1})

                label = _cluster_label(cluster_local)
                rooms.append(
                    {
                        "id": seed_id,
                        "name": f"{label} Seed",
                        "cluster": cid,
                        "tags": [f"seed.{cid}"],
                        "desc": f"Seed room for {cid}. TODO: replace with themed content.",
                        "exits": [{"dir": "back", "to": zone_hub_id, "len": 1}],
                    }
                )

                room_by_id[seed_id] = rooms[-1]
                rooms_by_cluster[cid].append(seed_id)

            existing_ids.add(seed_id)
            added_total += 1
            anchor_id = seed_id
            need -= 1
            if need <= 0:
                continue

        filler_ids: list[str] = []
        next_num = mx + 1
        while len(filler_ids) < need:
            rid = prefix + str(next_num).zfill(width)
            next_num += 1
            if rid in existing_ids:
                continue
            filler_ids.append(rid)
            existing_ids.add(rid)

        if not dry_run:
            anchor = room_by_id[anchor_id]
            _ensure_exit(
                anchor,
                {"dir": f"wander_{token.lower()}", "to": filler_ids[0], "len": 1},
            )

            label = _cluster_label(cluster_local)
            for i, rid in enumerate(filler_ids):
                back_to = anchor_id if i == 0 else filler_ids[i - 1]
                ahead_to = anchor_id if i == len(filler_ids) - 1 else filler_ids[i + 1]
                rooms.append(
                    {
                        "id": rid,
                        "name": f"{label} Loop",
                        "cluster": cid,
                        "tags": [f"filler.{cid}"],
                        "desc": f"Filler room for {cid}. TODO: replace with themed content.",
                        "exits": [
                            {"dir": "back", "to": back_to, "len": 1},
                            {"dir": "ahead", "to": ahead_to, "len": 1},
                        ],
                    }
                )

        added_total += need

    if dry_run:
        sys.stdout.write(f"{zone_id}: would add {added_total} filler rooms\n")
        return 0

    doc["rooms"] = rooms
    area_path.write_text(yaml.dump(doc, Dumper=Dumper, sort_keys=False, width=120), encoding="utf-8")
    sys.stdout.write(f"{zone_id}: added {added_total} filler rooms\n")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--overworld", default=str(OVERWORLD_YAML))
    ap.add_argument("--zones-dir", default=str(ZONES_DIR))
    ap.add_argument("--areas-dir", default=str(AREAS_DIR))
    ap.add_argument("--zone-id", action="append", default=[], help="Budgetfill only this zone_id (repeatable)")
    ap.add_argument("--all", action="store_true", help="Budgetfill every area file found in world/areas/")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args(argv)

    overworld = yaml.safe_load(Path(args.overworld).read_text(encoding="utf-8"))
    zones_dir = Path(args.zones_dir)
    areas_dir = Path(args.areas_dir)

    if args.all:
        zone_ids = sorted([p.stem for p in areas_dir.glob("*.yaml")])
    else:
        zone_ids = list(args.zone_id)

    if not zone_ids:
        err("error: pass --all or at least one --zone-id")
        return 2

    rc = 0
    for zid in zone_ids:
        rc = max(rc, budgetfill_zone(zid, overworld, zones_dir, areas_dir, dry_run=bool(args.dry_run)))
        if rc != 0:
            return rc
    return rc


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))
