# Area Iteration Loop (Zone Authoring)

Goal: translate A01 zones into stub "area shape" files that stay consistent across:

- `docs/zone_beats.md` (clusters + room budgets)
- `protoadventures/*.md` (what parties do)
- `docs/overworld_cartesian_layout.md` (portal coords + exit lengths)

This is intentionally still pre-engine: we are authoring planning YAML + validations so multiple agents can work in parallel without stepping on each other.

## What To Read (New Agent)

1. `docs/areas_todo.md` (pick a zone and see what is missing)
2. `docs/zone_beats.md` (official cluster IDs)
3. `docs/overworld_cartesian_layout.md` (authoritative portals + travel cost)
4. `protoadventures/README.md` (protoadventure format + SRD rule)
5. `docs/adventure_iteration.md` (how protoadventures are produced)

Optional context:

- `docs/area_summary.md` (themes)
- `docs/room_scale_plan.md` (budgets)
- `docs/area_files.md` (room graph area files)

## Artifacts

- `world/overworld.yaml`: portals + exit lengths exported from `docs/overworld_cartesian_layout.md`
- `world/overworld_pairs.tsv`: edge list with endpoint coords + `len` (tool/visualization friendly)
- `world/zones/<zone_id>.yaml`: one file per zone (bounds + portals + clusters)
- `locks/areas/<zone_id>.lock`: local lock files for parallel work (not committed)

## Status Flow (Recommended)

- `needs_proto`: missing cluster coverage; write/adjust protoadventures to cover the missing clusters
- `proto_ready`: all official clusters have at least one protoadventure touchpoint
- `stub`: `world/zones/<zone_id>.yaml` exists and validates (clusters + portals + bounds)
- `shaped`: zone YAML has planning hints (`cluster_edges`, `key_rooms`, `notes`, `hubs`)

Next stage (room graphs):

- `area_stub`: `world/areas/<zone_id>.yaml` exists and passes `just area-files-validate` (often under room budgets)
- `area_budgeted`: area file meets the zone’s cluster room targets (no under-target warnings)

Notes:

- `docs/areas_todo.md` is the shared scoreboard for these statuses.
- Don’t edit `docs/areas_todo.md` to claim work. Claim work by taking a lock.

## Fast Checks (No Guessing)

- Proto coverage vs official zone beats: `just proto-coverage`
- Overworld + zone shape validity: `just world-validate`
- Area files (room graphs): `just area-files-validate`

## Workflow (Per Zone)

Optional baseline (fast start):

- Generate/refresh stub zone files from docs + overworld YAML: `just zones-stubgen` (or `python3 scripts/zone_shape_stubgen.py`)
- Annotate/refresh derived `protoadventures:` lists inside zone stubs: `just zones-annotate-proto` (or `just zones-annotate-proto <zone_id>`)

1. Pick a zone in `docs/areas_todo.md`.
2. Acquire the lock:
   - `just area-lock <zone_id> <claimed_by>`
3. Export + validate the overworld spec:
   - `just overworld-export`
   - `just overworld-validate`
   - Optional: `just world-validate` (runs both overworld + zone shape validation)
4. Draft the zone shape file at `world/zones/<zone_id>.yaml`:
   - include zone metadata (A01, level band, target rooms)
   - list official clusters from `docs/zone_beats.md`
   - list portal IDs that live in this zone (from `world/overworld.yaml`)
   - define a simple global `bounds` rectangle that contains the zone's portal coords
   - if you add extra authoring fields (`notes`, `protoadventures`, key room lists, etc), keep them as planning hints (not engine requirements)
   - optional authoring hints (validated if present by `just areas-validate`):

```yaml
hubs:
  - HUB_SEWERS_JUNCTION

cluster_edges:
  - a: CL_SEWERS_ENTRY
    b: CL_SEWERS_JUNCTION

key_rooms:
  - id: R_SEW_JUNC_01
    cluster: CL_SEWERS_JUNCTION
    tags: [HUB_SEWERS_JUNCTION]
```
5. Validate everything:
   - `just areas-validate`
6. Release the lock:
   - `just area-unlock <zone_id> <claimed_by>`

## Next Stage: Area Files (Room Graphs)

When a zone is `shaped` and its protoadventures feel stable, translate it into an engine-facing room graph:

- Spec + workflow: `docs/area_files.md`
- Files live in: `world/areas/`

## Do This Now (10-Minute Loop)

```bash
zone_id="under_town_sewers"
claimed_by="yourname"

just area-lock "$zone_id" "$claimed_by"
just world-validate
just proto-coverage
just zones-annotate-proto "$zone_id"

# Edit: add planning notes, cluster adjacency hints, key rooms, etc.
# (Do not worry about engine format yet.)
${EDITOR:-vi} "world/zones/${zone_id}.yaml"

just areas-validate
just area-unlock "$zone_id" "$claimed_by"
```

## SRD 5.2.1 (CC BY 4.0) Rule

- Do not copy Wizards of the Coast text.
- If you directly use SRD 5.2.1 phrasing or numeric tables, add a row to:
  - `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`

Tabulation is a ledger table (ID, what you used, where it appears, attribution status). It’s intentionally boring and strict.
