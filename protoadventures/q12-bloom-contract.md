---
adventure_id: q12-bloom-contract
area_id: A01
zone: Crater Gardens
clusters:
  - CL_GARDENS_GREENHOUSE
  - CL_GARDENS_WILDS
  - CL_GARDENS_BLOOM_GROVES
  - CL_GARDENS_ROOT_ENGINE
hubs:
  - HUB_GARDENS_GREENHOUSE
setpieces:
  - setpiece.q12.bloom_grove_1
  - setpiece.q12.bloom_grove_2
  - setpiece.q12.bloom_grove_3
  - setpiece.q12.root_engine_arena
level_band: [13, 16]
party_size: [3, 5]
expected_runtime_min: 80
---

# Q12: Bloom Contract (Crater Gardens) (L13-L16, Party 3-5)

## Hook

Crater Gardens look like a reclaimed paradise until you notice the seams: engineered plants wrapped around ancient machinery. The greenhouse staff offers a contract that is simple on paper and dangerous in execution: harvest three bloom samples from three groves, then take the samples to the Root Engine to open the path to The Core.

The real boss is attrition and timing: spores pulse, caretakers "sanitize" intruders, and harvest interactions punish panic.

Inputs (design assumptions):

- Spore pulse/lull cadence must be explicit and consistent; "wait spots" must be clearly safe.
- A decontam safe pocket after sample 1 is required so under-leveled/no-healer/small parties can continue.
- Sample harvest UX must be loud and consistent (start/interrupted/harvested) with a visible `samples_collected=0/3` counter.
- Caretaker heal/cleanse cycle needs loud telegraphs and at least one non-healer success path.
- Melee viability depends on intentional funnels and readable hazard lines; avoid wide open "spore carpet" rooms.
- Optional bio-pact/debt systems must be opt-in with explicit, capped costs and a clear paydown mechanic.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q12_bloom_contract.state`: `entry` -> `samples` -> `engine` -> `complete` (placeholder)
- `q.q12_bloom_contract.samples`: `0..3` (placeholder)
- `gate.core.entry`: `0/1`

Success conditions:

- harvest 3 samples (one per grove)
- defeat the Root Engine setpiece (or complete its objective)
- return to the greenhouse and unlock The Core gate

## Rules That Must Be Legible

### Spore Attrition (named stack)

Define one named stack (placeholder):

- `spore_load` stacks when parties stand in spore fog during pulse windows or linger in grove hazards.
- `spore_load` should be clearly telegraphed (audio + text): pulse vs lull.
- Decontam clears or reduces `spore_load` (see safe pocket).

Players must know:

- what causes stacks (pulse exposure, certain mobs)
- what stacks do (debuffs + eventual danger threshold)
- how to remove them (decontam chamber, specific consumable)

### Harvest Interaction (explicit UX)

Harvesting is an interaction, not a loot click:

- start is loud
- interruptions are loud (pulse starts, hit taken, movement)
- completion is loud (sample secured; quest counter increments)

Harvest should refuse/interrupt safely if a pulse begins (avoid gotcha wipes).

## Safe Pocket (guaranteed progress)

After the first sample is delivered, unlock a greenhouse decontam chamber:

- clearly marked, consistent signage language
- clears/reduces `spore_load`
- restock point (water/cloth equivalents)
- doubles as wipe recovery so under-leveled/no-healer/small parties can make consistent progress

## Secret (navigation reward): Service Tunnels

One consistent secret route bypasses an exposed wilds segment:

- a greenhouse service tunnel accessed via tunnel grates/maintenance markers
- connects the greenhouse hub to a grove approach in fewer pulls

This is the Gardens secret: competence via route literacy, not random loot.

----

## Room Flow

Crater Gardens should teach:

- pulse/lull timing (spores),
- funnel discipline (melee viability),
- objective-first play (harvest under pressure),
- cycle counterplay (caretaker heals/cleanses),
- and a boss that is mostly "windows + geometry".

### R_GARDENS_GREENHOUSE_01 (CL_GARDENS_GREENHOUSE, HUB_GARDENS_GREENHOUSE)

- beat: contract hub. Staff explains three groves; samples gate Root Engine access.
- teach: "do not harvest during pulses"; decontam exists but is locked until sample 1 returns
- quest: set `q.q12_bloom_contract.state=entry` (placeholder)
- exits:
  - north -> `R_GARDENS_WILDS_01` (main route)
  - east -> `R_GARDENS_SERVICE_01` (secret if discovered)
  - west -> `R_GARDENS_DECON_01` (unlocked after sample 1)

### R_GARDENS_SERVICE_01 (service tunnels, secret)

- beat: service tunnel behind greenhouse panels. Maintenance markers match the "service" icon from signage.
- teach: consistent secrets; route literacy is competence
- exits: west -> `R_GARDENS_GREENHOUSE_01`, north -> `R_GARDENS_WILDS_02`

### R_GARDENS_WILDS_01 (CL_GARDENS_WILDS)

- beat: mixed packs; terrain funnels.
- teach: pull back to clear squares; do not chase into spore fog
- exits: north -> `R_GARDENS_GROVE_1_01`, south -> `R_GARDENS_GREENHOUSE_01`

----

## Bloom Grove 1 (teach spores + harvest)

### R_GARDENS_GROVE_1_01 (setpiece.q12.bloom_grove_1)

- beat: spore fog pulses; swarms pressure harvesters.
- objective: secure sample 1 via harvest interaction.
- teach: pulse/lull grammar and "harvest only on lull"
- quest: `samples += 1`
- exits:
  - south -> `R_GARDENS_GREENHOUSE_01` (deliver sample)
  - east -> `R_GARDENS_WILDS_02` (continue to grove 2)

### R_GARDENS_DECON_01 (safe pocket, unlocked after sample 1)

- beat: greenhouse decontam chamber opens. Staff repeats rules and shows clear "spore load" messaging.
- teach: decontam as reset between groves
- exits: back -> `R_GARDENS_GREENHOUSE_01`

----

## Bloom Grove 2 (teach funnels + vine geometry)

### R_GARDENS_WILDS_02 (CL_GARDENS_WILDS)

- beat: approach to vine labyrinth; reach attackers punish open terrain.
- teach: funnel discipline; reset points; do not chain pulls
- exits: north -> `R_GARDENS_GROVE_2_01`, south -> `R_GARDENS_GROVE_1_01`

### R_GARDENS_GROVE_2_01 (setpiece.q12.bloom_grove_2)

- beat: vine labyrinth with clear lanes and "safe squares"; reach attackers and hazards.
- objective: secure sample 2.
- teach: movement matters; melee can win if lanes are legible
- quest: `samples += 1`
- exits: east -> `R_GARDENS_WILDS_03`, west -> `R_GARDENS_WILDS_02`

----

## Bloom Grove 3 (teach caretaker cycle counterplay)

### R_GARDENS_WILDS_03 (CL_GARDENS_WILDS)

- beat: patrol-heavy approach; optional checkpoint beat can be bypassed via staff token/negotiation (future).
- teach: avoid loop spirals; navigation markers matter
- exits: north -> `R_GARDENS_GROVE_3_01`, west -> `R_GARDENS_GROVE_2_01`

### R_GARDENS_GROVE_3_01 (setpiece.q12.bloom_grove_3)

- beat: caretaker drone arrives mid-harvest and runs a heal/cleanse cycle while adds pressure.
- objective: secure sample 3.
- teach: cycle counterplay (interrupt, focus windows, or mechanic); don’t split
- quest: `samples += 1` (now 3/3)
- exits: east -> `R_GARDENS_ENGINE_GATE_01`, west -> `R_GARDENS_WILDS_03`

----

## Root Engine (setpiece.q12.root_engine_arena)

Root Engine should be a multi-stage "windows + geometry" fight:

- Phase 1: roots and adds, with hazard lines that punish clustering.
- Phase 2: engine core exposed in brief windows; parties must commit burst/interrupts.

### R_GARDENS_ENGINE_GATE_01 (CL_GARDENS_ROOT_ENGINE)

- beat: sample check; gate only opens when `samples == 3`.
- quest: set `q.q12_bloom_contract.state=engine` (placeholder)
- exits: north -> `R_GARDENS_ENGINE_01`, south -> `R_GARDENS_GROVE_3_01`

### R_GARDENS_ENGINE_01 (setpiece.q12.root_engine_arena)

- beat: multi-stage engine fight with clear hazard lines and exposure windows.
- teach: coordinate windows; move on lulls; target priority under swarm pressure
- quest: set `q.q12_bloom_contract.state=complete` (placeholder); set `gate.core.entry=1`
- exits: south -> `R_GARDENS_ENGINE_GATE_01`

## Optional Knobs (so reruns stay interesting)

### Choice: Burn vs Preserve

Offer a loud choice at least once (grove 2 or pre-engine):

- burn: faster progress and less vine pressure, but costs reputation/biome stability
- preserve: slower/safer, but better reputation and future garden stability

### Optional System: Bio-Pacts (mutation debt, capped)

If a party opts in via warlock/contract beat, define a capped debt system:

- `mutation_debt` tokens: start at 0; cap at 2.
- taking a pact: +tempo (ignore one pulse, faster harvest, or one free bypass), then `mutation_debt += 1`.
- debt consequences: increases spore severity or adds one extra caretaker response later.
- paydown: only in `R_GARDENS_DECON_01` (spend time or forfeit contract bonus to reduce by 1).

Make costs explicit and persistent so this feels like a deliberate choice, not a hidden trap.

## NPCs

- Greenhouse Contract Lead: practical; repeats "three groves" and "harvest on lull".
- Garden Staff (optional): can grant a bypass token if negotiated (future).
- Caretaker Drones: sanitize intruders; heal/cleanse cycle telegraphs must be learnable.

## Rewards

- `gate.core.entry=1` (The Core travel unlocked)
- greenhouse contract upgrades (future): better pay, bypass token, or reduced spore severity once per day

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q12-bloom-contract/`):

- The decontam safe pocket after sample 1 is required for under-leveled/no-healer/small parties to progress.
- Spore pulse/lull must be explicit and consistent; "wait spots" need to be clearly safe.
- Caretaker cycle counterplay needs loud telegraphs and a non-healer success path.
- Melee viability depends on intentional funnels and readable hazard lines.
- Optional debt systems (bio-pacts) are fun if costs are explicit, capped, and have paydown.
