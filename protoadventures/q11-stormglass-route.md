---
adventure_id: q11-stormglass-route
area_id: A01
zone: Glass Wastes
clusters:
  - CL_WASTES_OUTPOST
  - CL_WASTES_DUNES
  - CL_WASTES_GLASSFIELDS
  - CL_WASTES_STORM_EYE
hubs:
  - HUB_WASTES_OUTPOST
setpieces:
  - setpiece.q11.storm_eye_event
level_band: [12, 15]
party_size: [3, 5]
expected_runtime_min: 70
---

# Q11: Stormglass Route (Glass Wastes) (L12-L15, Party 3-5)

## Hook

The Glass Wastes are not a dungeon you "clear". They're a weather problem you route through: dunes you can navigate, glassfields you can survive if you respect them, and shelters that keep you from being ground down by attrition.

The outpost needs a usable stormglass route. Reach the storm eye, stabilize it long enough to get a clean window, and return with proof.

Inputs (design assumptions):

- Attrition rules must be explicit and readable (what stacks, how to clear it, what counts as a safe rest).
- Shelters must be discoverable via consistent hints, and at least one shelter should be guaranteed so runs do not hard-fail to RNG.
- The storm eye setpiece must have clear safe-zone anchors; pure visibility loss feels unfair without them.
- Navigation choices must be legible (landmarks, repeatable signs) so getting lost is a player mistake, not a text parser issue.
- Optional rescue/escort beats need concrete reward hooks so parties choose them intentionally.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q11_stormglass_route.state`: `entry` -> `eye` -> `complete` (key name placeholder; align later)
- `q.q11_stormglass_route.exposure`: counter/bool (placeholder)
- `gate.crater_gardens.entry`: `0/1`

Success conditions:

- reach the storm eye
- complete the storm eye setpiece
- report back at the outpost and unlock the next gate

## Room Flow

Glass Wastes should teach endurance and route literacy:

- Attrition is explicit: it has a name, stacks, and clear recovery rules.
- Shelters are the real checkpoints: you hop between them to manage attrition and pulses.
- Speed routes exist, but they are explicit opt-ins (risk vs time), not accidental suicides.

### R_WASTES_OUTPOST_01 (CL_WASTES_OUTPOST, HUB_WASTES_OUTPOST)

- beat: outpost hub. Contract: reach the storm eye and stabilize it long enough to open passage.
- teach: treat this as a route, not a room-clearing dungeon; fight in cover pockets
- quest: set `q.q11_stormglass_route.state=entry` (placeholder)
- exits:
  - north -> `R_WASTES_DUNE_01` (main route)
  - east -> `R_WASTES_GLASS_01` (risky shortcut)

### Attrition Rule (must be legible)

Define one named stack (placeholder):

- `stormglass_fatigue` stacks when parties sprint through exposed dunes or glassfields.
- shelters reduce or clear stacks.

Players should know:

- what causes stacks
- what stacks do
- how to remove them

### Safe Pocket (must exist)

Guarantee at least one clearly-marked safe pocket shelter on the main route. This is where parties recover and where wipes should converge back to (to avoid full re-clear pain).

### Secret: Shelter Tube (navigation reward)

One consistent secret route bypasses a nasty open segment:

- a half-buried shelter tube that reconnects dunes -> glassfields safely.

This is the wastes secret: survival skill, not loot.

----

## Dunes (long sightlines, ranged pressure)

### R_WASTES_DUNE_01 (CL_WASTES_DUNES)

- beat: dunes with clear landmark sightlines (outpost visible in the distance).
- teach: route-finding; pace yourself; pull into cover
- exits: north -> `R_WASTES_DUNE_02`

### R_WASTES_DUNE_02 (CL_WASTES_DUNES)

- beat: dunes leading into a glassfield edge. First explicit storm pulse/lull rhythm should be introduced here.
- teach: pulse/lull rhythm; choose risk: safe dunes vs faster glassfield line
- exits:
  - north -> `R_WASTES_SHELTER_01` (safe shelter)
  - east -> `R_WASTES_GLASS_01` (risky)

### R_WASTES_SHELTER_01 (shelter checkpoint)

- beat: guaranteed shelter with consistent hint language (glyph/cairn/marker).
- teach: shelters as checkpoints; clear attrition stacks here; "wait out pulses"
- exits: north -> `R_WASTES_GLASS_01`

----

## Glassfields (navigation hazard)

### R_WASTES_GLASS_01 (CL_WASTES_GLASSFIELDS)

- beat: glass shards and glare. Standing in shard zones too long applies a bleed-like status.
- teach: navigation; don’t wander in pulses; fight in cover pockets
- exits: east -> `R_WASTES_GLASS_02`

### R_WASTES_GLASS_02 (CL_WASTES_GLASSFIELDS, safe pocket)

- beat: clearly-marked safe pocket shelter near the storm eye approach.
- teach: regroup point to avoid full re-clear pain
- exits: east -> `R_WASTES_EYE_01`

----

## Storm Eye (setpiece.q11.storm_eye_event)

Storm eye should have readable anchors:

- rotating safe zones (visual and audio)
- a "safe line" marker language (flares/pylons)
- pulse cadence that parties can learn

### R_WASTES_EYE_01 (CL_WASTES_STORM_EYE)

- beat: approach ring. Visibility drops. Explicit pulse/lull language.
- teach: follow anchors; don’t panic-run
- quest: set `q.q11_stormglass_route.state=eye` (placeholder)
- exits: east -> `R_WASTES_EYE_02`

### R_WASTES_EYE_02 (setpiece.q11.storm_eye_event)

- beat: stabilize a marker/beacon while waves arrive in fixed cadence. Phase shifts change hazard intensity.
- teach: cadence, shelter chaining, and target priority under low visibility
- quest: set `q.q11_stormglass_route.state=complete` (placeholder)
- exits: west -> `R_WASTES_OUTPOST_01`

## Optional System: Storm Boon Bargains (debt, capped)

Some parties want to "fight the storm". Give them an explicit opt-in bargain that trades survivability for tempo.

- `storm_debt` tokens: start at 0; cap at 2.
- taking a boon: +tempo (skip one exposed segment, or get one guaranteed lull window), then `storm_debt += 1`.
- debt consequences (loudly messaged): each debt increases pulse severity in the next exposed segment or setpiece phase.
- paydown options (at shelters only): spend time (wait out an extra pulse) or sacrifice a contract bonus to reduce `storm_debt` by 1.

This should feel like a deliberate choice, not a hidden trap.

## NPCs

- Outpost Contract Lead: blunt and practical; frames the run as endurance and route marking.
- Travelers (optional): rescue/escort beats should have concrete reward hooks.

## Rewards

- `gate.crater_gardens.entry=1` (Crater Gardens travel unlocked)
- Optional: rescue/escort beats should grant rep or pricing discounts.

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/q11-stormglass-route/`):

- Attrition rules must be explicit and readable.
- Shelters should be discoverable via consistent hints, and at least one should be guaranteed.
- Storm eye needs clear safe-zone anchors; pure visibility loss feels unfair without them.
