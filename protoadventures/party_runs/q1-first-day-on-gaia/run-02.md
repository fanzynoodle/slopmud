---
adventure_id: "q1-first-day-on-gaia"
run_id: "run-02"
party_size: 1
party_levels: [1]
party_classes: ["wizard"]
party_tags: ["brand-new", "robot-kit", "curious", "reads-everything"]
expected_duration_min: 30
---

# Party Run: q1-first-day-on-gaia / run-02

## Party

- size: 1
- levels: 1
- classes: wizard
- tags: brand-new, robot-kit, curious, reads-everything

## What They Did (Timeline)

1. hook / acceptance
   - Tried to treat the facility as a mystery and asked why NPCs can see them.
   - Wanted to inspect the signage and "ask R0 what the rules are".
2. travel / navigation decisions
   - Followed the linear route but stopped often to check if rooms changed after drills.
   - In the dorms, tried to find a hidden closet by interacting with everything (but did not type `search`).
3. key fights / obstacles
   - In prep bay, tried to `use` the heal item immediately to see what it does (good test).
   - Training drone was fine; they wanted a short explanation of what "retreat" means.
4. setpiece / spike
   - Hazard strip taught movement. They asked for a clearer telegraph of the hazard cycle.
5. secret / shortcut (or why they missed it)
   - Missed the dorm supply closet because they never used the explicit `search` verb.
6. resolution / rewards
   - Picked robot kit. Asked if robot kit changes dialogue in town (it should, lightly).

## Spotlight Moments (By Class)

- wizard: tried verbs experimentally (`use` early); learned hazard timing; asked for rules explanations (good for `help`/tutorial text).
- fighter: none
- rogue: none
- cleric: none
- ranger: none
- paladin: none
- bard: none
- druid: none
- barbarian: none
- warlock: none
- sorcerer: none
- monk: none

## Friction / Missing Content

- Needs a clear tutorial line for `search` if we expect players to find the supply closet.
- Hazard strip needs a readable telegraph cycle so it doesn't feel random.
- "Retreat" concept needs a simple definition and a suggested command (even if combat is basic).

## Extracted TODOs

- TODO: Add one `search` tutorial hint in the dorm cluster.
- TODO: Add consistent hazard telegraph messaging in `R_NS_LABS_05`.
- TODO: Add a 2-sentence retreat explanation in `R_NS_LABS_01`.

