---
adventure_id: side-sewer-drone-rescue
area_id: A01
zone: Under-Town Sewers (Optional Wing)
clusters:
  - CL_SEWERS_JUNCTION
  - CL_SEWERS_ENTRY
  - CL_SEWERS_SLUDGE_LOOPS
hubs:
  - HUB_SEWERS_JUNCTION
setpieces:
  - setpiece.side.drone_extract
  - setpiece.side.flooded_valve_sequence
  - setpiece.side.keycard_shortcut
level_band: [3, 5]
party_size: [1, 5]
expected_runtime_min: 40
---

# Side Quest: Sewer Drone Rescue (Sewers Optional Wing) (L3-L5, Party 1-5)

## Hook

Maintenance lost a drone in an optional wing. It is wedged, damaged, and panicking. Bring it back to the junction intact enough to reboot. Do this and you unlock a small utility vendor: parts, antidotes, and "sewer-sense" supplies that make future runs less miserable.

This is an escort quest that should feel fair: readable panic rules, visible safe lanes, and a clear shortcut lane for small parties.

Inputs (design assumptions):

- Panic must be predictable: meter + thresholds + explicit calm cooldown.
- Escort pathing must prefer marked safe lanes and avoid obvious hazard suicide routes.
- Small parties and solos need an explicit shortcut lane; default lane is otherwise too punishing.
- Flooded valve sequence must have loud correct/incorrect feedback and explicit completion messaging.
- There must be at least one safe pocket where waiting is explicitly safe (no escalation).
- Reward tiers should be explicit (base unlock vs discount tier vs route knowledge).

## Quest Line

Success conditions:

- locate the drone
- extract it from the debris
- escort it to the junction (`HUB_SEWERS_JUNCTION`)
- unlock utility vendor access (base)

Optional outcomes:

- faster delivery + low drone damage -> discount tier
- keycard shortcut discovered -> permanent route knowledge (and future repeatability)

## Rules That Must Be Legible

### Drone Panic (meter + calm)

Define one named meter (placeholder):

- `drone_panic` rises when the drone takes damage (especially splash) or enters hazard tiles.
- at thresholds, the drone attempts to flee into side corridors (danger).
- `calm` interaction reduces panic and prevents fleeing for a short duration.
- `calm` has a cooldown; show it explicitly.

Escort must feel deterministic:

- players should predict panic before it happens
- fleeing should be preventable by play (not random)

### Safe Lanes (markers)

Use a consistent marker language for "escort-safe lanes":

- paint stripes, bolts, or maintenance arrows that indicate the intended path.
- the drone pathing should prefer marked lanes (avoid suicide routes).

## Safe Pocket (required)

Include one guaranteed safe pocket in the wing:

- a dry maintenance alcove where the drone can wait safely
- no escalation while waiting
- a place to recover, read rules, and plan the flooded section

## Secret (navigation reward): Keycard Shortcut Lane

One explicit shortcut bypasses the worst flooded segment:

- obtain a one-use keycard from a maintenance console at an explicit cost (money/rep/time)
- open a locked maintenance door that skips the flooded section hazards

This lane should be discoverable, optional, and clearly described.

----

## Room Flow

This side quest is linear with one major branch (shortcut vs flooded valves).

### Entry: R_SEW_JUNC_DRONE_01 (HUB_SEWERS_JUNCTION)

- beat: at the junction, a side sign reads "OPTIONAL WING: DRONE RECOVERY."
- teach: escort rules exist; this is not a normal clear-and-move room
- exits:
  - east -> `R_SEW_WING_01`

### R_SEW_WING_01 (optional wing approach)

- beat: narrow corridors; first ambush.
- teach: scout/clear before moving escort
- exits: east -> `R_SEW_WING_02`, south -> `R_SEW_SAFE_01`

### R_SEW_WING_02 (drone location, setpiece.side.drone_extract)

- beat: drone wedged in debris. Extraction is an interaction while adds pressure.
- teach: hold vs solve roles; protect the drone from splash
- objective: extract drone; start escort state
- exits: west -> `R_SEW_WING_01` (escort back)

### R_SEW_SAFE_01 (safe pocket)

- beat: dry maintenance alcove with clear safe-lane markers.
- effects: panic meter explained; calm cooldown explained; no escalation while waiting.
- exits: west -> `R_SEW_WING_01`, south -> `R_SEW_FLOOD_01`, east -> `R_SEW_KEYCARD_01`

----

## Branch A: Flooded Section (default lane)

### R_SEW_FLOOD_01 (flooded approach)

- beat: standing water + sludge slow; chip damage matters for small/no-healer parties.
- teach: do not carry escort through hazards blindly
- exits: south -> `R_SEW_FLOOD_VALVES_01`, north -> `R_SEW_SAFE_01`

### R_SEW_FLOOD_VALVES_01 (setpiece.side.flooded_valve_sequence)

- beat: drain the section by flipping two valves in sequence under time pressure.
- teach: puzzle feedback must be explicit (correct/incorrect/complete)
- objective: drain water so the drone can cross safely
- exits: north -> `R_SEW_FLOOD_01` (back), west -> `R_SEW_RETURN_01`

----

## Branch B: Keycard Shortcut (explicit opt-in)

### R_SEW_KEYCARD_01 (maintenance console)

- beat: a console offers a one-use keycard at an explicit cost (money/rep/time).
- teach: shortcut lane is a deliberate choice
- exits: east -> `R_SEW_KEYCARD_DOOR_01`, west -> `R_SEW_SAFE_01`

### R_SEW_KEYCARD_DOOR_01 (setpiece.side.keycard_shortcut)

- beat: locked maintenance door; keycard opens it.
- objective: bypass flooded section
- exits: west -> `R_SEW_RETURN_01`

----

## Return

### R_SEW_RETURN_01 (return corridor)

- beat: last ambush; punish players who sprint escort through uncleared rooms.
- teach: escort discipline under pressure
- exits: west -> `R_SEW_JUNC_DRONE_02`

### R_SEW_JUNC_DRONE_02 (junction delivery)

- beat: deliver drone to a reboot cradle.
- rewards:
  - utility vendor unlock (base)
  - optional discount tier if drone is low damage and delivery is fast
- exits: west -> `R_SEW_JUNC_DRONE_01`

## NPCs

- Maintenance Dispatcher: blunt, time-conscious. Explains costs and the keycard lane.
- The Drone: communicates via panic behavior; calm interaction is class-flexible (bard/cleric/command).

## Rewards

- Utility vendor access (parts, antidotes, escort supplies)
- Optional: discount tier and/or future keycard lane knowledge

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/side-sewer-drone-rescue/`):

- Panic must be predictable: meter + thresholds + clear calm cooldown.
- Small parties and solos need an explicit shortcut lane; default lane is otherwise too punishing.
- Flooded valve sequence must have loud correct/incorrect feedback.
- There must be at least one safe pocket where waiting is safe (no escalation).
- Escort pathing must prefer marked safe lanes and avoid obvious hazard suicide routes.
- Escort-friendly-fire/splash rules must be explicit, with UI warnings when the drone is in blast radius.
- Add a "park the drone here" affordance in safe pockets so parties know when it's safe to fight.
- Keycard shortcut needs a predictable acquisition method and a clear opt-in cost before the escort starts.
- Carry mechanics need explicit rules (speed penalty, hazard interactions, panic effects) and clear feedback during valve windows.
- Traps must be readable in escort-critical rooms; hidden drone damage feels unacceptable.
- Under-leveled parties need a clear warning at accept; the bottom of the band should be survivable but not a surprise.
- Bonus criteria (speed, low drone damage) should be explicit and rewarded loudly.
