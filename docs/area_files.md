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
4. `world/overworld.yaml` (authoritative portal IDs + travel lens)
5. `protoadventures/*.md` for the zone you are translating

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
      Short, original prose. No WotC text.
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

## Validation Rules (current)

Run:

- `just world-validate` (overworld + zone shapes)
- `just area-files-validate` (area files)

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

## Parallel Workflow (Per Zone)

1. Take the lock:
   - `just area-lock <zone_id> <claimed_by>`
2. Draft or edit `world/areas/<zone_id>.yaml`.
3. Validate:
   - `just world-validate`
   - `just area-files-validate`
4. Release the lock:
   - `just area-unlock <zone_id> <claimed_by>`

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
