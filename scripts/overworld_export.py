#!/usr/bin/env python3
"""
Export the overworld cartesian planning spec to machine-readable files.

Source of truth: `docs/overworld_cartesian_layout.md`

Outputs (repo-relative):
  - `world/overworld.yaml`         (zones, portals, edges)
  - `world/overworld_pairs.tsv`   (edge list with endpoint coords)
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SRC_MD = REPO_ROOT / "docs" / "overworld_cartesian_layout.md"
WORLD_DIR = REPO_ROOT / "world"
OUT_YAML = WORLD_DIR / "overworld.yaml"
OUT_TSV = WORLD_DIR / "overworld_pairs.tsv"
OUT_SUMMARY_JSON = WORLD_DIR / "overworld_summary.json"


def utc_now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()


def slugify(name: str) -> str:
    s = name.lower().strip()
    s = re.sub(r"[^a-z0-9]+", "_", s)
    s = re.sub(r"_+", "_", s).strip("_")
    if not s:
        s = "zone"
    return s


def parse_markdown_tables(text: str) -> list[tuple[list[str], list[list[str]]]]:
    lines = text.splitlines()
    tables: list[tuple[list[str], list[list[str]]]] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.lstrip().startswith("|"):
            i += 1
            continue
        # Potential header row.
        header_cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if i + 1 >= len(lines):
            i += 1
            continue
        sep = lines[i + 1]
        if not sep.lstrip().startswith("|"):
            i += 1
            continue
        sep_cells = [c.strip() for c in sep.strip().strip("|").split("|")]
        if len(sep_cells) != len(header_cells):
            i += 1
            continue
        # Separator line is mostly '-' ':' and spaces.
        if not all(re.fullmatch(r"[-: ]+", c or " ") for c in sep_cells):
            i += 1
            continue

        i += 2
        rows: list[list[str]] = []
        while i < len(lines) and lines[i].lstrip().startswith("|"):
            row_cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
            if len(row_cells) == len(header_cells) and any(cell for cell in row_cells):
                rows.append(row_cells)
            i += 1
        tables.append((header_cells, rows))
    return tables


_COORD_RE = re.compile(r"^\(\s*(-?\d+)\s*,\s*(-?\d+)\s*\)$")


def parse_coord(s: str) -> tuple[int, int]:
    m = _COORD_RE.match(s.strip())
    if not m:
        raise ValueError(f"invalid coord: {s!r}")
    return int(m.group(1)), int(m.group(2))


def yaml_quote(s: str) -> str:
    # Minimal safe quoting for YAML double-quoted strings.
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def dump_overworld_yaml(
    *,
    zones: list[dict],
    portals: list[dict],
    edges: list[dict],
    generated_at_utc: str,
    source_path: str,
) -> str:
    out: list[str] = []
    out.append("version: 1")
    out.append(f"generated_at_utc: {yaml_quote(generated_at_utc)}")
    out.append(f"generated_from: {yaml_quote(source_path)}")
    out.append("units:")
    out.append("  coord_unit: planning-grid")
    out.append("  len_unit: movement-cost")
    out.append("zones:")
    for z in zones:
        out.append(f"  - id: {z['id']}")
        out.append(f"    name: {yaml_quote(z['name'])}")
        out.append("    anchor:")
        out.append(f"      x: {z['anchor']['x']}")
        out.append(f"      y: {z['anchor']['y']}")
    out.append("portals:")
    for p in portals:
        out.append(f"  - id: {p['id']}")
        out.append(f"    zone_id: {p['zone_id']}")
        out.append(f"    zone_name: {yaml_quote(p['zone_name'])}")
        out.append(f"    cluster_hint: {p['cluster_hint']}")
        out.append(f"    connects_to: {p['connects_to']}")
        out.append("    pos:")
        out.append(f"      x: {p['pos']['x']}")
        out.append(f"      y: {p['pos']['y']}")
    out.append("edges:")
    for e in edges:
        out.append(f"  - a: {e['a']}")
        out.append(f"    b: {e['b']}")
        out.append(f"    len: {e['len']}")
        if e.get("notes"):
            out.append(f"    notes: {yaml_quote(e['notes'])}")
    out.append("")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=str(SRC_MD), help="Path to overworld_cartesian_layout.md")
    ap.add_argument("--out-yaml", default=str(OUT_YAML))
    ap.add_argument("--out-tsv", default=str(OUT_TSV))
    ap.add_argument("--out-summary-json", default=str(OUT_SUMMARY_JSON))
    args = ap.parse_args(argv)

    src_path = Path(args.src)
    text = src_path.read_text(encoding="utf-8")
    tables = parse_markdown_tables(text)

    anchors_header = ["Zone", "Anchor (x,y)"]
    portals_header = ["Portal ID", "Zone", "Cluster hint", "Connects to", "(x,y)"]
    lengths_header = ["From", "To", "len", "Notes"]

    anchors_rows: list[list[str]] | None = None
    portals_rows: list[list[str]] | None = None
    lengths_rows: list[list[str]] | None = None
    for header, rows in tables:
        if header == anchors_header:
            anchors_rows = rows
        elif header == portals_header:
            portals_rows = rows
        elif header == lengths_header:
            lengths_rows = rows

    if anchors_rows is None:
        raise SystemExit(f"missing anchors table with header: {anchors_header}")
    if portals_rows is None:
        raise SystemExit(f"missing portals table with header: {portals_header}")
    if lengths_rows is None:
        raise SystemExit(f"missing lengths table with header: {lengths_header}")

    # Zones in declared order.
    zones: list[dict] = []
    zone_id_by_name: dict[str, str] = {}
    used_zone_ids: set[str] = set()
    for zone_name, anchor_str in anchors_rows:
        base = slugify(zone_name)
        zone_id = base
        n = 2
        while zone_id in used_zone_ids:
            zone_id = f"{base}_{n}"
            n += 1
        used_zone_ids.add(zone_id)
        zone_id_by_name[zone_name] = zone_id
        ax, ay = parse_coord(anchor_str)
        zones.append({"id": zone_id, "name": zone_name, "anchor": {"x": ax, "y": ay}})

    portals: list[dict] = []
    portal_by_id: dict[str, dict] = {}
    for portal_id, zone_name, cluster_hint, connects_to, pos_str in portals_rows:
        if zone_name not in zone_id_by_name:
            raise SystemExit(f"portal {portal_id} references unknown zone: {zone_name!r}")
        x, y = parse_coord(pos_str)
        p = {
            "id": portal_id,
            "zone_id": zone_id_by_name[zone_name],
            "zone_name": zone_name,
            "cluster_hint": cluster_hint,
            "connects_to": connects_to,
            "pos": {"x": x, "y": y},
        }
        if portal_id in portal_by_id:
            raise SystemExit(f"duplicate portal id: {portal_id}")
        portal_by_id[portal_id] = p
        portals.append(p)

    edges: list[dict] = []
    seen_edge_keys: set[tuple[str, str]] = set()
    for a, b, len_str, notes in lengths_rows:
        if a not in portal_by_id:
            raise SystemExit(f"edge references unknown portal: {a}")
        if b not in portal_by_id:
            raise SystemExit(f"edge references unknown portal: {b}")
        ln = int(len_str.strip())
        key = tuple(sorted((a, b)))
        if key in seen_edge_keys:
            raise SystemExit(f"duplicate edge (undirected): {a} <-> {b}")
        seen_edge_keys.add(key)
        e = {"a": a, "b": b, "len": ln}
        if notes.strip():
            e["notes"] = notes.strip()
        edges.append(e)

    WORLD_DIR.mkdir(parents=True, exist_ok=True)

    # YAML
    generated_at = utc_now_iso()
    yaml_text = dump_overworld_yaml(
        zones=zones,
        portals=portals,
        edges=edges,
        generated_at_utc=generated_at,
        source_path=str(src_path.relative_to(REPO_ROOT)),
    )
    Path(args.out_yaml).write_text(yaml_text, encoding="utf-8")

    # TSV pairs
    tsv_lines = [
        "\t".join(
            [
                "a",
                "b",
                "len",
                "ax",
                "ay",
                "bx",
                "by",
                "a_zone",
                "b_zone",
                "notes",
            ]
        )
    ]
    for e in edges:
        pa = portal_by_id[e["a"]]
        pb = portal_by_id[e["b"]]
        tsv_lines.append(
            "\t".join(
                [
                    e["a"],
                    e["b"],
                    str(e["len"]),
                    str(pa["pos"]["x"]),
                    str(pa["pos"]["y"]),
                    str(pb["pos"]["x"]),
                    str(pb["pos"]["y"]),
                    pa["zone_name"],
                    pb["zone_name"],
                    e.get("notes", ""),
                ]
            )
        )
    Path(args.out_tsv).write_text("\n".join(tsv_lines) + "\n", encoding="utf-8")

    # Summary JSON (handy for tools)
    zone_portal_counts: dict[str, int] = {}
    for p in portals:
        zone_portal_counts[p["zone_id"]] = zone_portal_counts.get(p["zone_id"], 0) + 1
    summary = {
        "generated_at_utc": generated_at,
        "zones": [{"id": z["id"], "name": z["name"], "portal_count": zone_portal_counts.get(z["id"], 0)} for z in zones],
        "portal_count": len(portals),
        "edge_count": len(edges),
    }
    Path(args.out_summary_json).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(__import__("sys").argv[1:]))

