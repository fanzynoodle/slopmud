# Gaia World Scale Plan (Draft)

This doc budgets rooms for the initial 0-20 experience and sketches placeholder "future areas" to reach ~20,000 rooms total.

Terminology in this doc:

- Room: a single location node (id + description + exits + contents).
- Zone: a coherent chunk of rooms that ships together (what we'd likely call an "area file" later).
- Area ID: a stable numeric label for a large region bundle (A01..A50). A01 is the initial shipped world; A02+ are placeholders for now.

## Targets

- A01: 2,000 rooms, supports levels 1-20 end-to-end.
- A02-A50: 18,000 rooms total (placeholders for now).
- Total: 20,000 rooms.

## D&D Tier Model (for area difficulty)

We want the world to "feel" like D&D progression: the further you get from the start (or the deeper behind gates you go), the higher the expected character level.

Tiers (roughly):

- Tier 1: levels 1-4 (local heroes, tutorial + early threats)
- Tier 2: levels 5-10 (regional travel, builds come online)
- Tier 3: levels 11-16 (world-scale threats, complex dungeons)
- Tier 4: levels 17-20 (mythic/endgame loops)

Notes:

- A01 is the launch "spine" and contains content across all tiers (so a fresh server can support 1-20 without expansions).
- A02+ expansions should be ordered so their *primary* tier generally increases with the area number.

## A01 (2,000 rooms) Zone Budget (Levels 1-20)

These zones are the "real game" for the initial launch. Room counts are targets.

| Zone | Levels | Target rooms | Notes |
| --- | --- | ---:| --- |
| Newbie School | 1-2 | 50 | onboarding campus, tutorial loops |
| Town: Gaia Gate | 1-4 | 120 | hub services, factions, job board |
| Meadowline | 2-3 | 90 | safe grind loop, low-risk XP |
| Scrap Orchard | 2-3 | 90 | salvage loop, robot-flavored drops |
| Under-Town Sewers | 3-4 | 110 | first dungeon, valves/keys |
| Quarry | 4-6 | 70 | heavier hits, first elite |
| Old Road Checkpoint | 4-6 | 30 | travel gate, patrols, tolls |
| Hillfort Ruins | 4-7 | 70 | mixed packs, first branching paths |
| Rail Spur | 5-7 | 50 | world events, travel unlock |
| Rustwood | 5-8 | 120 | wilderness navigation, stealth threats |
| Sunken Library | 6-9 | 110 | puzzle-ish dungeon, map fragments |
| Factory District (Outskirts) | 7-10 | 120 | line-of-sight tactics, hazards |
| The Reservoir | 8-11 | 110 | resource mechanics, faction conflict |
| The Underrail | 9-12 | 120 | subway dungeon, ambush + traversal |
| Skybridge | 11-13 | 100 | vertical traversal, knockback, winds |
| Glass Wastes | 12-15 | 160 | attrition, navigation, storms |
| Crater Gardens | 13-16 | 130 | mid-late wilderness, elite loops |
| The Core | 15-17 | 150 | "raid-lite" dungeon, multi-stage boss |
| The Seam | 17-20 | 200 | endgame loops, shard boundary theme |

Total A01 rooms: 2,000.

Zone cluster breakdown (for drafting actual area/room files) is tracked in `docs/zone_beats.md`.

## A02-A50 (18,000 rooms) Placeholder Areas

These are planned areas that will exist later. For now, we should ship *blocked exits* that point at them so the topology is stable and we can add content without breaking the world graph.

Room budgets:

- 17 "major" areas at 400 rooms each.
- 32 "standard" areas at 350 rooms each.
- Total A02-A50 rooms: 18,000.

### Blocked Exit Design (what ships now)

For each placeholder connection:

1. The exit exists in the room graph (so map topology is real).
2. Attempting to traverse it yields a consistent message and does not move the player.
3. The exit can later be toggled "open" without renumbering rooms or rewriting other zones.

Suggested standard message:

> The way is sealed. You can feel the world beyond, but Gaia is not ready yet.

Suggested metadata (for later implementation):

- `exit.state = sealed`
- `exit.opens_area = "A##"`
- `exit.opens_patch = "YYYY-MM-DD"` (optional)

### Placeholder Area List (A02..A50)

Entry column is where we expect the first gate/blocked exit to live in A01.

| Area | D&D tier | Levels | Rooms | Entry (A01) | Status |
| --- | --- | --- | ---:| --- | --- |
| A02 Coastline Commons | T1 | 1-4 | 400 | Town | placeholder |
| A03 Kestrel Peaks | T1 | 2-4 | 350 | Old Road | placeholder |
| A04 Gleamfields | T1 | 1-4 | 350 | Town | placeholder |
| A05 The Old Metro | T1 | 3-5 | 400 | Underrail | placeholder |
| A06 Cranktown | T1 | 4-5 | 350 | Rail Spur | placeholder |
| A07 Verdant Array | T1 | 2-4 | 350 | Town | placeholder |
| A08 The Ash Canal | T1 | 4-5 | 350 | Old Road | placeholder |
| A09 Fallow Barrens | T1 | 4-5 | 350 | Old Road | placeholder |
| A10 Mirror Lake | T1 | 3-5 | 350 | Rustwood | placeholder |
| A11 The Bleached Docks | T2 | 5-7 | 350 | Coastline Commons | placeholder |
| A12 Signal Ridge | T2 | 6-8 | 400 | Old Road | placeholder |
| A13 Copper Basilica | T2 | 7-9 | 350 | Town | placeholder |
| A14 The Cinder Mile | T2 | 7-9 | 350 | Old Road | placeholder |
| A15 Warden Forest | T2 | 6-9 | 350 | Rustwood | placeholder |
| A16 The Broken Observatory | T2 | 8-10 | 400 | Sunken Library | placeholder |
| A17 Silt Delta | T2 | 7-10 | 350 | Reservoir | placeholder |
| A18 The Drift Mines | T2 | 8-10 | 400 | Quarry | placeholder |
| A19 Stoneglass Canyon | T2 | 9-10 | 350 | Glass Wastes | placeholder |
| A20 The Flooded Forum | T2 | 8-10 | 350 | Reservoir | placeholder |
| A21 Hollow Spire | T2 | 9-10 | 350 | Skybridge | placeholder |
| A22 The Ember Gardens | T2 | 9-10 | 350 | Crater Gardens | placeholder |
| A23 The Lattice | T2 | 8-10 | 400 | Factory | placeholder |
| A24 Clockwork Marsh | T2 | 9-10 | 350 | Silt Delta | placeholder |
| A25 The Saltwind Track | T2 | 8-10 | 350 | Old Road | placeholder |
| A26 The Great Relay | T3 | 11-13 | 350 | Signal Ridge | placeholder |
| A27 Vault-Kiln | T3 | 12-14 | 400 | Factory | placeholder |
| A28 Sapphire Underways | T3 | 13-15 | 350 | The Core | placeholder |
| A29 The Orchard of Knives | T3 | 13-15 | 350 | Crater Gardens | placeholder |
| A30 Titan Grave | T3 | 14-16 | 400 | Glass Wastes | placeholder |
| A31 The Null Bazaar | T3 | 12-14 | 350 | Town | placeholder |
| A32 The Pale Engine | T3 | 14-16 | 400 | The Core | placeholder |
| A33 The Tenfold Locks | T3 | 13-16 | 350 | Reservoir | placeholder |
| A34 Kite City | T3 | 12-14 | 350 | Skybridge | placeholder |
| A35 The Meridian Ruins | T3 | 14-16 | 350 | Glass Wastes | placeholder |
| A36 Chromehollow | T3 | 15-16 | 400 | The Core | placeholder |
| A37 The Radiant Trench | T3 | 13-16 | 350 | Coastline Commons | placeholder |
| A38 The Ivy Foundry | T3 | 11-13 | 350 | Factory | placeholder |
| A39 The Quiet Warfront | T3 | 12-14 | 350 | Old Road | placeholder |
| A40 The Far Archive | T3 | 13-16 | 350 | Sunken Library | placeholder |
| A41 Seam: Red Shift | T4 | 17-20 | 400 | The Seam | placeholder |
| A42 Seam: Blue Noise | T4 | 17-20 | 400 | The Seam | placeholder |
| A43 Seam: Glass Choir | T4 | 18-20 | 400 | The Seam | placeholder |
| A44 Seam: Broken Sun | T4 | 18-20 | 400 | The Seam | placeholder |
| A45 Seam: The Suture | T4 | 19-20 | 400 | The Seam | placeholder |
| A46 The Crown Assembly | T4 | 19-20 | 400 | The Seam | placeholder |
| A47 The Last Switchyard | T4 | 19-20 | 350 | The Underrail | placeholder |
| A48 The Dream Cache | T4 | 19-20 | 350 | The Seam | placeholder |
| A49 The End Orchard | T4 | 20 | 350 | The Seam | placeholder |
| A50 Gaia: Ascension Loop | T4 | 20 | 400 | The Seam | placeholder |

## Totals Check

- A01: 2,000
- A02-A50: 18,000
- Total: 20,000
