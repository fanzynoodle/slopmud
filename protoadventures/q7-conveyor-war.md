---
adventure_id: q7-conveyor-war
area_id: A01
zone: Factory District (Outskirts)
clusters:
  - CL_FACTORY_GATE
  - CL_FACTORY_SWITCHROOM
  - CL_FACTORY_CONVEYORS
  - CL_FACTORY_FOUNDRY
  - CL_FACTORY_FOREMAN_ARENA
hubs:
  - HUB_FACTORY_SWITCHROOM
setpieces:
  - setpiece.q7.line_shutdown_1
  - setpiece.q7.line_shutdown_2
  - setpiece.q7.line_shutdown_3
  - setpiece.q7.foreman_clank_arena
level_band: [7, 10]
party_size: [3, 5]
expected_runtime_min: 45
---

# Q7: Conveyor War (Factory District) (L7-L10, Party 3-5)

## Hook

The factory is producing the wrong things, at the wrong volume, for the wrong people. The switchroom crew has a plan: disable three conveyor lines to force a shutdown, then take down Foreman Clank while the system is unstable.

Inputs (design assumptions):

- Factory navigation must have a consistent signage grammar (line IDs, color bands, floor arrows, sirens).
- "Line shutdown" setpieces must share a standard interface (trigger -> waves -> completion -> persistent state -> back-to-hub).
- Turret pressure must have at least one non-caster counterplay path (cover objects, shield consoles, smoke/steam valves, vents).
- Belts/knockback must be readable in text-only form via "safe squares" that bots and humans can coordinate on.
- Optional "overclock" bargains must be opt-in with explicit, persistent costs (debt, debuff, rep).
- Include at least one checkpoint unlock mid-run (after shutdown 1 or 2) so wipes don't restart the whole factory.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q7_conveyor_war.state`: `entry` -> `shutdown` -> `boss` -> `complete`
- `q.q7_conveyor_war.lines_disabled`: `0..3`
- `gate.underrail.entry`: `0/1`
- `gate.factory.shortcut_to_reservoir`: `0/1`

Success conditions:

- disable three lines (three setpieces)
- defeat Foreman Clank
- unlock Underrail entry

## Room Flow

Factory should feel like a place with rules:

- LOS matters (turrets, tight corridors).
- Geometry matters (belts, knockback, safe squares).
- Objectives are explicit (line IDs, levers, alarms).

### R_FAC_GATE_01 (CL_FACTORY_GATE)

- beat: entry streets and signage. Everything is labeled with line IDs (1/2/3) and warning bands.
- teach: factory signage grammar; "don’t fight on belts unless forced"
- exits: north -> `R_FAC_SWITCH_01`

### R_FAC_SWITCH_01 (CL_FACTORY_SWITCHROOM, HUB_FACTORY_SWITCHROOM)

- beat: switchroom hub. The crew explains the three shutdown points and marks them on a wall map.
- teach: multi-objective dungeon run with return-to-hub logic
- feedback: always show `lines_disabled=0/3` (and update on return)
- note: after shutdown 2, unlock a safer return corridor/checkpoint so wipes don't reset the whole run
- quest: set `q.q7_conveyor_war.state=entry`
- exits:
  - west -> `R_FAC_ROUTE_01` (to line 1)
  - east -> `R_FAC_ROUTE_02` (to line 2)
  - north -> `R_FAC_ROUTE_03` (to line 3)
  - south -> `R_FAC_ARENA_GATE_01` (sealed until 3 lines)

### Secret: Quiet Route

There is one consistent secret path that bypasses a dangerous belt segment:

- `R_FAC_CATWALK_01` (hidden door / vent route) -> reconnects near line 3.

This is the factory's "secret": smart navigation, not a hidden treasure chest.

----

## Line Shutdown 1 (tight corridor + turret pressure)

### R_FAC_ROUTE_01 (CL_FACTORY_CONVEYORS)

- beat: tight corridor with turret pressure; players learn to break LOS.
- teach: corner play; pull discipline
- note: include at least one non-caster counterplay object (cover console, shield panel, smoke valve) so this isn't "caster tax"
- exits: west -> `R_FAC_LINE1_01`

### R_FAC_LINE1_01 (setpiece.q7.line_shutdown_1)

- beat: console + lever. Pulling the lever starts a wave timer; complete by surviving to the end and flipping the lockout.
- teach: defend objective under pressure; role assignment
- feedback: show a clear setpiece state machine in text (armed -> waves -> lockout -> complete) and a persistent `lines_disabled=1/3` update
- quest: `lines_disabled += 1`
- exits: east -> `R_FAC_SWITCH_01`

Optional sub-objective (rescue-first parties):

- a side door with trapped workers that can be rescued before the lever (time cost, rep reward later).

----

## Line Shutdown 2 (belts + foundry heat)

### R_FAC_ROUTE_02 (CL_FACTORY_CONVEYORS)

- beat: moving belts and knockback. Safe squares are marked. Belt-runners try to chain-pull.
- teach: geometry as a mechanic
- exits: east -> `R_FAC_FOUNDRY_01`

### R_FAC_FOUNDRY_01 (CL_FACTORY_FOUNDRY)

- beat: heat vents apply a stacking burn-like status; the room wants you to keep moving.
- teach: hazard literacy; status management
- exits: east -> `R_FAC_LINE2_01`

### R_FAC_LINE2_01 (setpiece.q7.line_shutdown_2)

- beat: shutdown requires disabling two locks (one can be done "quietly" by a rogue/scout route).
- teach: split duties without splitting the party
- quest: `lines_disabled += 1`
- exits: west -> `R_FAC_SWITCH_01`

----

## Line Shutdown 3 (quiet route payoff + add discipline)

### R_FAC_ROUTE_03 (CL_FACTORY_CONVEYORS)

- beat: belt maze with a clearly-suggested vent/catwalk alternate route.
- teach: reward navigation; punish stubbornness
- exits: north -> `R_FAC_LINE3_01`

### R_FAC_LINE3_01 (setpiece.q7.line_shutdown_3)

- beat: add wave is the main threat. If players pull extra packs into the room, "alert level" escalates and spawns an extra bruiser.
- teach: clean pulls; discipline
- quest: `lines_disabled += 1`
- exits: south -> `R_FAC_SWITCH_01`

----

## Boss Gate

When `lines_disabled == 3`, unseal the arena gate.

### R_FAC_ARENA_GATE_01 (CL_FACTORY_SWITCHROOM)

- beat: alarms dim. The crew says: "Now. Before it recovers."
- quest: set `q.q7_conveyor_war.state=shutdown`
- exits: south -> `R_FAC_ARENA_01`

### R_FAC_ARENA_01 (setpiece.q7.foreman_clank_arena)

- beat: Foreman Clank + add caller mechanic. If the add caller is not interrupted/killed, adds snowball.
- teach: target priority; interrupts as a team skill; don’t tunnel boss
- quest: set `q.q7_conveyor_war.state=boss` then `complete`
- exits: north -> `R_FAC_REWARD_01`

### R_FAC_REWARD_01 (post-boss hub)

- beat: the crew stamps your shutdown proof and hands you an Underrail access token.
- quest:
  - set `gate.underrail.entry=1`
  - set `gate.factory.shortcut_to_reservoir=1` (late unlock)
- exits:
  - north -> `R_FAC_SWITCH_01`
  - south -> (future) Underrail entry

## NPCs

- Switchroom Crew Lead: pragmatic, safety-minded; explains line IDs and objectives.
- Foreman Clank: speaks through mechanics; the add call is its "voice".

## Rewards

- Underrail entry unlocked
- Factory-to-Reservoir shortcut unlocked (late)
- Optional: worker rescue rep bump (future)

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q7-conveyor-war/`):

- Conveyors are the real boss unless telegraphs are explicit.
- Turret corridors need at least one non-caster counterplay route (cover, vents, shield consoles).
- Small parties need bot autofill that understands belts (belt-aware pathing + simple commands).
- Overclock/bargain consoles are fun, but only if costs are explicit and persistent.
