#!/usr/bin/env python3
"""
Proto coverage checker: do protoadventures cover all official clusters?

Source of truth for clusters: `docs/zone_beats.md`
Source of truth for proto clusters: YAML front matter in `protoadventures/*.md`

This is an authoring-time check used by `docs/areas_todo.md`.
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


def err(msg: str) -> None:
    sys.stderr.write(msg.rstrip() + "\n")


def parse_zone_beats() -> tuple[dict[str, list[str]], dict[str, str]]:
    """
    Returns:
      - clusters_by_zone_name: zone_name -> [CL_*]
      - zone_by_cluster: CL_* -> zone_name
    """

    text = ZONE_BEATS_MD.read_text(encoding="utf-8")
    clusters_by_zone: dict[str, list[str]] = {}
    zone_by_cluster: dict[str, str] = {}

    zone_name: str | None = None
    for line in text.splitlines():
        if line.startswith("### "):
            title = line[len("### ") :].strip()
            # Keep any parentheses in the actual zone name; strip only the trailing "(L..-L.., N rooms)".
            zone_name = re.sub(r"\s*\([^()]*\)\s*$", "", title).strip()
            clusters_by_zone.setdefault(zone_name, [])
            continue
        if zone_name and line.lstrip().startswith("| CL_"):
            cid = line.strip().split("|")[1].strip()
            clusters_by_zone[zone_name].append(cid)
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


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--show-ok", action="store_true", help="Also print zones with full coverage")
    args = ap.parse_args(argv)

    clusters_by_zone, zone_by_cluster = parse_zone_beats()
    all_clusters = set(zone_by_cluster.keys())

    covered_clusters: set[str] = set()
    unknown_clusters: dict[str, set[str]] = {}

    proto_files = sorted([p for p in PROTO_DIR.glob("*.md") if p.name not in {"README.md", "_TEMPLATE.md"}])
    for p in proto_files:
        fm = read_front_matter(p)
        cls = fm.get("clusters") or []
        if not isinstance(cls, list):
            continue
        for c in cls:
            if not isinstance(c, str):
                continue
            if c in all_clusters:
                covered_clusters.add(c)
            elif c.startswith("CL_"):
                unknown_clusters.setdefault(p.name, set()).add(c)

    missing_global = sorted(all_clusters - covered_clusters)
    if unknown_clusters:
        err("warning: protoadventures reference unknown cluster IDs:")
        for fname in sorted(unknown_clusters):
            err(f"  - {fname}: {sorted(unknown_clusters[fname])}")

    missing_by_zone: dict[str, list[str]] = {}
    for zname, cls in clusters_by_zone.items():
        miss = [c for c in cls if c not in covered_clusters]
        if miss:
            missing_by_zone[zname] = miss

    if missing_by_zone:
        err("missing cluster coverage:")
        for zname in sorted(missing_by_zone):
            err(f"  - {zname}: {missing_by_zone[zname]}")
        return 2

    if missing_global:
        # Shouldn't happen if missing_by_zone is empty, but keep a direct check.
        err(f"missing clusters: {missing_global}")
        return 2

    ok_zones = len(clusters_by_zone)
    ok_clusters = len(all_clusters)
    sys.stdout.write(f"ok: {ok_zones} zones, {ok_clusters} clusters covered by protoadventures\n")

    if args.show_ok:
        for zname in sorted(clusters_by_zone):
            sys.stdout.write(f"- {zname}: ok\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))

