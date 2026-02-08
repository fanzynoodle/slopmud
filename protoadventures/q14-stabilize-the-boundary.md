---
adventure_id: q14-stabilize-the-boundary
area_id: A01
zone: The Seam
clusters:
  - CL_SEAM_EDGE_STATION
  - CL_SEAM_RIFTS
  - CL_SEAM_MAZE
  - CL_SEAM_STABILIZER_NEXUS
hubs:
  - HUB_SEAM_EDGE
setpieces:
  - setpiece.q14.rift_1
  - setpiece.q14.rift_2
  - setpiece.q14.rift_3
  - setpiece.q14.rift_4
  - setpiece.q14.stabilizer_nexus
level_band: [17, 20]
party_size: [3, 5]
expected_runtime_min: 90
---

# Q14: Stabilize the Boundary (The Seam) (L17-L20, Party 3-5)

## Hook

The Seam is where Gaia's rules fray. Rifts open and reality becomes "wrong": gravity skews, silence pockets bloom, casts misfire, and geometry collapses. You are not here to brute-force mobs. You are here to stabilize rifts before they widen the boundary.

Close four rifts, choose what the Seam reinforces, then enter the stabilizer nexus and confront the thing that is teaching reality new bad habits.

Inputs (design assumptions):

- Rule changes must be named and readable; otherwise they feel like bugs.
- Rift stabilization must have explicit progress UI and collapse timers (players should always know what is happening).
- Blind spots and safe pockets must be discoverable and consistent; waiting must be explicitly safe in anchor pockets.
- Trio scaling and partial-progress persistence are required to prevent endgame frustration.
- Temptations must be opt-in with loud, explicit, capped costs (and clear paydown rules if debt exists).

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q14_stabilize_boundary.state`: `rifts` -> `choice` -> `final` -> `complete`
- `q.q14_stabilize_boundary.rifts`: `0..4`
- `q.q14_stabilize_boundary.reinforce`: `unset`, `red_shift`, `blue_noise`, `glass_choir`
- `gate.A41`, `gate.A42`, `gate.A43`: future area gates (placeholders)

Success conditions:

- stabilize rifts 1..4
- pick a reinforcement target at the edge station
- defeat the stabilizer nexus encounter
- report in at the edge station

## Room Flow

Seam should teach "rules are part of the fight":

- Each rift has a named rule-modifier with explicit telegraphs and a readable UI banner.
- Rift completion is about stabilization progress, not kill count.
- Every rift has a safe pocket or anchor mechanic so chaos is solvable.
- Trio scaling exists: collapse timers and required anchors adjust for party size 3, and partial progress can persist after wipes.

### Seam Counters (two small systems, both capped)

**`rift_instability` (per rift run):** a short-lived counter that rises when you fail the rift’s rule (panic-run, stand in the wrong zone, spam into choir) and makes the current rift harder (more adds, shorter windows). This should reset when the rift is closed.

**`reality_debt` (optional temptations):** capped at 2. Temptations grant tempo or power *now* and worsen the next rift or the next nexus phase. Debt can be paid down at the edge station by spending time or forfeiting a contract bonus.

### R_SEAM_EDGE_01 (CL_SEAM_EDGE_STATION, HUB_SEAM_EDGE)

- beat: edge station hub. The crew explains four rifts and the stabilizer nexus.
- teach: read rule banners; stabilize first, fight second
- quest: set `q.q14_stabilize_boundary.state=rifts`
- exits:
  - north -> `R_SEAM_RIFT_ROUTE_01` (rift routes)
  - south -> `R_SEAM_CHOICE_01` (locked until rifts==4)

### R_SEAM_RIFT_ROUTE_01 (CL_SEAM_RIFTS)

- beat: rift route hub. Four stabilized paths branch out from the station, each marked with a distinct icon and warning banner.
- teach: choose a rift; retreat is always possible; the edge station is the reset
- exits:
  - south -> `R_SEAM_EDGE_01`
  - north -> `R_SEAM_RIFT1_01`
  - west -> `R_SEAM_RIFT2_01`
  - east -> `R_SEAM_RIFT3_01`
  - up -> `R_SEAM_RIFT4_01`

### Rule Banner (required)

When entering a rift radius, show:

- rift name (e.g. "RIFT: BLUE NOISE")
- 1-2 rule changes (e.g. "abilities double-fire", "movement is slippery")
- the stabilization objective and timer
- whether waiting is safe (anchor pocket: safe wait; outside: escalation)

Also use the same banner format for every nexus phase swap (audio stinger + UI banner).

----

## Rift 1 (gravity skew)

### R_SEAM_RIFT1_01 (setpiece.q14.rift_1, CL_SEAM_RIFTS)

- beat: gravity skew changes movement and knockback behavior.
- objective: stabilize a node during a collapse window.
- teach: move on cadence; fight in safe pockets
- anchor pocket: one clearly-marked pocket where knockback is reduced and safe squares are consistent
- quest: `q.q14_stabilize_boundary.rifts += 1`
- exits: back -> `R_SEAM_RIFT_ROUTE_01`

## Rift 2 (blue noise: silence pockets)

### R_SEAM_RIFT2_01 (setpiece.q14.rift_2)

- beat: silence pockets bloom and drift. Casters must reposition; melee can anchor pockets.
- objective: stabilize three anchors (trio: two) before timer ends.
- teach: role adaptation; repositioning without panic
- anchor pocket: a stable "quiet edge" nook that is safe to wait in (no escalation)
- quest: `rifts += 1`
- exits: back -> `R_SEAM_RIFT_ROUTE_01`

## Rift 3 (blind spots + collapsing geometry)

### R_SEAM_RIFT3_01 (setpiece.q14.rift_3)

- beat: collapsing geometry. "Blind spots" are the only safe path to the node.
- objective: reach the node via blind spots and stabilize under pressure.
- teach: discovery and pattern recognition, not random wall-humping
- blind spot tells: consistent marker language + banner hint (not random)
- quest: `rifts += 1`
- exits: back -> `R_SEAM_RIFT_ROUTE_01`

## Rift 4 (glass choir: cast punishment)

### R_SEAM_RIFT4_01 (setpiece.q14.rift_4)

- beat: repeated casting triggers a punishment pulse ("choir"). Parties must vary actions or rotate roles.
- objective: stabilize while managing pulse cadence; safe pocket can be unlocked mid-fight.
- teach: coordination and restraint; explicit opt-in temptation should exist but be capped
- counterplay: banner should explicitly say "rotate actions, vary roles, wait on lulls"
- quest: `rifts += 1`
- exits: back -> `R_SEAM_RIFT_ROUTE_01`

### Partial Progress (quality-of-life)

If a rift requires multiple anchors:

- record partial progress (e.g. 1/3 anchors remain stabilized after a wipe) to avoid full resets for trios.
- allow partial progress to persist within the same rift closure attempt (optional: persist across wipes as 1/3 kept).

----

## Choice Gate (reinforcement target)

After `rifts == 4`, unlock the reinforcement choice at the edge station.

### R_SEAM_CHOICE_01 (CL_SEAM_EDGE_STATION)

- beat: pick what the Seam reinforces:
  - `red_shift`: brute stability; more physical anchor options
  - `blue_noise`: control stability; more safe pockets and anti-caster punishment
  - `glass_choir`: restraint stability; more cadence and anti-spam rewards
- quest: set `q.q14_stabilize_boundary.reinforce=...` and set `state=choice`
- note: this choice should be concrete (changes what the nexus phases reward and what anchor pockets look like), not just flavor
- exits: south -> `R_SEAM_NEXUS_GATE_01`

----

## Stabilizer Nexus (finale)

### R_SEAM_NEXUS_GATE_01 (CL_SEAM_STABILIZER_NEXUS)

- beat: staging line; the crew warns: "It will change the rules mid-fight."
- quest: set `q.q14_stabilize_boundary.state=final`
- optional: edge station offers a "pay down reality debt" action here (time or forfeit bonus)
- exits: south -> `R_SEAM_NEXUS_01`

### R_SEAM_NEXUS_01 (setpiece.q14.stabilizer_nexus)

- beat: reality-glitch boss with phase swaps that change rules.
  - phase swaps must be telegraphed with an audio stinger + UI banner
  - temptations (power offers) may appear, but are explicit and capped
- teach: read, adapt, stabilize; don't brute force unknown phases
- quest: set `q.q14_stabilize_boundary.state=complete`
- exits: north -> `R_SEAM_EDGE_01`

## NPCs

- Edge Station Stabilizer Lead: clinical and exhausted; teaches rule banners and stabilization priority.
- The Nexus Entity: communicates via rule swaps, temptation offers, and phase stingers.

## Rewards

- Future seam gates (`gate.A41..A43`) can be toggled later depending on reinforce choice.

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q14-stabilize-the-boundary/`):

- Rule changes must be named and readable; otherwise it feels like bugs.
- Rift stabilization must have explicit progress UI and collapse timers.
- Blind spots and safe pockets must be discoverable and consistent.
- Trio scaling and partial-progress persistence prevent endgame frustration.
- Temptations are fun only when costs are loud, explicit, and capped.
- Waiting needs to be explicitly safe in anchor pockets (and explicitly unsafe outside them, if escalation exists).
