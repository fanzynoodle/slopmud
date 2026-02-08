# Gaia Overworld Cartesian Layout (Draft)

This doc assigns cartesian coordinates to **area entrances/exits** and defines a **length** (movement cost) for each inter-area exit.

Later, when we enforce movement points, these lengths become the authoritative travel cost for "overworld" transitions. Normal room-to-room movement inside zones remains cost 1 unless explicitly overridden.

## Workflow (Source Of Truth)

`world/overworld.yaml` and `world/overworld_pairs.tsv` are generated from this doc.

After editing this file:

- Regenerate: `just overworld-export`
- Validate: `just overworld-validate` (or `just world-validate`)

## Units

- Coordinate unit: arbitrary planning unit (integer grid).
- Exit length (`len`): integer movement cost for traversing that exit.
- Rule for beginners: any exit involving **Newbie School / Town / Meadowline / Scrap Orchard** should be `len=1`.

## Zone Anchors (for grouping only)

Anchors are not used for travel costs directly; portal coordinates below are.

| Zone | Anchor (x,y) |
| --- | --- |
| Newbie School | (0,-2) |
| Town: Gaia Gate | (0,0) |
| Meadowline | (3,0) |
| Scrap Orchard | (0,3) |
| Under-Town Sewers | (3,3) |
| Quarry | (6,3) |
| Hillfort Ruins | (6,6) |
| Old Road Checkpoint | (9,4) |
| Rail Spur | (12,4) |
| Rustwood | (9,7) |
| Sunken Library | (12,7) |
| Factory District (Outskirts) | (15,6) |
| The Reservoir | (15,9) |
| The Underrail | (18,7) |
| Skybridge | (21,7) |
| Glass Wastes | (24,7) |
| Crater Gardens | (27,7) |
| The Core | (30,7) |
| The Seam | (33,7) |

## Portals (Entrances/Exits)

Each portal is a concrete "exit room" (or small boundary cluster) inside a zone.

Column notes:

- Cluster hints reference `docs/zone_beats.md` and are just guidance for where the portal should live.
- Coordinates are global cartesian positions for planning.

| Portal ID | Zone | Cluster hint | Connects to | (x,y) |
| --- | --- | --- | --- | --- |
| P_NS_TOWN | Newbie School | CL_NS_ORIENTATION | P_TOWN_NS | (0,-1) |
| P_TOWN_NS | Town: Gaia Gate | CL_TOWN_GATE_PLAZA | P_NS_TOWN | (0,0) |
| P_TOWN_MEADOW | Town: Gaia Gate | CL_TOWN_EDGE | P_MEADOW_TOWN | (1,0) |
| P_MEADOW_TOWN | Meadowline | CL_MEADOW_TRAILS | P_TOWN_MEADOW | (2,0) |
| P_TOWN_ORCHARD | Town: Gaia Gate | CL_TOWN_EDGE | P_ORCHARD_TOWN | (0,1) |
| P_ORCHARD_TOWN | Scrap Orchard | CL_ORCHARD_GROVE | P_TOWN_ORCHARD | (0,2) |
| P_TOWN_SEWERS | Town: Gaia Gate | CL_TOWN_MAINT_OFFICE | P_SEWERS_TOWN | (1,1) |
| P_SEWERS_TOWN | Under-Town Sewers | CL_SEWERS_ENTRY | P_TOWN_SEWERS | (2,1) |
| P_MEADOW_SEWERS | Meadowline | CL_MEADOW_SEWER_GRATE | P_SEWERS_MEADOW | (3,1) |
| P_SEWERS_MEADOW | Under-Town Sewers | CL_SEWERS_ENTRY | P_MEADOW_SEWERS | (3,2) |
| P_ORCHARD_SEWERS | Scrap Orchard | CL_ORCHARD_PIPEWAY | P_SEWERS_ORCHARD | (1,3) |
| P_SEWERS_ORCHARD | Under-Town Sewers | CL_SEWERS_ENTRY | P_ORCHARD_SEWERS | (2,3) |
| P_SEWERS_QUARRY | Under-Town Sewers | CL_SEWERS_JUNCTION | P_QUARRY_SEWERS | (4,3) |
| P_QUARRY_SEWERS | Quarry | CL_QUARRY_FOOTHILLS | P_SEWERS_QUARRY | (5,3) |
| P_QUARRY_CHECKPOINT | Quarry | CL_QUARRY_FOOTHILLS | P_CHECKPOINT_QUARRY | (6,2) |
| P_CHECKPOINT_QUARRY | Old Road Checkpoint | CL_CHECKPOINT_ROAD | P_QUARRY_CHECKPOINT | (7,2) |
| P_QUARRY_HILLFORT | Quarry | CL_QUARRY_FOOTHILLS | P_HILLFORT_QUARRY | (6,4) |
| P_HILLFORT_QUARRY | Hillfort Ruins | CL_HILLFORT_APPROACH | P_QUARRY_HILLFORT | (6,5) |
| P_HILLFORT_CHECKPOINT | Hillfort Ruins | CL_HILLFORT_APPROACH | P_CHECKPOINT_HILLFORT | (8,5) |
| P_CHECKPOINT_HILLFORT | Old Road Checkpoint | CL_CHECKPOINT_ROAD | P_HILLFORT_CHECKPOINT | (9,5) |
| P_CHECKPOINT_RAIL | Old Road Checkpoint | CL_CHECKPOINT_ROAD | P_RAIL_CHECKPOINT | (10,4) |
| P_RAIL_CHECKPOINT | Rail Spur | CL_RAIL_TERMINAL | P_CHECKPOINT_RAIL | (12,4) |
| P_CHECKPOINT_RUSTWOOD | Old Road Checkpoint | CL_CHECKPOINT_ROAD | P_RUSTWOOD_CHECKPOINT | (9,6) |
| P_RUSTWOOD_CHECKPOINT | Rustwood | CL_RUSTWOOD_TRAILS | P_CHECKPOINT_RUSTWOOD | (9,8) |
| P_RUSTWOOD_LIBRARY | Rustwood | CL_RUSTWOOD_TRAILS | P_LIB_RUSTWOOD | (11,8) |
| P_LIB_RUSTWOOD | Sunken Library | CL_LIB_ANTECHAMBER | P_RUSTWOOD_LIBRARY | (13,8) |
| P_LIB_FACTORY | Sunken Library | CL_LIB_DECODER_CHAMBER | P_FACTORY_LIBRARY | (13,7) |
| P_FACTORY_LIBRARY | Factory District (Outskirts) | CL_FACTORY_GATE | P_LIB_FACTORY | (15,7) |
| P_LIB_RESERVOIR | Sunken Library | CL_LIB_DECODER_CHAMBER | P_RESERVOIR_LIBRARY | (13,9) |
| P_RESERVOIR_LIBRARY | The Reservoir | CL_RES_SHORES | P_LIB_RESERVOIR | (15,9) |
| P_FACTORY_RESERVOIR | Factory District (Outskirts) | CL_FACTORY_GATE | P_RESERVOIR_FACTORY | (16,8) |
| P_RESERVOIR_FACTORY | The Reservoir | CL_RES_SHORES | P_FACTORY_RESERVOIR | (17,8) |
| P_FACTORY_UNDERRAIL | Factory District (Outskirts) | CL_FACTORY_SWITCHROOM | P_UNDERRAIL_FACTORY | (16,6) |
| P_UNDERRAIL_FACTORY | The Underrail | CL_UNDERRAIL_PLATFORM | P_FACTORY_UNDERRAIL | (18,6) |
| P_RESERVOIR_UNDERRAIL | The Reservoir | CL_RES_TUNNELS | P_UNDERRAIL_RESERVOIR | (16,10) |
| P_UNDERRAIL_RESERVOIR | The Underrail | CL_UNDERRAIL_PLATFORM | P_RESERVOIR_UNDERRAIL | (18,10) |
| P_UNDERRAIL_SKYBRIDGE | The Underrail | CL_UNDERRAIL_PLATFORM | P_SKYBRIDGE_UNDERRAIL | (20,7) |
| P_SKYBRIDGE_UNDERRAIL | Skybridge | CL_SKY_ANCHORHOUSE | P_UNDERRAIL_SKYBRIDGE | (23,7) |
| P_SKYBRIDGE_WASTES | Skybridge | CL_SKY_SPANS | P_WASTES_SKYBRIDGE | (24,7) |
| P_WASTES_SKYBRIDGE | Glass Wastes | CL_WASTES_OUTPOST | P_SKYBRIDGE_WASTES | (27,7) |
| P_WASTES_GARDENS | Glass Wastes | CL_WASTES_OUTPOST | P_GARDENS_WASTES | (28,7) |
| P_GARDENS_WASTES | Crater Gardens | CL_GARDENS_GREENHOUSE | P_WASTES_GARDENS | (31,7) |
| P_GARDENS_CORE | Crater Gardens | CL_GARDENS_GREENHOUSE | P_CORE_GARDENS | (33,7) |
| P_CORE_GARDENS | The Core | CL_CORE_GATE_RING | P_GARDENS_CORE | (37,7) |
| P_CORE_SEAM | The Core | CL_CORE_GATE_RING | P_SEAM_CORE | (38,7) |
| P_SEAM_CORE | The Seam | CL_SEAM_EDGE_STATION | P_CORE_SEAM | (42,7) |

## Exit Lengths (Movement Cost)

Lengths are defined per portal-to-portal edge. Unless noted, treat them as symmetric (same cost both directions).

Starter region rule: all exits involving Newbie School / Town / Meadowline / Scrap Orchard are `len=1`.

| From | To | len | Notes |
| --- | --- | ---:| --- |
| P_NS_TOWN | P_TOWN_NS | 1 | newbie frictionless |
| P_TOWN_MEADOW | P_MEADOW_TOWN | 1 | newbie frictionless |
| P_TOWN_ORCHARD | P_ORCHARD_TOWN | 1 | newbie frictionless |
| P_TOWN_SEWERS | P_SEWERS_TOWN | 1 | newbie frictionless |
| P_MEADOW_SEWERS | P_SEWERS_MEADOW | 1 | newbie frictionless |
| P_ORCHARD_SEWERS | P_SEWERS_ORCHARD | 1 | newbie frictionless |
| P_SEWERS_QUARRY | P_QUARRY_SEWERS | 1 | still early-game |
| P_QUARRY_CHECKPOINT | P_CHECKPOINT_QUARRY | 1 | still early-game |
| P_QUARRY_HILLFORT | P_HILLFORT_QUARRY | 1 | still early-game |
| P_HILLFORT_CHECKPOINT | P_CHECKPOINT_HILLFORT | 1 | still early-game |
| P_CHECKPOINT_RAIL | P_RAIL_CHECKPOINT | 2 | road travel begins to matter |
| P_CHECKPOINT_RUSTWOOD | P_RUSTWOOD_CHECKPOINT | 2 | road travel begins to matter |
| P_RUSTWOOD_LIBRARY | P_LIB_RUSTWOOD | 2 | regional travel |
| P_LIB_FACTORY | P_FACTORY_LIBRARY | 2 | regional travel |
| P_LIB_RESERVOIR | P_RESERVOIR_LIBRARY | 2 | regional travel |
| P_FACTORY_RESERVOIR | P_RESERVOIR_FACTORY | 1 | adjacent districts |
| P_FACTORY_UNDERRAIL | P_UNDERRAIL_FACTORY | 2 | elevator/entry |
| P_RESERVOIR_UNDERRAIL | P_UNDERRAIL_RESERVOIR | 2 | tunnel/entry |
| P_UNDERRAIL_SKYBRIDGE | P_SKYBRIDGE_UNDERRAIL | 3 | long transit segment |
| P_SKYBRIDGE_WASTES | P_WASTES_SKYBRIDGE | 3 | harsh travel |
| P_WASTES_GARDENS | P_GARDENS_WASTES | 3 | harsh travel |
| P_GARDENS_CORE | P_CORE_GARDENS | 4 | endgame travel |
| P_CORE_SEAM | P_SEAM_CORE | 4 | endgame travel |

## Notes / Next Steps

- This is intentionally coarse. When we draft actual room graphs, each portal becomes a specific room and we can add a short "corridor" of 1-cost rooms behind it to make the feel match the `len` here.
- If we later want movement costs to be derived from coordinates automatically, we can switch to `len = round(distance(Pa, Pb))` and re-place portal points.
