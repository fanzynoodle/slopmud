# Area Files (Room Graphs)

This is the next step after zone shaping (`world/zones/*.yaml`): translating a zone into an engine-facing room graph under `world/areas/`.

Goals:

- Make protoadventures concrete: real rooms + exits, not just beats.
- Keep topology stable: portal exits match `world/overworld.yaml` and can be validated.
- Stay parallel-safe: authors claim zones via lock files, not by editing shared docs.

## What To Read (New Agent)

1. `docs/area_iteration.md` (zone shaping + locks)
2. `docs/areas_todo.md` (pick a zone)
3. `docs/zone_beats.md` (official clusters + target room counts)
4. `docs/overworld_cartesian_layout.md` (authoritative portals + exit lengths)
5. `world/overworld.yaml` (generated portals + exit lengths; do not hand-edit)
6. `protoadventures/*.md` for the zone you are translating

## Area File Location

- One file per zone: `world/areas/<zone_id>.yaml`
- Example: `world/areas/newbie_school.yaml`

## Minimal Schema (v1)

```yaml
version: 1
zone_id: newbie_school
zone_name: "Newbie School"
area_id: A01
level_band: [1, 2]

# Room ID to spawn new players into (for this zone’s entry experience).
start_room: R_NS_ORIENT_01

# Rooms in this zone (cluster assignment is required so we can track budgets).
rooms:
  - id: R_NS_ORIENT_01
    name: "Orientation Wing"
    cluster: CL_NS_ORIENTATION
    tags: [HUB_NS_ORIENTATION]
    desc: |
      Short, original prose. No WotC text. (Player-facing: do not put TODOs here.)
    exits:
      - dir: east
        to: R_NS_ORIENT_02
        len: 1

  # Portal rooms are explicit rooms with IDs that match overworld portal IDs.
  - id: P_NS_TOWN
    name: "Town Gate Transfer"
    cluster: CL_NS_ORIENTATION
    tags: [portal.P_NS_TOWN]
    exits:
      - dir: north
        to: P_TOWN_NS
        len: 1
```

Conventions:

- Room IDs are globally unique strings. Prefer stable, boring IDs.
- `cluster` must be one of the zone’s official cluster IDs from `world/zones/<zone_id>.yaml`.
- `tags` are free-form planning hooks (`HUB_*`, `setpiece.*`, `portal.*`, etc).
- `len` is movement cost (planning now; engine will use it later). Default is `1`.
- `state: sealed` blocks an exit until the engine is ready (or until a `gate:` condition opens it).
- `gate:` (optional) is a small expression checked at movement time. Supported forms:
  - `gate.some_key` (truthy/nonzero)
  - `q.some.counter>=3` (numeric compare; `>=`, `<=`, `<`, `>`)
  - `q.some.state==complete` / `!=` (string or numeric equality)

## Validation Rules (current)

Run:

- `just world-validate` (overworld + zone shapes)
- `just area-files-validate` (area files)
- `just area-files-report` (maturity snapshot: filler/setpiece counts)
- `just area-files-report <zone_id>` (single zone)
- `just area-files-report "" tsv` (all zones, TSV)

If you changed overworld portals or exit lengths:

- Edit `docs/overworld_cartesian_layout.md` (source of truth).
- Regenerate `world/overworld.yaml` + `world/overworld_pairs.tsv`: `just overworld-export`
- Validate: `just overworld-validate` (or `just world-validate`)

Area file validation enforces:

- Room IDs are unique.
- Every exit points to:
  - an in-file room ID, OR
  - an overworld portal ID (`P_*`), OR
  - a sealed placeholder (`state: sealed` with `opens_area: A##`).
- Every portal room:
  - exists as a room in the correct zone,
  - has an exit to its `connects_to` portal,
  - uses `len` that matches `world/overworld.yaml` for that portal edge.
- Cluster room counts are compared to the zone shape budgets (warn if under target).

Status mapping (for `docs/areas_todo.md`):

- `area_stub`: file exists and validates, but may be under cluster budgets
- `area_budgeted`: no under-target warnings for this zone (budgets met)

## Parallel Workflow (Per Zone)

1. Take the lock:
   - `just area-lock <zone_id> <claimed_by>`
2. Draft or edit `world/areas/<zone_id>.yaml`.
3. Validate:
   - `just world-validate`
   - `just area-files-validate`
4. Release the lock:
   - `just area-unlock <zone_id> <claimed_by>`

## Integrating A Protoadventure (Checklist)

When translating `protoadventures/<adventure_id>.md` into `world/areas/<zone_id>.yaml`, aim for a room graph that makes the proto’s linear run "true" without turning the whole zone into a star hub.

Checklist:

- The proto's key `R_*` room IDs exist in the area file (same IDs).
- The proto's spine path exists as a clean walk (hub -> spokes -> gated door -> boss/choice -> return).
- Regroup hubs are explicit rooms (not "portal rooms connect to everything").
- Quest gates are represented as exits with `state: sealed` + `gate: ...` (even if the engine will enforce more later).
- If a proto unlocks travel between zones, gate the portal edge on both ends (see below).

## Cross-Zone Edits (Portal Gating)

If you add a `gate:` condition to a portal connection (quest unlock), make it symmetric:

- In zone A, gate the portal room's exit that goes to the `connects_to` portal.
- In zone B, gate the portal room's exit that goes to the `connects_to` portal.

This prevents "one-way" accidental access (e.g., you can't enter from the far side before the unlock).

Process note:

- If you need to touch multiple zones (to gate both sides), take locks for all affected zones before editing.

After budgeting:

- If you hit `area_budgeted`, update the zone row in `docs/areas_todo.md` (minimal edit: one row only).
- Before playtesting, remove player-facing placeholders (filler/seed/anchor stubs):
  - `just area-files-themify <zone_id>` (or `just area-files-themify` for all zones)

## Do This Now (10-Minute Loop)

```bash
zone_id="under_town_sewers"
claimed_by="yourname"

just area-lock "$zone_id" "$claimed_by" "draft area file"
${EDITOR:-vi} "world/areas/${zone_id}.yaml"

just world-validate
just area-files-validate
just area-unlock "$zone_id" "$claimed_by"
```

Tips:

- Start by adding the zone’s portal rooms (`P_*`). Validation requires them.
- Portal exits must include an exit to `connects_to` with `len` matching `world/overworld.yaml`.
- If you just want a starting scaffold (portals + key rooms + reachability wiring), run `just area-files-stubgen <zone_id>`.
- If you want fast filler to hit cluster room budgets (then refine by hand), run `just area-files-budgetfill <zone_id>`.
- Budgetfill-generated placeholders are tagged `filler.<cluster_id>` (and sometimes `seed.<cluster_id>`); search and replace those first.
- If you want to remove placeholder room text (TODOs/filler stubs) before playtesting, run `just area-files-themify <zone_id>`.

## Text + Licensing Rule (SRD 5.2.1 CC BY 4.0)

- Do not copy Wizards of the Coast text.
- If you quote or closely paraphrase SRD 5.2.1 phrasing (or copy numeric tables/DCs/ranges), add a row to:
  - `reference/gaming-systems/dnd5e-srd-5.2.1-cc/TABULATION.md`

## Replacing Filler Rooms (Fast)

Goal: keep topology and cluster budgets stable while turning placeholder loops into readable landmarks.

1. Find placeholder rooms:
   - `rg -n 'filler\\.' "world/areas/${zone_id}.yaml"`
2. For each room tagged `filler.*`:
   - Keep `id`, `cluster`, and `exits` unchanged (avoid breaking links/budgets).
   - Replace `name` + `desc` with specific, player-facing landmarks (2-4 short lines).
   - Remove the `filler.*` tag entirely (delete `tags:` if it becomes empty).
3. Validate:
   - `just area-files-validate` (and `just world-validate` if you touched portals)
4. Quick smoke test:
   - `just dev-run` then `warp <room_id>` and walk the loop

Quality bar:

- No `TODO`/`FIXME` text in `desc` (it is player-facing).
- Every loop has at least 3 distinct landmarks you can refer to in chat ("the sump grate", "the listen tube", etc).

## Playtest (Local)

The dev shard overlays these YAML room graphs at load time (they are embedded at compile time from `world/areas/*.yaml`).

1. Start the local broker + shard: `just dev-run`
2. Connect: `telnet 127.0.0.1 4000`
3. Jump to a room: `warp R_SEW_JUNC_01` (or `where` to see your current room id)
4. Walk the graph: `look` then `go <exit>`

Gate debugging (dev-only, not persisted yet):

- Inspect keys: `quest list`
- Set a key: `quest set gate.sewers.shortcut_to_quarry 1`
- Set a counter: `quest set q.q3_sewer_valves.valves_opened 3`
