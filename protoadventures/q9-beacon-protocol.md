---
adventure_id: q9-beacon-protocol
area_id: A01
zone: The Underrail
clusters:
  - CL_UNDERRAIL_PLATFORM
  - CL_UNDERRAIL_TUNNELS
  - CL_UNDERRAIL_SWITCHYARD
  - CL_UNDERRAIL_WARDEN_ARENA
hubs:
  - HUB_UNDERRAIL_PLATFORM
setpieces:
  - setpiece.q9.beacon_1
  - setpiece.q9.beacon_2
  - setpiece.q9.beacon_3
  - setpiece.q9.beacon_4
  - setpiece.q9.platform_warden_arena
level_band: [9, 12]
party_size: [3, 5]
expected_runtime_min: 60
---

# Q9: Beacon Protocol (The Underrail) (L9-L12, Party 3-5)

## Hook

The Underrail is the buried spine of Gaia’s travel network. It mostly works. "Mostly" is not good enough when the line carries people. Relight four beacons to stabilize routing, then defeat the Platform Warden that’s enforcing the wrong rules.

Inputs (design assumptions):

- Beacon charge UX must be loud and consistent in text (start/interrupted/charged/cooldown) with a visible `beacons_lit=0/4` counter.
- Patrol timing must have a readable, learnable grammar (signals, audio cues, fixed loops) so stealth/scouting is skill, not RNG.
- Under-leveled/no-healer parties need at least one safe pocket/checkpoint so wipes do not cascade.
- Melee parties need lure tools for far adds (noise maker, lever, bait) so beacon defenses are not "ranged tax".
- Optional "bind beacon" bargains must be opt-in with explicit, capped costs and clear paydown rules.
- Bot autofill must be ambush- and timing-aware (do not stop in bends; cross on signal windows).

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q9_beacon_protocol.state`: `unstarted` -> `beacons` -> `boss` -> `complete` (key name placeholder; align later)
- `q.q9_beacon_protocol.beacons_lit`: `0..4` (placeholder)
- `gate.skybridge.entry`: `0/1`

Success conditions:

- light 4 beacons
- defeat the Platform Warden
- unlock Skybridge access

## Room Flow

Underrail should feel like:

- long tunnels with ambush discipline ("don’t stop in bends"),
- switchyard timing that is learnable (signals, loop cadence),
- beacon setpieces that punish starting the charge under pressure,
- a boss where add control and phase discipline matter more than raw DPS.

### R_UR_PLATFORM_01 (CL_UNDERRAIL_PLATFORM, HUB_UNDERRAIL_PLATFORM)

- beat: platform hub with four beacon IDs on a board (BEACON-1..4).
- teach: objective sequencing; 4/4 required to open the boss gate
- exits:
  - north -> `R_UR_TUNNEL_01` (to beacon 1)
  - west -> `R_UR_TUNNEL_02` (to beacon 2)
  - east -> `R_UR_TUNNEL_03` (to beacon 3)
  - south -> `R_UR_SWITCH_01` (to beacon 4 and warden gate)

### R_UR_TUNNEL_01 (CL_UNDERRAIL_TUNNELS)

- beat: long tunnel with a bend that invites bad stops. "Don’t stop in bends" signage is painted over old markings.
- teach: ambush discipline; retreat lanes; scouting
- exits: north -> `R_UR_BEACON1_01`, south -> `R_UR_PLATFORM_01`

### R_UR_TUNNEL_02 (CL_UNDERRAIL_TUNNELS)

- beat: a tunnel with patrol overlap and signal lights that teach timing.
- teach: timing is learnable; wait in safe pockets, not in bends
- exits:
  - west -> `R_UR_BEACON2_01`
  - east -> `R_UR_PLATFORM_01`
  - secret -> `R_UR_MAINT_CRAWL_01`

### R_UR_TUNNEL_03 (CL_UNDERRAIL_TUNNELS)

- beat: tight tunnel with loud audio cues before ambush packs. The room description itself is a warning.
- teach: signal literacy; avoid panic pulls
- exits: east -> `R_UR_BEACON3_01`, west -> `R_UR_PLATFORM_01`

### Safe Pocket

After beacon 2 is lit, unlock a small safe pocket back on the platform:

- reduces wipe pain
- supports under-leveled/no-healer parties

### Secret: Maintenance Crawl

One consistent secret bypasses the worst ambush bend or patrol lane:

- `R_UR_MAINT_CRAWL_01` connects tunnel -> switchyard with fewer patrol overlaps.

### R_UR_MAINT_CRAWL_01 (maintenance crawl, secret)

- beat: a cramped crawlspace with fewer patrol overlaps. It smells like ozone and old grease.
- teach: secret routes are competence; the bypass is consistent, not RNG
- exits: south -> `R_UR_SWITCH_01`, back -> `R_UR_TUNNEL_02`

----

## Beacon 1 (defend the point, teach charge UX)

### R_UR_BEACON1_01 (setpiece.q9.beacon_1)

- beat: clear the room, then start a beacon charge timer; waves arrive in a fixed cadence.
- teach: "charge starts a timer"; clear patrols first
- quest: `beacons_lit += 1`
- exits: back -> `R_UR_TUNNEL_01`

## Beacon 2 (patrol overlap lesson)

### R_UR_BEACON2_01 (setpiece.q9.beacon_2)

- beat: beacon room is near a patrol lane. Starting the charge while patrol is active is likely death.
- teach: respect patrol timing signals
- quest: `beacons_lit += 1`
- exits: back -> `R_UR_TUNNEL_02` (and unlock safe pocket)

## Beacon 3 (ambush tax)

### R_UR_BEACON3_01 (setpiece.q9.beacon_3)

- beat: tunnel approach tries to bait you into stopping in bends; beacon room itself is not hard.
- teach: "don’t stop in bends"; scouting rewards
- quest: `beacons_lit += 1`
- exits: back -> `R_UR_TUNNEL_03`

## Beacon 4 (switchyard timing)

### R_UR_SWITCH_01 (CL_UNDERRAIL_SWITCHYARD)

- beat: switchyard hub with signals and patrol loops. Timing is the puzzle.
- teach: learnable rhythm (signal lights, announcements)
- exits: south -> `R_UR_BEACON4_01`, north -> `R_UR_PLATFORM_01`

### R_UR_BEACON4_01 (setpiece.q9.beacon_4)

- beat: charge requires holding a safe square while patrols pass; if you panic-move, you chain-pull.
- teach: discipline; "cross on signal"
- quest: `beacons_lit += 1`
- exits: north -> `R_UR_WARDEN_GATE_01`, back -> `R_UR_SWITCH_01`

----

## Boss Gate

When `beacons_lit == 4`, unseal the warden gate.

### R_UR_WARDEN_GATE_01 (CL_UNDERRAIL_SWITCHYARD)

- beat: the line "stabilizes". The gate mechanism acknowledges beacon state.
- exits: south -> `R_UR_WARDEN_01`, north -> `R_UR_BEACON4_01`

### R_UR_WARDEN_01 (setpiece.q9.platform_warden_arena)

- beat: Platform Warden with phase shifts and add windows. Add caller is the fight.
- teach: target priority; phase discipline; interrupts as team skill
- quest: set boss complete; unlock Skybridge
- exits: north -> `R_UR_REWARD_01`, south -> `R_UR_WARDEN_GATE_01`

### R_UR_REWARD_01 (post-boss platform)

- beat: platform ledger stamps your authorization and opens the Skybridge route.
- quest: set `gate.skybridge.entry=1`
- exits: south -> `R_UR_PLATFORM_01`

## NPCs

- Platform Dispatcher: repeats the rules and beacon IDs; calls out signals.
- The Warden: "speaks" via add calls and terrain hazards.

## Rewards

- Skybridge entry unlocked
- Optional: maintenance crawl shortcut becomes permanent (future)

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q9-beacon-protocol/`):

- Beacon charge UX must be loud and consistent (start/interrupted/charged/cooldown).
- Patrol timing needs a readable, learnable grammar (signals, audio cues, fixed loops).
- Under-leveled/no-healer parties need a safe pocket / checkpoint.
- Melee parties need lure tools for far adds (noise maker / lever / bait).
- Optional "bind beacon" bargains are fun if costs are explicit and capped.
- Bot autofill must be ambush- and timing-aware (don’t stop in bends; cross on signal).
