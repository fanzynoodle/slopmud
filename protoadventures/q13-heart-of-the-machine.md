---
adventure_id: q13-heart-of-the-machine
area_id: A01
zone: The Core
clusters:
  - CL_CORE_GATE_RING
  - CL_CORE_APPROACH
  - CL_CORE_INNERWORKS
  - CL_CORE_AUDITOR_ARENA
hubs:
  - HUB_CORE_GATE
setpieces:
  - setpiece.q13.lock_1_precision
  - setpiece.q13.lock_2_geometry
  - setpiece.q13.lock_3_sustain
  - setpiece.q13.core_auditor_arena
level_band: [15, 17]
party_size: [3, 5]
expected_runtime_min: 95
---

# Q13: Heart of the Machine (The Core) (L15-L17, Party 3-5)

## Hook

The Core is not a wilderness or a hallway dungeon. It is a coordination exam. The Gate Ring opens, a machine attendant stamps your warrant, and the Auditor begins counting.

Disable three heart locks, then survive the Auditor Arena. The Core will open the path to The Seam only if you prove you can move like a party.

Inputs (design assumptions):

- Safe-square marker language and hazard cadence must be consistent across the entire run.
- Reset pockets are required for wipe recovery and for mark/debt paydown.
- Mid-fight precision objectives must scale by party size (3-person groups cannot spare as much peel/escort duty).
- Optional bargain systems (forbidden shards) must be opt-in with explicit, capped costs and clear paydown rules.
- Healing must not trivialize the dungeon; geometry failures should remain lethal and readable.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q13_heart_machine.state`: `entry` -> `locks` -> `boss` -> `complete` (placeholder)
- `q.q13_heart_machine.locks`: `0..3` (placeholder)
- `q.q13_heart_machine.audit_marks`: `0..N` (placeholder)
- `gate.seam.entry`: `0/1`

Success conditions:

- disable 3 locks
- defeat/complete the auditor setpiece
- claim seam access token at the gate ring

## The Core’s Prime Rule: Movement Is The Mechanic

This quest lives or dies on readability:

- hazard lines must be visible
- safe squares must be consistent
- cadence must be learnable (audio + text)

If players die, it should be because they ignored the grammar, not because it was hidden.

## Rule That Must Be Legible: Audit Marks (mistake counter)

Define one named counter (placeholder):

- `audit_marks` increase when players commit obvious failures:
  - stand in marked zones during pulse windows
  - panic-run through a beam sweep
  - trigger a lock fail condition (wrong sequence)
  - accept forbidden shard bargains (see `core_debt`)
- `audit_marks` are loud (message + icon), with an explicit reason line.

Marks should matter, but not soft-lock the run:

- small marks: increase add pressure or reduce exposure windows slightly
- high marks: auditor phases become harsher; optional "audited run" rewards degrade

### Mark Recovery (reset pockets)

Provide explicit wipe recovery and mark recovery:

- each lock completion unlocks a short reset chamber
- reset chamber removes 1 `audit_marks` (or reduces severity), at a time cost
- reset chamber is also the debt paydown point (if bargains exist)

## Optional System: Forbidden Shards (`core_debt`, capped)

Some parties will chase tempo by taking power from the Core.

- `core_debt` tokens: start at 0; cap at 2.
- taking a shard: immediate boon (extra burst window, skip one add wave, or widen a safe square), then `core_debt += 1`.
- debt consequence: increases hazard intensity in the next Innerworks segment or next boss phase.
- paydown (reset chambers only): spend time (one extra hazard cycle) or forfeit a bonus reward to reduce debt by 1.

Make this explicit so it feels like a deliberate risk, not a delayed trap.

----

## Room Flow

The Core is a linear raid-lite:

1. Gate Ring hub + staging
2. Approach ramp-up
3. Innerworks traversal + lock setpieces (1..3)
4. Pre-arena reset pocket
5. Auditor Arena boss
6. Seam access unlock

### R_CORE_GATE_01 (CL_CORE_GATE_RING, HUB_CORE_GATE)

- beat: staging hub with a "heart board" showing 3 locks (L1/L2/L3) and current `audit_marks`.
- teach: safe-square marker language (use this everywhere); role assignment (solve vs hold)
- quest: set `q.q13_heart_machine.state=entry` (placeholder)
- exits:
  - north -> `R_CORE_APPROACH_01`

### Safe Square Marker Language (must exist)

Pick one marker language and reuse it in every hazard room:

- floor chevrons, pylons, or projected squares that are always safe during beam sweeps.

The player lesson is "stand on the marker when the sweep comes."

----

## Approach (ramp-up)

### R_CORE_APPROACH_01 (CL_CORE_APPROACH)

- beat: add waves in corridors; the Core pressures you to keep moving.
- teach: target priority and clean pulls; the Core punishes chasing into geometry
- exits: north -> `R_CORE_INNER_01`

----

## Innerworks (traversal hazards + locks)

Innerworks is where most wipes happen. Provide:

- short segments between setpieces
- visible cadence (beam sweep, then lull)
- at least one "stop and breathe" nook before each lock

### R_CORE_INNER_01 (CL_CORE_INNERWORKS)

- beat: first beam sweep corridor (low damage, high lesson).
- teach: move on lull; stand on safe squares
- exits: north -> `R_CORE_LOCK_1_01`

### Lock 1: Precision Sequence (rogue moment)

### R_CORE_LOCK_1_01 (setpiece.q13.lock_1_precision)

- beat: a lock console must be solved in sequence while adds pressure.
- teach: role assignment: one solver, others hold; interrupts matter
- quest: `locks += 1`
- exits: north -> `R_CORE_RESET_01`

### R_CORE_RESET_01 (reset pocket after lock 1)

- beat: short chamber with a visible "audit sink" panel.
- effect: remove 1 `audit_marks` and allow `core_debt` paydown (optional) at time cost.
- teach: recovery is deliberate; do not chain-pull while marked
- exits: north -> `R_CORE_INNER_02`

### R_CORE_INNER_02 (CL_CORE_INNERWORKS)

- beat: rotating fields + add trickle.
- teach: stack/spread discipline; do not cluster in overlapping fields
- exits: north -> `R_CORE_LOCK_2_01`

### Lock 2: Geometry + Add Swarms (wizard/bard moment)

### R_CORE_LOCK_2_01 (setpiece.q13.lock_2_geometry)

- beat: safe squares rotate; adds arrive in waves; lock advances only during "clean window".
- teach: cadence calling; movement is the mechanic
- quest: `locks += 1`
- exits: north -> `R_CORE_RESET_02`

### R_CORE_RESET_02 (reset pocket after lock 2)

- beat: second reset chamber (also a checkpoint for wipe recovery).
- effect: remove 1 `audit_marks`; allow `core_debt` paydown
- exits: north -> `R_CORE_INNER_03`

### R_CORE_INNER_03 (CL_CORE_INNERWORKS)

- beat: moving-beam hallway (the melee wipe-maker if unreadable).
- teach: never panic-run; step to safe square and wait for lull
- exits: north -> `R_CORE_LOCK_3_01`

### Lock 3: Sustain + Objective Windows (cleric moment)

### R_CORE_LOCK_3_01 (setpiece.q13.lock_3_sustain)

- beat: sustained pressure while a timed objective must be completed during exposure windows.
- teach: recovery pacing; objective-first play under pressure
- quest: `locks += 1` (now 3/3)
- exits: north -> `R_CORE_RESET_03`

### R_CORE_RESET_03 (pre-arena reset pocket, guaranteed)

- beat: final reset before the arena. Make this unmistakable.
- effect: remove 1 `audit_marks`; allow `core_debt` paydown; last resupply.
- exits: north -> `R_CORE_AUDITOR_GATE_01`

----

## Auditor Arena (setpiece.q13.core_auditor_arena)

Auditor fight should feel like:

- windows + adds + geometry,
- mid-fight precision objective (optional or scaled for party size),
- and the "counted mistakes" theme that never becomes surprise punishment.

### R_CORE_AUDITOR_GATE_01 (CL_CORE_GATE_RING / CL_CORE_AUDITOR_ARENA boundary)

- beat: the board updates to 3/3 locks. Auditor "acknowledges" your mark count.
- quest: set `q.q13_heart_machine.state=boss` (placeholder)
- exits: north -> `R_CORE_AUDITOR_01`

### R_CORE_AUDITOR_01 (setpiece.q13.core_auditor_arena)

- beat: multi-phase boss with rotating safe squares and exposure windows.
- Phase A: add waves; learn geometry cadence.
- Phase B: exposure windows; burst matters; avoid marked zones.
- Phase C: precision interlock disable (optional or scaled) that shortens the fight if executed cleanly.
- quest: set `q.q13_heart_machine.state=complete` (placeholder); set `gate.seam.entry=1`
- exits: south -> `R_CORE_GATE_01`

## Scaling Notes

- Party size 3: reduce simultaneous mechanics in Phase C or make the interlock disable optional (reward, not requirement).
- All-melee: ensure safe squares aren’t only at long range; provide funnels and clear stand spots.
- No-healer: ensure the run is winnable via perfect movement; chip damage must be avoidable through play.

## NPCs

- Gate Attendant Construct: stamps warrants; can optionally trade hints for marks (future).
- The Auditor: communicates via mark messages and phase transitions; never "gotcha" punishes.

## Rewards

- `gate.seam.entry=1` (The Seam travel unlocked)
- Optional: "Audited Run" scoring rewards (future): fewer marks = better loot/rep

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q13-heart-of-the-machine/`):

- Safe-square marker language and hazard cadence must be consistent across the entire run.
- Reset pockets are required for wipe recovery and for mark/debt paydown.
- Mid-fight precision objectives should scale by party size (3-person groups can’t spare peel).
- Optional bargain systems (forbidden shards) are fun only when capped and explicitly paid down.
- Healing must not trivialize the dungeon; geometry failures should remain lethal.
