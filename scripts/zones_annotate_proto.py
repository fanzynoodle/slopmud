#!/usr/bin/env python3
"""
Annotate `world/zones/*.yaml` with a derived `protoadventures:` list.

Why: makes it easy for area authors to see which protoadventures constrain a zone.

Derivation:
  - Parse official cluster IDs from `docs/zone_beats.md`
  - Parse protoadventure YAML front matter `clusters: [...]`
  - A protoadventure belongs to a zone if it references any cluster in that zone

This does not touch lock files and does not require taking a lock; it is purely derived.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
ZONE_BEATS_MD = REPO_ROOT / "docs" / "zone_beats.md"
PROTO_DIR = REPO_ROOT / "protoadventures"
ZONES_DIR = REPO_ROOT / "world" / "zones"


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def parse_zone_beats() -> tuple[dict[str, set[str]], dict[str, str]]:
    """
    Returns:
      - clusters_by_zone_name: zone_name -> {CL_*}
      - zone_name_by_cluster: CL_* -> zone_name
    """

    text = ZONE_BEATS_MD.read_text(encoding="utf-8")
    clusters_by_zone: dict[str, set[str]] = {}
    zone_by_cluster: dict[str, str] = {}

    zone_name: str | None = None
    for line in text.splitlines():
        if line.startswith("### "):
            title = line[len("### ") :].strip()
            zone_name = re.sub(r"\s*\([^()]*\)\s*$", "", title).strip()
            clusters_by_zone.setdefault(zone_name, set())
            continue
        if zone_name and line.lstrip().startswith("| CL_"):
            cid = line.strip().split("|")[1].strip()
            clusters_by_zone[zone_name].add(cid)
            zone_by_cluster[cid] = zone_name

    return clusters_by_zone, zone_by_cluster


def read_front_matter(path: Path) -> dict:
    text = path.read_text(encoding="utf-8", errors="replace")
    if not text.startswith("---"):
        return {}
    parts = text.split("\n---", 2)
    if len(parts) < 2:
        return {}
    raw = parts[0].lstrip("-").strip()
    try:
        data = yaml.safe_load(raw) or {}
    except Exception:
        return {}
    if not isinstance(data, dict):
        return {}
    return data


def replace_top_level_block(*, text: str, key: str, new_block: str) -> tuple[str, bool]:
    """
    Replace a top-level YAML block (e.g. "protoadventures:") with `new_block`.
    If the block doesn't exist, append `new_block` at EOF.
    """

    lines = text.splitlines(keepends=True)
    start = None
    for i, line in enumerate(lines):
        if line.startswith(f"{key}:"):
            start = i
            break
    if start is None:
        # Append with a blank line separator if needed.
        out = text
        if out and not out.endswith("\n"):
            out += "\n"
        if out and not out.endswith("\n\n"):
            out += "\n"
        out += new_block
        if not out.endswith("\n"):
            out += "\n"
        return out, True

    # Find end of block: next non-empty top-level key.
    end = start + 1
    while end < len(lines):
        ln = lines[end]
        if not ln.strip():
            end += 1
            continue
        if ln[0].isspace():
            end += 1
            continue
        # New top-level key
        break

    out_lines = []
    out_lines.extend(lines[:start])
    out_lines.append(new_block if new_block.endswith("\n") else (new_block + "\n"))
    out_lines.extend(lines[end:])
    return "".join(out_lines), True


def format_list_block(key: str, items: list[str]) -> str:
    out = [f"{key}:"]
    for it in items:
        out.append(f"  - {it}")
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("zone_id", nargs="?", help="Only annotate one zone_id (optional)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args(argv)

    _, zone_by_cluster = parse_zone_beats()

    # zone_id -> set(adventure_id)
    protos_by_zone_id: dict[str, set[str]] = {}

    proto_files = sorted([p for p in PROTO_DIR.glob("*.md") if p.name not in {"README.md", "_TEMPLATE.md"}])
    for p in proto_files:
        fm = read_front_matter(p)
        adv_id = fm.get("adventure_id") or p.stem
        cls = fm.get("clusters") or []
        if not isinstance(adv_id, str) or not isinstance(cls, list):
            continue
        zones_touched: set[str] = set()
        for c in cls:
            if not isinstance(c, str):
                continue
            zname = zone_by_cluster.get(c)
            if zname is None:
                continue
            # Derive zone_id from overworld export slugs to avoid duplicating slug rules.
            # `world/overworld.yaml` uses the slugified zone name already.
            zones_touched.add(zname)
        # We'll map zone name -> zone_id by reading world/overworld.yaml once.

        # Store by zone name for now; remap after loading overworld.yaml.
        for zname in zones_touched:
            protos_by_zone_id.setdefault(zname, set()).add(adv_id)

    overworld = yaml.safe_load((REPO_ROOT / "world" / "overworld.yaml").read_text(encoding="utf-8"))
    zones = overworld.get("zones") or []
    zone_id_by_name = {z["name"]: z["id"] for z in zones}

    # Remap: real zone_id -> set(proto ids)
    mapped: dict[str, set[str]] = {}
    for zname, advs in protos_by_zone_id.items():
        zid = zone_id_by_name.get(zname)
        if zid is None:
            err(f"warning: zone name not found in overworld.yaml: {zname!r}")
            continue
        mapped.setdefault(zid, set()).update(advs)

    zone_files = sorted(ZONES_DIR.glob("*.yaml"))
    if args.zone_id:
        zone_files = [p for p in zone_files if p.stem == args.zone_id]
        if not zone_files:
            err(f"no such zone file: {args.zone_id}")
            return 2

    changed = 0
    for path in zone_files:
        zid = path.stem
        advs = sorted(mapped.get(zid, set()))
        block = format_list_block("protoadventures", advs)
        original = path.read_text(encoding="utf-8")
        updated, did = replace_top_level_block(text=original, key="protoadventures", new_block=block)
        if did and updated != original:
            changed += 1
            if not args.dry_run:
                path.write_text(updated, encoding="utf-8")

    sys.stdout.write(f"ok: annotated {len(zone_files)} zone file(s); changed {changed}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__('sys').argv[1:]))

