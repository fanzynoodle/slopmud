---
adventure_id: q8-flow-treaty
area_id: A01
zone: The Reservoir
clusters:
  - CL_RES_PUMP_STATION
  - CL_RES_SHORES
  - CL_RES_TUNNELS
  - CL_RES_CONTROL_NODES
  - CL_RES_FLOW_REGULATOR
hubs:
  - HUB_RES_PUMP_STATION
setpieces:
  - setpiece.q8.control_node_1
  - setpiece.q8.control_node_2
  - setpiece.q8.control_node_3
  - setpiece.q8.flow_regulator_chamber
level_band: [8, 11]
party_size: [3, 5]
expected_runtime_min: 50
---

# Q8: Flow Treaty (Reservoir) (L8-L11, Party 3-5)

## Hook

The Reservoir is the first place in Gaia that feels like it can drown you slowly: wide shores with clean sightlines for enemies, and tight tunnels that punish sloppy pulls. The pump station crew needs three control nodes restored before The Flow Regulator locks the whole system into a hostile "safe mode".

Do the work, defeat the regulator, and you earn a new route into The Underrail plus a permanent Reservoir <-> Factory shortcut.

Inputs (design assumptions):

- The pump station hub must be an explicit safe reset pocket with clear node progress UI (`controls_restored=0/3`).
- Control node setpieces must have loud, consistent interaction UX (start/interrupted/restored) and party-size scaling knobs.
- Runoff/deep-water hazards must be visually loud and consistent; safe squares should be markable in text so bots and humans can coordinate.
- The maintenance hatch shortcut must be a real, repeatable route with a single symbol language and clear hints.
- Boss "pressure surge" must have a readable grammar (cast bar + audio cue); missing one interrupt hurts but the first miss should not be an instant wipe.
- Adds must matter enough to teach target priority even for over-leveled parties; never let the boss become pure single-target DPS.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q8_flow_treaty.state`: `entry` -> `controls` -> `boss` -> `complete`
- `q.q8_flow_treaty.controls_restored`: `0..3`
- `gate.underrail.entry`: `0/1`
- `gate.reservoir.shortcut_to_factory`: `0/1`

Success conditions:

- restore 3 control nodes
- defeat The Flow Regulator
- report back at the pump station (treaty choice / unlocks)

## Room Flow

Reservoir should teach a simple rule set:

- Shores = ranged pressure + wide pulls; use cover and disciplined pulls.
- Tunnels = ambush pressure; use line-of-sight corners and retreat lanes.
- Water hazards are readable: shallow runoff is "bad squares" (stacking debuff), deep water is "no-go" unless explicitly safe.
- Objectives are explicit: Node-1/2/3 signage is consistent across the zone.

### R_RES_PUMP_01 (CL_RES_PUMP_STATION, HUB_RES_PUMP_STATION)

- beat: pump station hub. Crew explains: three nodes, any order, then regulator chamber.
- teach: multi-objective run with return-to-hub navigation
- quest: set `q.q8_flow_treaty.state=entry`
- exits:
  - west -> `R_RES_SHORE_01` (shore route)
  - east -> `R_RES_TUNNEL_01` (tunnel route)
  - up -> `R_RES_TUNNEL_03` (mixed route)
  - north -> `R_RES_REG_GATE_01` (sealed until 3 nodes)

### Secret: Maintenance Hatch (Quiet Route)

There is one consistent shortcut that makes the Reservoir feel "learnable":

- `R_RES_HATCH_01` connects `R_RES_SHORE_02` to `R_RES_TUNNEL_02`.

It should be hinted by:

- a repeating hatch icon on signage
- audible pump hum near the wall
- an NPC line at the pump station: "If you get pinned in tunnels, find a hatch. They all share the same symbol."

### R_RES_HATCH_01 (maintenance hatch, secret connector)

- beat: a narrow maintenance hatch with the repeating hatch icon stamped on the frame. The hum of pumps is louder here.
- teach: the reservoir is learnable; the secret is navigation competence, not loot
- exits: west -> `R_RES_SHORE_02`, east -> `R_RES_TUNNEL_02`

----

## Control Node 1 (Shores: ranged pressure + cover play)

### R_RES_SHORE_01 (CL_RES_SHORES)

- beat: wide shoreline approach. Enemies try to peel the backline from long range.
- teach: break LOS using rocks and berms; don’t over-pull in open space
- exits: west -> `R_RES_SHORE_02`

### R_RES_SHORE_02 (CL_RES_SHORES)

- beat: cover pockets appear. Players learn to "own" a pocket before interacting.
- teach: safe squares and reset points
- exits:
  - north -> `R_RES_NODE1_01`
  - secret -> `R_RES_HATCH_01`

### R_RES_NODE1_01 (setpiece.q8.control_node_1, CL_RES_CONTROL_NODES)

- beat: repair console objective. Starting the repair triggers light waves until it completes.
- teach: defend-interact-under-pressure; assign roles
- quest: `q.q8_flow_treaty.controls_restored += 1`
- exits: south -> `R_RES_PUMP_01` (return)

----

## Control Node 2 (Tunnels: ambush + runoff hazard)

### R_RES_TUNNEL_01 (CL_RES_TUNNELS)

- beat: tight corridor. Ambush pack tries to pin and split the party.
- teach: retreat lanes; line-of-sight corners are your friend
- exits: east -> `R_RES_TUNNEL_02`

### R_RES_TUNNEL_02 (CL_RES_TUNNELS)

- beat: shallow runoff channels cross the floor.
- teach: hazard literacy; standing in runoff applies a stacking debuff that must be respected
- exits:
  - north -> `R_RES_NODE2_01`
  - secret -> `R_RES_HATCH_01`

### R_RES_NODE2_01 (setpiece.q8.control_node_2, CL_RES_CONTROL_NODES)

- beat: "purge then repair" objective. Runoff spikes during waves; safe squares are marked.
- teach: movement discipline; don’t greed the interact while standing in hazards
- quest: `q.q8_flow_treaty.controls_restored += 1`
- exits: south -> `R_RES_PUMP_01` (return)

Optional side-room (for druid/nature flavor):

- a sump eel nest that can be calmed instead of fought; reward is a small reduction in ambush density on the return path.

----

## Control Node 3 (Mixed: commit under fire + objective pressure)

### R_RES_TUNNEL_03 (CL_RES_TUNNELS)

- beat: a fork where one path is fast but exposed and the other is slower but safer.
- teach: route choice as difficulty selection
- exits: north -> `R_RES_NODE3_01`, south -> `R_RES_PUMP_01`

### R_RES_NODE3_01 (setpiece.q8.control_node_3, CL_RES_CONTROL_NODES)

- beat: jammed pump wheel objective. Someone must commit to turning it while adds pressure the room.
- teach: commitment under fire; peel discipline; clean pulls
- quest: `q.q8_flow_treaty.controls_restored += 1`
- exits: south -> `R_RES_PUMP_01` (return)

----

## Boss Gate

When `q.q8_flow_treaty.controls_restored == 3`, unseal the regulator gate and mark the objective clearly at the pump station.

### R_RES_REG_GATE_01 (CL_RES_PUMP_STATION)

- beat: the crew says: "Now. Before it locks again."
- quest: set `q.q8_flow_treaty.state=controls`
- exits: north -> `R_RES_REG_CHAMBER_01`

### R_RES_REG_CHAMBER_01 (setpiece.q8.flow_regulator_chamber, CL_RES_FLOW_REGULATOR)

- beat: The Flow Regulator uses a pressure gauge mechanic.
  - It periodically casts a "pressure surge" that must be interrupted.
  - If the surge completes, water level rises and safe squares shrink until a reset window.
  - Add waves spawn on pressure thresholds; ignoring them causes snowball wipes.
- teach: interrupts as a team skill; target priority; don’t tunnel boss
- quest: set `q.q8_flow_treaty.state=boss` then `complete`
- exits: south -> `R_RES_REWARD_01`

### R_RES_REWARD_01 (CL_RES_PUMP_STATION, HUB_RES_PUMP_STATION)

- beat: treaty debrief. Players pick terms (flavor now; rep later) and receive access tokens.
- quest:
  - set `gate.underrail.entry=1`
  - set `gate.reservoir.shortcut_to_factory=1`
- exits:
  - west -> `R_RES_SHORE_01`
  - east -> `R_RES_TUNNEL_01`
  - south -> (future) Factory shortcut
  - north -> (future) Underrail entry

## NPCs

- Pump Station Crew Lead: calm and procedural; explains node IDs and objective mechanics.
- The Flow Regulator: "speaks" via cast bars and pressure gauge; its surge is its voice.

## Rewards

- `gate.underrail.entry=1` (alternate entry into The Underrail)
- `gate.reservoir.shortcut_to_factory=1` (permanent Reservoir <-> Factory shortcut)
- Treaty terms (future): rep flavor and access pricing / vendor behavior

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q8-flow-treaty/`):

- Control nodes need party-size scaling knobs (interact time, wave size, hazard intensity) for 3-player parties.
- Runoff hazards must be visually loud and consistent; safe squares should be marked like Factory belts.
- The maintenance hatch should be a real, repeatable shortcut with a single symbol language.
- Boss fight must be legible: cast bar + audio cue for "pressure surge".
- Boss fight must be legible: missing an interrupt should hurt, but the first miss should not be an instant wipe.
- Boss fight must be legible: adds must matter enough to teach target priority even for over-leveled parties.
