---
adventure_id: "q2-job-board-never-sleeps"
run_id: "run-08"
party_size: 2
party_levels: [2, 2]
party_classes: ["warlock", "bot:medic"]
party_tags: ["declared-bot", "automation", "wants-determinism"]
expected_duration_min: 24
---

# Party Run: q2-job-board-never-sleeps / run-08

## Party

- size: 2 (1 human + 1 assist bot)
- levels: 2/2
- classes: warlock, bot:medic
- tags: declared-bot, automation, wants-determinism

## What They Did (Timeline)

1. hook / acceptance
   - Tried to accept contracts programmatically and wanted stable tokens for each contract.
2. travel / navigation decisions
   - Treated each contract as a finite state machine and expected explicit counters.
3. key fights / obstacles
   - Combat was fine. The only failure mode was unclear completion feedback.
4. setpiece / spike
   - Delivery contract was hardest for automation because it is "walk and interact".
5. secret / shortcut (or why they missed it)
   - Ignored secrets. Automation wants deterministic, surfaced content.
6. resolution / rewards
   - Picked civic and wanted explicit gate unlock logs (sewers/quarry entry flags).

## Spotlight Moments (By Class)

- warlock: validated state-machine style questing needs; pushed for stable contract tokens and counters.
- bot:medic: provided predictable sustain without solving objectives.
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

- Need stable contract identifiers and explicit counters (`contracts_done=2/3`).
- Need explicit gate unlock messages after choice (what changed).
- Bots must not auto-solve contract objectives; they should sustain only.

## Extracted TODOs

- TODO: Add stable contract IDs to hub UI text and internal state.
- TODO: Add explicit gate unlock logging after faction choice.
- TODO: Ensure assist bots do not interact with objectives by default.

