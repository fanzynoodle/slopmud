---
adventure_id: side-library-misfiled-wing
area_id: A01
zone: Sunken Library (Optional Wing)
clusters:
  - CL_LIB_STACKS
  - CL_LIB_FLOODED_WINGS
hubs: []
setpieces:
  - setpiece.side.misfiled_rule
  - setpiece.side.flooded_vent_bypass
  - setpiece.side.warded_archive_puzzle
level_band: [6, 10]
party_size: [1, 5]
expected_runtime_min: 65
---

# Side Quest: Library Misfiled Wing (Sunken Library) (L6-L10, Party 1-5)

## Hook

The Sunken Library has an optional wing nobody admits exists. It is "misfiled": doors are labeled wrong on purpose, and wrong turns accumulate mold and confusion. The reward is real: a rare plate fragment and a lore key that matters later.

This is a navigation puzzle with debuffs, not a kill corridor.

Inputs (design assumptions):

- Navigation must be learnable; the misfiled rule needs 1-2 explicit hints so it is logic, not guessing.
- Wrong-turn penalties should be clear and not purely punishing (avoid pure HP tax; vary pressure via locks/patrols/cadence).
- Mold load must be loud and have clear cleanse sources; the safe pocket should reduce/clear stacks.
- Secrets (vent bypass and shelf-smash) need clear reconnection so they feel like competence, not a maze.
- Small parties need checkpointing and scaled sentinel pressure.
- Optional bargain systems are fun only if explicit, capped, and have paydown.

## Quest Line

Success conditions:

- enter the misfiled wing
- solve the filing rule to reach the archive core
- secure the plate fragment + lore key
- exit without collapsing the wing (or choose to destroy it)

Optional outcomes:

- preserve vs destroy choice (consequences)
- discover one shortcut (vent bypass or shelf-smash route)

## Rules That Must Be Legible

### Mold Load (named stack)

Define one named stack (placeholder):

- `mold_load` rises on wrong turns, stagnant rooms, and certain hazards
- `mold_load` should be loud and cleansable (cleric item, cleanse station)

Wrong turns should cost *something*, but not feel like a pure HP tax:

- increase patrols, lock a door temporarily, or raise hazard cadence
- avoid "just more damage" as the only penalty

### Misfiled Rule (learnable)

The wing must have a simple rule that players can learn from 1-2 hints, for example:

- "the correct door is always the one with the wrong shelf label"
- "the stamped catalog glyph points opposite the printed label"

The point: navigation is logic, not guessing.

### Safe Pocket (required)

Add a safe pocket room:

- waiting is explicitly safe (no escalation)
- clears/reduces `mold_load`
- serves as checkpoint so wipes don’t require full re-clear

## Secrets (navigation rewards)

- **Vent bypass:** a flooded lock can be bypassed via a vent route (rogue/monk-friendly).
- **Shelf smash:** a jammed shelf wall can be smashed for a loud shortcut (barbarian/fighter-friendly).

## Optional Debt: Forbidden Index (`forbidden_index_debt`, capped)

If a party opts in (warlock beat):

- `forbidden_index_debt` tokens: start at 0; cap at 2
- boon gives immediate power or shortcut hint
- debt consequence increases mold/patrol pressure later
- paydown at the safe pocket (time or forfeit bonus)

Make it explicit and never a surprise.

----

## Room Flow

### R_LIB_SIDE_ENTRY_01 (entry)

- beat: a half-submerged sign reads "CATALOG MAINTENANCE." This is the misfiled wing door.
- teach: this is optional; reward is real; wrong turns have consequences
- exits: east -> `R_LIB_MIS_01`

### R_LIB_MIS_01 (setpiece.side.misfiled_rule)

- beat: first filing hint. One obvious wrong door; one subtle correct door.
- teach: misfiled rule and how to read hints
- exits: east -> `R_LIB_MIS_02`

### R_LIB_MIS_02 (mold pressure corridor)

- beat: tight corridors with mold load pressure if you linger or backtrack.
- teach: keep moving, but don’t guess
- exits:
  - east -> `R_LIB_SAFE_01`
  - north -> `R_LIB_VENT_01` (secret route start)

### R_LIB_SAFE_01 (safe pocket)

- beat: dry reading nook with a cleanse station and clear "safe to wait" language.
- effects: reduce `mold_load`; checkpoint.
- exits: east -> `R_LIB_MIS_03`, west -> `R_LIB_MIS_02`

### R_LIB_VENT_01 (secret: flooded vent bypass)

- beat: vent route bypasses a flooded lock.
- teach: movement/stealth path reward
- exits: east -> `R_LIB_MIS_03`

### R_LIB_MIS_03 (shelf wall)

- beat: jammed shelf wall blocks a direct line.
- options:
  - solve filing rule route (clean)
  - smash shelf wall (loud shortcut)
- exits: east -> `R_LIB_ARCHIVE_01`

### R_LIB_ARCHIVE_01 (setpiece.side.warded_archive_puzzle)

- beat: warded archive puzzle with sentinel spawns if solved wrong or too slow.
- teach: hold vs solve roles; avoid wrong-turn panic
- rewards: plate fragment + lore key
- choice: preserve vs destroy the archive (sets future consequences)
- exits: west -> `R_LIB_SAFE_01`

## NPCs

- None required; the wing can be environmental storytelling.
- Optional: a trapped archivist construct that can provide one hint for a cost.

## Rewards

- rare plate fragment (future unlock hook)
- lore key (unlocks a future hint, shortcut, or quest dialogue)

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/side-library-misfiled-wing/`):

- Navigation must be learnable; wrong-turn penalties should be clear but not purely punishing.
- Mold load must be loud and have clear cleanse sources.
- Safe pocket language must be visible from range and reduce/clear stacks; no-cleanse parties need a viable slow route.
- Vent bypass needs a real hint (draft sound, loose grate visuals, antechamber note); it should be discoverable, not luck.
- Shelf-smash loud shortcut needs deterministic consequences (what spawns, when, and how you recover).
- Traversal/puzzle tells must be consistent; avoid "guess -> sentinel spawn -> guess again" loops without recovery.
- Preserve/destroy prompt must be explicit about payoff and escape difficulty; add party_size guidance (solo/duo vs full party).
- Small parties need checkpointing and scaled sentinel pressure, especially on preserve escape.
- Forbidden index boon needs explicit UI and caps; escape wave pacing needs clear "wave start/end" cues.
- Optional bargain systems are fun only if explicit, capped, and have paydown.
