---
adventure_id: q10-anchor-the-wind
area_id: A01
zone: Skybridge
clusters:
  - CL_SKY_ANCHORHOUSE
  - CL_SKY_SPANS
  - CL_SKY_ANCHORS
  - CL_SKY_WIND_MARSHAL
hubs:
  - HUB_SKYBRIDGE_ANCHORHOUSE
setpieces:
  - setpiece.q10.anchor_1
  - setpiece.q10.anchor_2
  - setpiece.q10.anchor_3
  - setpiece.q10.wind_marshal_span
level_band: [11, 13]
party_size: [3, 5]
expected_runtime_min: 65
---

# Q10: Anchor The Wind (Skybridge) (L11-L13, Party 3-5)

## Hook

Skybridge is how you cross distances that used to take days. The anchors keep it from becoming a death sentence. Three anchors are failing. The Wind Marshal is enforcing a "closure protocol" that looks a lot like murder. Repair the anchors, then end the Marshal.

Inputs (design assumptions):

- Gust timing must have a readable, learnable grammar (audio + text cues) so players can plan movement windows.
- Falling/edge hazards must be clearly signaled and recoverable (avoid pure gotchas; provide an explicit recovery loop if you slip).
- Anchor interaction UX must be loud and consistent (start/interrupted/complete) with a visible `anchors_repaired=0/3` counter.
- Melee parties need cover language and lure tools for far adds so span fights are not "ranged tax".
- Optional wind pact bargains must be opt-in with explicit, capped costs and clear paydown rules.
- Bot autofill must be edge-aware and gust-aware (never chase to rails during gust windows).

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q10_anchor_wind.state`: `entry` -> `anchors` -> `boss` -> `complete` (key name placeholder; align later)
- `q.q10_anchor_wind.anchors_repaired`: `0..3` (placeholder)
- `gate.glass_wastes.entry`: `0/1`

Success conditions:

- repair 3 anchors
- defeat the Wind Marshal
- unlock Glass Wastes access

## Room Flow

Skybridge should feel like:

- traversal pressure (gust windows),
- lethal edges (positioning discipline),
- long sightlines (ranged threats matter),
- objectives that require calm (crank/repair during waves),
- a boss whose "damage" is mostly geometry.

### R_SKY_ANCHOR_01 (CL_SKY_ANCHORHOUSE, HUB_SKYBRIDGE_ANCHORHOUSE)

- beat: anchorhouse hub with three anchor IDs (A1/A2/A3) on a board. A warning sign: "Do not fight near rails."
- teach: 3/3 anchors required before marshal span opens
- exits:
  - north -> `R_SKY_SPAN_01` (to A1)
  - east -> `R_SKY_SPAN_02` (to A2)
  - west -> `R_SKY_SPAN_03` (to A3)
  - south -> `R_SKY_MARSHAL_GATE_01` (sealed until 3/3)

### R_SKY_SPAN_01 (CL_SKY_SPANS)

- beat: exposed span segment. Gust windows are called out with audio cues and a short, consistent text banner.
- teach: gust timing; "wait, then move" beats
- exits: north -> `R_SKY_A1_01`, south -> `R_SKY_ANCHOR_01`

### R_SKY_SPAN_02 (CL_SKY_SPANS)

- beat: a longer span with windbreak panels. Ranged pressure teaches cover and safe squares.
- teach: cover discipline; pull back to panels
- exits: east -> `R_SKY_A2_01`, west -> `R_SKY_ANCHOR_01`, secret -> `R_SKY_SCAFFOLD_01`

### R_SKY_SPAN_03 (CL_SKY_SPANS)

- beat: the worst wind span. The rail edge is loud and obvious; the encounter teaches "never chase to edge".
- teach: positioning discipline; lure adds into safe squares
- exits: west -> `R_SKY_A3_01`, east -> `R_SKY_ANCHOR_01`, secret -> `R_SKY_SCAFFOLD_01`

### Safe Pocket

After anchor 2 is repaired, unlock a small safe pocket in/near the anchorhouse:

- reduces wipe pain
- supports under-leveled/no-healer parties

### Secret: Side Scaffold

One consistent secret bypasses the most exposed span segment:

- `R_SKY_SCAFFOLD_01` connects `R_SKY_SPAN_02` to `R_SKY_SPAN_03` with better cover.

This is the Skybridge secret: navigation that feels like competence.

### R_SKY_SCAFFOLD_01 (side scaffold, secret)

- beat: scaffold walkway with better cover but tighter footing. Safer from ranged pressure, worse during gusts.
- teach: tradeoffs; secret routes are competence, not free wins
- exits: east -> `R_SKY_SPAN_02`, west -> `R_SKY_SPAN_03`

----

## Anchor 1 (teach repair UX)

### R_SKY_A1_01 (setpiece.q10.anchor_1)

- beat: stabilize the room, then start the anchor repair (crank). Waves arrive in fixed cadence; gust windows interrupt careless attempts.
- teach: interact UX; wait out gust windows
- quest: `anchors_repaired += 1`
- exits: back -> `R_SKY_SPAN_01`

## Anchor 2 (traversal + ranged pressure)

### R_SKY_A2_01 (setpiece.q10.anchor_2)

- beat: long sightline approach with ranged pressure. Repair requires holding a safe square behind windbreak panels.
- teach: cover discipline; rotate interact role
- quest: `anchors_repaired += 1`
- exits: back -> `R_SKY_SPAN_02` (and unlock safe pocket)

## Anchor 3 (worst wind, strict positioning)

### R_SKY_A3_01 (setpiece.q10.anchor_3)

- beat: strongest gust windows. Adds spawn to bait you toward rails. Correct play is "never chase to edge."
- teach: positioning discipline; lure adds into safe squares
- quest: `anchors_repaired += 1`
- exits: back -> `R_SKY_SPAN_03`

----

## Boss Gate

When `anchors_repaired == 3`, unseal the marshal span.

### R_SKY_MARSHAL_GATE_01 (CL_SKY_ANCHORHOUSE)

- beat: anchorhouse alarms dim; the route to the marshal opens.
- exits: south -> `R_SKY_MARSHAL_01`, north -> `R_SKY_ANCHOR_01`

### R_SKY_MARSHAL_01 (setpiece.q10.wind_marshal_span)

- beat: Wind Marshal fight on a long span. Gust phases, add calls, and knockback define the encounter.
- teach: phase discipline; add priority; never fight on rails
- quest: set boss complete; unlock Glass Wastes
- exits: north -> `R_SKY_REWARD_01`, south -> `R_SKY_MARSHAL_GATE_01`

### R_SKY_REWARD_01 (post-boss anchorhouse)

- beat: anchorhouse issues a stamped route token and opens the wasteland route.
- quest: set `gate.glass_wastes.entry=1`
- exits: north -> `R_SKY_ANCHOR_01`

## NPCs

- Anchorhouse Lead: repeats anchor IDs and gust safety rules.
- The Wind Marshal: "speaks" via gust phases and add calls.

## Rewards

- Glass Wastes entry unlocked
- Side scaffold becomes a permanent shortcut (future)

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q10-anchor-the-wind/`):

- Gust timing must have a readable, learnable grammar (audio + text cues).
- Falling/edge hazards must be clearly signaled and recoverable (avoid pure gotchas).
- Anchor interact UX must be loud and consistent (start/interrupted/complete).
- Melee parties need cover language and lure tools for far adds.
- Optional wind pact bargains are fun if costs are explicit and capped.
- Bot autofill must be edge-aware and gust-aware (never chase to rails during gust windows).
