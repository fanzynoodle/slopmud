---
adventure_id: "q1-first-day-on-gaia"
run_id: "run-07"
party_size: 1
party_levels: [1]
party_classes: ["ranger"]
party_tags: ["explorer", "tests-boundaries", "map-brain"]
expected_duration_min: 22
---

# Party Run: q1-first-day-on-gaia / run-07

## Party

- size: 1
- levels: 1
- classes: ranger
- tags: explorer, tests-boundaries, map-brain

## What They Did (Timeline)

1. hook / acceptance
   - Tried to look for a map and asked where the nearest exit is.
2. travel / navigation decisions
   - Tested whether they could go off-path. When they couldn't, they accepted the tutorial rails.
   - In dorm doors drill, deliberately tried wrong doors to see if the world punishes exploration (it should not).
3. key fights / obstacles
   - Drone and pests were fine. They wanted a "how to retreat" reminder in combat rooms.
4. setpiece / spike
   - Hazard strip: understood quickly and wanted it to be reusable as a practice room.
5. secret / shortcut (or why they missed it)
   - Looked for secrets by interacting with objects, but did not know `search`.
6. resolution / rewards
   - Wanted the corridor-to-town window to show obvious landmarks (job board square).

## Spotlight Moments (By Class)

- ranger: validated navigation signaling; tested boundary behavior; pushed for landmarks and re-usable practice rooms.
- fighter: none
- rogue: none
- cleric: none
- wizard: none
- paladin: none
- bard: none
- druid: none
- barbarian: none
- warlock: none
- sorcerer: none
- monk: none

## Friction / Missing Content

- Needs a simple "this is linear on purpose" line so explorers don't feel blocked.
- Needs `search` taught explicitly if we want secret content found organically.
- Needs landmarking so the first town transition feels like arrival, not a teleport.

## Extracted TODOs

- TODO: Add one short "tutorial rails" line in `R_NS_ORIENT_01` or `R_NS_ORIENT_02`.
- TODO: Add an explicit `search` hint in dorms if the supply closet is meant to be found.
- TODO: Add visible landmark text for the job board in `R_NS_EXIT_01`.

