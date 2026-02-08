# Gaia Level Progression (Draft)

This is a design doc for Gaia ("where humans and robots peacefully coexist"). It outlines the intended leveling path, the level ranges each area should support, and a few signature quest lines that can anchor progression.

Numbers are targets, not promises. Tune after playtests.

## Assumptions

- Level cap: 20 (D&D-style progression shape; we can ship fewer levels first).
- XP sources: kills + quests; quests should matter (avoid pure grind).
- Gear matters, but should not fully eclipse skill/knowledge. Every bracket should have multiple viable loadouts.
- Areas can have "private" details (spawn tables, rare drops, hidden exits). Players will learn whatever we transmit to clients; anything we never send can remain secret.
- Group-first: mainline quests and setpieces are tuned for parties of 3-5. If fewer than 3 humans are present, we auto-fill with Gaia assist bots to reach a trio (up to a quintet).

## Tiers (content cadence)

- Levels 1-4: Local problems, tutorialization, "learn to live".
- Levels 5-10: Regional travel, factions, first real build diversity.
- Levels 11-16: World-scale threats, multi-zone questlines, shard travel begins to matter.
- Levels 17-20: Mythic/legendary content, long loops, bespoke bosses, low volume.

## Recommended New Player Path (1-5)

1. **Newbie School** (L1-L2): onboarding, verbs, combat basics, death/recovery, inventory.
2. **Town** (L1-L3): social hub, vendors, bank, job board, factions, crafting intro.
3. **Hunting Grounds** (L2-L3): safe-ish loop for repeatable combat and first drops.
4. **Under-Town Sewers** (L3-L4): first "dungeon", status effects, keys, mini-boss.
5. **Quarry + Old Road** (L4-L5): travel, patrols, mixed packs, first named elite.

## Area Level Bands

Starter region (ship first):

| Area | Level band | Why it exists | Notes |
| --- | --- | --- | --- |
| Newbie School: Orientation Wing | 1 | "press buttons" tutorial | ultra-safe; gear mostly cosmetic/training |
| Newbie School: Combat Labs | 1-2 | teach combat loop | scripted failures ok; clear feedback |
| Newbie School: Sim Yard | 2 | teach groups + aggro | spawn "training drones" + "foam weapons" |
| Town: Gaia Gate | 1-3 | arrivals + account identity | name, species (human/robot), starter kit |
| Town: Market Row | 1-3 | vendors + gold sinks | ammo/repairs, basic consumables |
| Town: Job Board | 1-4 | quest faucet | points to every starter loop |
| Hunting Grounds: Meadowline | 2-3 | first real grind | fast respawn; low-risk currency |
| Hunting Grounds: Scrap Orchard | 2-3 | robot-themed loot | salvage drops, basic mods |
| Under-Town Sewers | 3-4 | first dungeon feel | keys, valves, poison-ish hazards |
| Quarry | 4-5 | first elite loop | ore + heavier mobs |
| Old Road Checkpoint | 4-5 | travel gate to midgame | guards, tolls, ambushes |

Midgame region (build next):

| Area | Level band | Why it exists | Notes |
| --- | --- | --- | --- |
| Rail Spur | 5-6 | fast travel + events | "train heist" style encounters |
| Rustwood | 5-7 | mixed wilderness | stealth mobs + ranged packs |
| Sunken Library | 6-8 | puzzle dungeon | knowledge gates, map fragments |
| Factory District (Outskirts) | 7-9 | urban combat | tight rooms; line-of-sight tactics |
| The Reservoir | 8-10 | resource control | water/power as mechanics |

High region (later):

| Area | Level band | Why it exists | Notes |
| --- | --- | --- | --- |
| Skybridge | 11-12 | vertical traversal | winds, knockback, flying threats |
| Glass Wastes | 12-14 | long-range survival | attrition, navigation, storms |
| The Core | 15-16 | "raid-lite" dungeon | coordination checks, multi-stage bosses |
| The Seam | 17-20 | endgame loops | rare drops, shards intersect |

## Signature Quest Lines (anchors)

These are intended to be memorable, to teach systems, and to create reasons to travel.

### 1) "First Day on Gaia" (L1-L2)

- Start: Newbie School Orientation Wing
- Beats:
  - get your badge (identity + permissions)
  - learn the verbs (look, move, get, drop, equip, say)
  - pass the Combat Labs (basic attacks, healing, retreat)
- Rewards: starter kit choice (human/robot flavored), town pass

### 2) "The Job Board Never Sleeps" (L1-L4)

- Start: Town Job Board
- Beats:
  - 3 small contracts (delivery, pest control, escort)
  - 1 choice contract (pick a faction contact: civic/industrial/green)
  - unlock repeatables (daily caps) + bounty board
- Rewards: faction introduction + "comm link" (future cross-shard handle)

### 3) "Sewer Valves" (L3-L4)

- Start: Town Maintenance Office
- Beats:
  - find 3 valves (teaches keys + backtracking)
  - mini-boss: "The Grease King" (status effects + adds)
  - optional: rescue lost drone (teaches escort AI)
- Rewards: utility item (flashlight/torch equivalent), sewer shortcut unlocked

### 4) "Quarry Rights" (L4-L6)

- Start: Quarry Foreman + Scrap Union Rep
- Beats:
  - clear an illegal dig
  - decide: union vs foreman (branching rep)
  - named elite: "Breaker-7" (teaches interrupts / big hits)
- Rewards: first real weapon mod + access to the Old Road Checkpoint

### 5) "The Sunken Index" (L6-L8)

- Start: Sunken Library antechamber
- Beats:
  - collect 4 index plates (dungeon scavenger hunt)
  - decode map fragments (introduces overworld graph navigation)
  - choose which district to unlock first (Factory vs Reservoir)
- Rewards: travel unlock + "index key" (future shard migration hook)

### Later "Spine" Quest Lines (L5-L20)

These are the mainline unlock quests that carry players through the rest of A01.

| Quest | Levels | Start | Unlock |
| --- | --- | --- | --- |
| Q6: Hillfort Signal | 5-7 | Hillfort Ruins | rail pass + safehouse |
| Q7: Conveyor War | 7-10 | Factory District | Underrail entry (factory route) |
| Q8: Flow Treaty | 8-11 | Reservoir | Underrail entry (reservoir route) |
| Q9: Beacon Protocol | 9-12 | The Underrail | Skybridge access |
| Q10: Anchor the Wind | 11-13 | Skybridge | Glass Wastes access |
| Q11: Stormglass Route | 12-15 | Glass Wastes | Crater Gardens access |
| Q12: Bloom Contract | 13-16 | Crater Gardens | The Core access |
| Q13: Heart of the Machine | 15-17 | The Core | The Seam access |
| Q14: Stabilize the Boundary | 17-20 | The Seam | seam expansions (A41+) setup |

## Content Scheduling (ship order)

- Phase 0: L1-L3 (Newbie School + Town + Meadowline)
- Phase 1: L1-L5 (Sewers + Quarry + Old Road Checkpoint)
- Phase 2: L5-L8 (Rustwood + Sunken Library)
- Phase 3: L8-L10 (Factory District + Reservoir)

## Room Scale

Room budgets and placeholder future areas are tracked in `docs/room_scale_plan.md`.

## Quest Graph

Quest progression as named states (and how it connects to areas) is tracked in:

- `docs/quest_state_model.md`
- `docs/quest_state_graph.mermaid`

## Attribution (SRD 5.2.1, CC BY 4.0)

This work includes material from the System Reference Document 5.2.1 ("SRD 5.2.1") by Wizards of the Coast LLC, available at https://www.dndbeyond.com/srd. The SRD 5.2.1 is licensed under the Creative Commons Attribution 4.0 International License, available at https://creativecommons.org/licenses/by/4.0/legalcode.
