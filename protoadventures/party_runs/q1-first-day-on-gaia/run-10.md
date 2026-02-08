---
adventure_id: "q1-first-day-on-gaia"
run_id: "run-10"
party_size: 1
party_levels: [1]
party_classes: ["barbarian"]
party_tags: ["brute-force", "tests-failure", "skips-text"]
expected_duration_min: 16
---

# Party Run: q1-first-day-on-gaia / run-10

## Party

- size: 1
- levels: 1
- classes: barbarian
- tags: brute-force, tests-failure, skips-text

## What They Did (Timeline)

1. hook / acceptance
   - Skipped text and tried to brute-force movement and commands.
2. travel / navigation decisions
   - In the doors drill, repeatedly slammed the sticky door. Wanted the failure loop to be short and readable.
3. key fights / obstacles
   - Training drone: tried to spam attacks and asked why sometimes nothing happened.
4. setpiece / spike
   - Hazard strip forced them to slow down and read, which is good.
5. secret / shortcut (or why they missed it)
   - Did not search for secrets.
6. resolution / rewards
   - Picked kit instantly and wanted to leave the tutorial right away.

## Spotlight Moments (By Class)

- barbarian: validated spammy input patterns; tested failure messaging in the sticky door drill; confirmed hazard strip forces reading.
- fighter: none
- rogue: none
- cleric: none
- wizard: none
- ranger: none
- paladin: none
- bard: none
- druid: none
- warlock: none
- sorcerer: none
- monk: none

## Friction / Missing Content

- Needs rate-limiting or debouncing for spammy input so output stays readable.
- Needs a clearer "why nothing happened" message on invalid commands.
- Needs a quick exit path for veterans who want to skip tutorial text (future).

## Extracted TODOs

- TODO: Add basic spam protection so tutorial output stays readable.
- TODO: Make invalid command feedback consistent and short ("huh? (try: help)").
- TODO: Add a future "skip tutorial" option for repeat characters.

