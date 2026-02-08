---
adventure_id: q3-sewer-valves
area_id: A01
zone: Under-Town Sewers
clusters:
  - CL_TOWN_MAINT_OFFICE
  - CL_SEWERS_ENTRY
  - CL_SEWERS_JUNCTION
  - CL_SEWERS_VALVE_RUN
  - CL_SEWERS_GREASE_KING
hubs:
  - HUB_TOWN_MAINT
  - HUB_SEWERS_JUNCTION
setpieces:
  - setpiece.q3.valve_room_1
  - setpiece.q3.valve_room_2
  - setpiece.q3.valve_room_3
  - setpiece.q3.grease_king_arena
level_band: [3, 4]
party_size: [3, 5]
expected_runtime_min: 35
---

# Q3: Sewer Valves (L3-L4, Party 3-5)

## Hook

The sewers are flooding the wrong neighborhoods. Maintenance wants three valves re-seated. Something down there keeps undoing the work.

Inputs (design assumptions):

- Maintenance office must be an explicit prep/restock node (especially for no-healer / under-leveled parties).
- `R_SEW_JUNC_01` must be a safe regroup pocket (no respawns/pressure) with clear valve progress signage (`valves_opened=0/3`).
- Valve 3 grates branch must teach line-of-sight with at least one explicit melee counterplay hint (corners, cover pockets, flank route).
- Grease King slick patches must have consistent telegraphs and cadence; never fully carpet the arena so "safe squares" exist.
- Broken drone rescue must be clearly optional + safe and have a small practical payoff (hint, cache, or dialogue change).
- Shortcut lever unlock must be explicit ("shortcut unlocked") and include an immediate sealed teaser room beyond the lever.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q3_sewer_valves.state`: `valves` -> `boss` -> `resolved` -> `complete`
- `q.q3_sewer_valves.valves_opened`: `0..3`
- `q.q3_sewer_valves.drone_rescued`: `0/1` (optional)
- `gate.sewers.shortcut_to_quarry`: `0/1` (permanent unlock)

Success conditions:

- open all three valves
- defeat the Grease King
- unlock the shortcut lever (permanent)

## Room Flow

Mostly linear with one hub junction that branches to three valves, then reconverges at the boss.

### R_TOWN_MAINT_01 (CL_TOWN_MAINT_OFFICE, HUB_TOWN_MAINT)

- beat: "Maintenance Office" with a big, unconvincing safety poster.
- teach: dungeon prep, antidotes, party check
- note: explicitly points low-sustain parties to prep (antidotes, bandages, basic consumables) before climbing down
- gate: requires `gate.sewers.entry=1` (from Q2)
- exits: down -> `R_SEW_ENTRY_01`

### R_SEW_ENTRY_01 (CL_SEWERS_ENTRY)

- beat: access ladder, wet concrete, echoing drip; first low-risk mob.
- teach: status hazards (sludge), simple pathing
- exits: east -> `R_SEW_ENTRY_02`

### R_SEW_ENTRY_02 (CL_SEWERS_ENTRY)

- beat: tunnel fork with painted arrows pointing "JUNCTION".
- exits: east -> `R_SEW_JUNC_01`

### R_SEW_JUNC_01 (CL_SEWERS_JUNCTION, HUB_SEWERS_JUNCTION)

- beat: big junction room with three big pipe mouths. A broken drone is wedged into debris (optional rescue later).
- teach: hub-and-spoke dungeon navigation; reconverge
- note: this room is a safe regroup pocket (no respawns/pressure); signage explicitly says you can wait here
- signage: show valve progress (`valves_opened=0/3`) and update it as valves are opened
- drone: optional rescue prompt is explicit that it is safe/optional; rescue has a small payoff (hint, cache, or dialogue change)
- quest: set `q.q3_sewer_valves.state=valves`
- exits:
  - north -> `R_SEW_VALVE1_01`
  - west -> `R_SEW_VALVE2_01`
  - east -> `R_SEW_VALVE3_01`
  - south -> `R_SEW_BOSS_01` (sealed; message says "3 valves required (0/3 opened)")

----

## Valve Run 1 (north)

### R_SEW_VALVE1_01 (CL_SEWERS_VALVE_RUN)

- beat: tight pipe corridor; one ambush.
- exits: north -> `R_SEW_VALVE1_02`

### R_SEW_VALVE1_02 (setpiece.q3.valve_room_1)

- beat: valve wheel stuck; two adds spawn when someone touches it; finish the turn to complete.
- teach: "do objective while under pressure"
- quest: `valves_opened += 1`
- exits: south -> `R_SEW_VALVE1_01`

----

## Valve Run 2 (west)

### R_SEW_VALVE2_01 (CL_SEWERS_VALVE_RUN)

- beat: sludge channel; standing still applies a slow debuff.
- teach: move or suffer; "don’t turtle in sludge"
- exits: west -> `R_SEW_VALVE2_02`

### R_SEW_VALVE2_02 (setpiece.q3.valve_room_2)

- beat: two leech mobs that apply a stacking nuisance; valve is easy but room is annoying.
- quest: `valves_opened += 1`
- exits: east -> `R_SEW_VALVE2_01`

----

## Valve Run 3 (east)

### R_SEW_VALVE3_01 (CL_SEWERS_VALVE_RUN)

- beat: maintenance drone carcasses; ranged attackers behind grates.
- teach: line-of-sight; focus fire
- note: at least one explicit "pull back to a corner to break LoS" hint exists so melee has a plan
- exits: east -> `R_SEW_VALVE3_02`

### R_SEW_VALVE3_02 (setpiece.q3.valve_room_3)

- beat: valve wheel is guarded by one elite drone with a telegraphed hit.
- teach: interrupts (first intro), or "step out when it winds up"
- quest: `valves_opened += 1`
- exits: west -> `R_SEW_VALVE3_01`

----

## Boss Gate

When `valves_opened == 3`, unseal the south exit at `R_SEW_JUNC_01`.

### R_SEW_BOSS_01 (CL_SEWERS_GREASE_KING)

- beat: pre-arena oily slope; foreshadow ground hazard.
- exits: south -> `R_SEW_BOSS_02`

### R_SEW_BOSS_02 (setpiece.q3.grease_king_arena)

- beat: Grease King + adds. Arena periodically spawns slick patches that punish staying clumped.
- teach: spread, re-position, "adds matter"
- note: slick patches have consistent telegraphing and cadence; avoid full-floor coverage so "safe squares" exist
- note: optional opt-in hard mode exists for over-leveled parties (extra add wave for better reward)
- quest: set `q.q3_sewer_valves.state=boss` then `resolved`
- exits: north -> `R_SEW_REWARD_01`

### R_SEW_REWARD_01 (post-boss junction)

- beat: a control box with a "QUARRY BYPASS" lever (permanent unlock).
- feedback: explicit "shortcut unlocked" confirmation and a visible world-change cue (sound, light, signage)
- quest: set `gate.sewers.shortcut_to_quarry=1`, set `q.q3_sewer_valves.state=complete`
- exits:
  - north -> `R_SEW_JUNC_01`
  - east -> `R_SEW_SHORTCUT_01` (sealed until lever; leads toward Quarry foothills)

### R_SEW_SHORTCUT_01 (shortcut tunnel)

- beat: short tunnel with clear signage pointing toward the quarry.
- teach: permanent world changes feel good
- exits: east -> `R_SEW_SHORTCUT_02` (sealed teaser; future quarry foothills)

### R_SEW_SHORTCUT_02 (sealed teaser)

- beat: a reinforced bulkhead marked "QUARRY ACCESS". The lock mechanism is visibly inactive for now.
- feedback: standard sealed-exit message (future content) so players understand this is intentional
- exits: west -> `R_SEW_SHORTCUT_01`

## NPCs

- Maintenance Officer (hub): blunt, rewards competence; doesn’t care about your loot.
- Grease King (boss): communicates via arena effects, not dialogue.

## Rewards

- `gate.sewers.shortcut_to_quarry=1`
- sewer vendor unlock (parts, antidotes) (future)

## Implementation Notes

- This is explicitly group-first: gate a bot autofill boundary at the ladder down.
- Make the three valve rooms mechanically distinct but structurally similar (so players learn the pattern).

Learnings from party runs (`protoadventures/party_runs/q3-sewer-valves/`):

- `R_SEW_JUNC_01` should be an explicit safe regroup pocket (clear signage; waiting is safe).
- Valve interaction UX must be loud (start/interrupted/complete) or parties will argue whether it worked.
- Grease King slick patches need clear boundary language; otherwise it reads as random slow.
- Party size 3 needs slightly gentler add pressure in the boss arena (or one safe pocket to reset).
- Optional rescue beats (broken drone) work best when escort/calm rules are explicit and predictable.
- Treat `R_SEW_JUNC_01` as the recovery node (no respawns/pressure) so wipes don't cascade for low-sustain parties.
- Keep boss hazard telegraphs consistent so wipes read as learning, not randomness.
