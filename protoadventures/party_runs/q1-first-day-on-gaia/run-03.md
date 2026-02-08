---
adventure_id: "q1-first-day-on-gaia"
run_id: "run-03"
party_size: 1
party_levels: [2]
party_classes: ["rogue"]
party_tags: ["mud-veteran", "speedrun", "command-abbrev", "tests-aliases"]
expected_duration_min: 12
---

# Party Run: q1-first-day-on-gaia / run-03

## Party

- size: 1
- levels: 2
- classes: rogue
- tags: mud-veteran, speedrun, command-abbrev, tests-aliases

## What They Did (Timeline)

1. hook / acceptance
   - Skipped text, tried to brute-force commands and movement.
2. travel / navigation decisions
   - Tried `n/e/s/w` and expected it to work. When it didn't, they used `go north` after being prompted.
   - In the dorm drill, intentionally failed once to see the failure messaging (good test).
3. key fights / obstacles
   - Training drone was trivial at level 2; they wanted a skip or a harder variant for over-leveled runs.
4. setpiece / spike
   - Hazard strip still taught something because it forced movement even when over-leveled.
5. secret / shortcut (or why they missed it)
   - Did not search for secrets. They would only do it if explicitly hinted.
6. resolution / rewards
   - Picked kit fast, wanted to immediately see the job board.

## Spotlight Moments (By Class)

- rogue: tested movement aliases; validated failure messaging; confirmed hazard strip still matters at higher level.
- fighter: none
- cleric: none
- wizard: none
- ranger: none
- paladin: none
- bard: none
- druid: none
- barbarian: none
- warlock: none
- sorcerer: none
- monk: none

## Friction / Missing Content

- Need lightweight direction aliases (`n/e/s/w`) or a clear prompt that teaches them fast.
- Over-leveled runs get bored in drone/pests; needs a "skip tutorial combat" option (admin or explicit).
- The path to Q2 should be immediately obvious when Q1 ends (job board teaser should be visible).

## Extracted TODOs

- TODO: Add `n/e/s/w` aliases in the newbie school rooms (or explicitly teach `go <dir>` as the only path).
- TODO: Add an optional "skip combat drills" toggle for veteran replays (future).
- TODO: Ensure job board teaser is impossible to miss at the end of Q1.

