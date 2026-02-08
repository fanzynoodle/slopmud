---
adventure_id: q7-conveyor-war
run_id: "run-01"
party_size: 5
party_levels: [8, 8, 8, 8, 8]
party_classes: [fighter, rogue, cleric, wizard, monk]
party_tags: [balanced, first-time, cautious]
expected_duration_min: 45
---

# Party Run: q7-conveyor-war / run-01

## Party

- size: 5
- levels: all 8
- classes: fighter, rogue, cleric, wizard, monk
- tags: balanced, first-time, cautious

## What They Did (Timeline)

1. Hook: they take the sabotage job because "the factory is eating the town's budget".
2. Prep: they buy basic consumables and agree on a simple rule: "never fight on a moving belt if we can avoid it."
3. Entry: first tight corridor is a wake-up call. Turrets create a hard LOS puzzle; wizard and rogue start calling corners.
4. Line shutdown 1: they find a switch lever. Touching it triggers a wave. Fighter anchors the choke; cleric stabilizes; rogue disables a secondary lock under pressure.
5. Traversal: conveyors become the real enemy. Monk scouts timing windows; party crosses in two pulses.
6. Line shutdown 2: they try to brute it and almost wipe when a belt-runner pulls an extra pack. They learn to pull back into a safe "dead belt" alcove.
7. Line shutdown 3: they finally find a "quiet route" (side maintenance catwalk). Rogue feels useful. They disable the third line without triggering a full wave.
8. Boss: Foreman Clank arena: boss + add calls. They wipe once to an add snowball. Second attempt: wizard pre-controls adds; rogue prioritizes the add caller; fighter holds center; cleric calls cooldown timing.
9. Resolution: they return to the switchroom with a stamped "shutdown proof". Everyone feels like the factory is a place with rules.

## Spotlight Moments (By Class)

- fighter: owns the choke points; makes the party feel safe moving between fights.
- rogue: "quiet route" discovery and disabling secondary locks while the wave is up.
- cleric: keeps the run stable; the difference between a wipe and a reset.
- wizard: turns turret rooms from unfair to solvable; makes add phase controllable.
- ranger: no ranger in party; would shine in long corridors picking off belt-runners.
- paladin: no paladin in party; would shine in "protect workers vs sabotage" moral beat.
- bard: no bard in party; would shine in switchroom coordination and morale after wipe.
- druid: no druid in party; would shine jamming gears / interacting with hazards in a non-combat way.
- barbarian: no barbarian in party; would shine ripping a jammed lever free mid-fight.
- warlock: no warlock in party; would shine in "overclock shutdown" optional bargain.
- sorcerer: no sorcerer in party; would shine in burst-clearing add waves.
- monk: becomes the traversal lead; timing belts and dodging knockbacks.

## Friction / Missing Content

- Need clear telegraphs for conveyor hazards (knockback, belt speed, safe squares).
- Need a consistent "sabotage verb" UX: `disable line`, `pull lever`, `lockout key`, etc.
- Arena wipe felt fair, but the add call needs a loud warning line.
- The map needs "return routes" so backtracking doesn't feel like punishment.

## Extracted TODOs

- TODO: define a standard setpiece interface for "line shutdown" (trigger -> waves -> completion -> persistent state).
- TODO: add signage grammar for factories (color bands, floor arrows, warning sirens).
- TODO: decide if turrets are enemies, hazards, or both (loot vs just pressure).

