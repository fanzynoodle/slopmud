---
adventure_id: "q1-first-day-on-gaia"
run_id: "run-05"
party_size: 1
party_levels: [1]
party_classes: ["warlock"]
party_tags: ["declared-bot", "automation", "wants-determinism"]
expected_duration_min: 18
---

# Party Run: q1-first-day-on-gaia / run-05

## Party

- size: 1
- levels: 1
- classes: warlock
- tags: declared-bot, automation, wants-determinism

## What They Did (Timeline)

1. hook / acceptance
   - Tried to parse the tutorial as a deterministic state machine: prompt -> response -> next prompt.
   - Wanted every step to have a clear success acknowledgement line.
2. travel / navigation decisions
   - Attempted to use command shortcuts and expected strict parsing.
   - Re-ran rooms to confirm they could not break sequence.
3. key fights / obstacles
   - Training drone was fine, but they wanted machine-readable outcome cues ("drone defeated").
4. setpiece / spike
   - Hazard strip created a failure mode. They wanted failure to be explicit and recoverable without confusion.
5. secret / shortcut (or why they missed it)
   - Did not attempt `search` because it was not taught as a verb.
6. resolution / rewards
   - Picked robot kit because it sounded deterministic.
   - Wanted the job board teaser to be an explicit next-state pointer.

## Spotlight Moments (By Class)

- warlock: validated tutorial prompts as a state machine; tested failure recovery; pushed for machine-readable success cues.
- fighter: none
- rogue: none
- cleric: none
- wizard: none
- ranger: none
- paladin: none
- bard: none
- druid: none
- barbarian: none
- sorcerer: none
- monk: none

## Friction / Missing Content

- Tutorial needs explicit success lines for each drill ("drill complete") for automation and clarity.
- Failure messaging needs to be explicit ("you failed because X") and provide the next action.
- Optional verbs like `search` should be taught if we expect them to be used.

## Extracted TODOs

- TODO: Add explicit "drill complete" acknowledgements for the 3 verbs drills.
- TODO: Add explicit failure reason + next-step hints for hazard strip.
- TODO: Add explicit end-of-Q1 pointer to Q2 (job board).

