---
adventure_id: q3-sewer-valves
run_id: "run-03"
claimed_by: "codex"
party_size: 5
party_levels: [4, 4, 4, 4, 4]
party_classes: [wizard, warlock, sorcerer, rogue, ranger]
party_tags: [no-healer, all-ranged, control-heavy, clean]
expected_duration_min: 42
---

# Party Run: q3-sewer-valves / run-03

## Party

- size: 5
- levels: all 4
- classes: wizard, warlock, sorcerer, rogue, ranger
- tags: no-healer, all-ranged, control-heavy, clean

## What They Did (Timeline)

1. Plan: avoid all chip damage. They treat sludge as "do not stand" and plan around LoS.
2. Junction: they ignore the drone rescue to keep tempo.
3. Valve 3: easiest for them; they delete ranged mobs and complete the objective safely.
4. Valve 1: rogue turns valve while casters control adds.
5. Valve 2: sludge is the only threat; they move constantly.
6. Grease King: geometry is still real; they win by spreading and controlling adds, bursting the king only during safe windows.
7. Reward: lever pull; they want a short confirmation line that it’s permanent.

## Spotlight Moments (By Class)

- wizard: control makes valve rooms deterministic.
- warlock: burst on elite drone and adds; would fit optional bargain systems well.
- sorcerer: deletes add waves quickly so no-healer stays safe.
- rogue: objective interaction and scouting.
- ranger: long sightline threat removal; keeps ranged rooms honest.

## Friction / Missing Content

- Need a clear rule for safe waiting in junction so no-healer parties can reset.
- Boss arena must have enough safe squares so spreading is possible.

## Extracted TODOs

- TODO: define a "safe wait" pocket at `R_SEW_JUNC_01` (no respawns, no pressure).
- TODO: ensure boss arena has readable safe squares (avoid full-floor slick).

