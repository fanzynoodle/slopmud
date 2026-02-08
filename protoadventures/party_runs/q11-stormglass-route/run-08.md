---
adventure_id: q11-stormglass-route
run_id: "run-08"
party_size: 2
party_levels: [14, 14]
party_classes: [wizard, fighter]
party_tags: [small-party, bot-autofill, methodical]
expected_duration_min: 85
---

# Party Run: q11-stormglass-route / run-08

## Party

- size: 2 humans (wizard, fighter)
- levels: both 14
- classes: wizard, fighter
- tags: small-party, bot-autofill, methodical

## What They Did (Timeline)

1. Hook: duo takes it but opts into bot autofill to reach 3 for storm eye setpiece.
2. Dunes: bot behavior is mostly fine in open spaces.
3. Pulses: bot needs pulse awareness. If it stands in the open during a storm pulse, it becomes a liability.
4. Shelters: duo relies on shelter tubes to prevent attrition spirals. Wizard navigates; fighter anchors cover.
5. Storm eye: duo + bot succeeds if bot prioritizes adds and stays in cover.
6. Resolution: duo likes the model if bots are pulse-aware and shelter-aware.

## Spotlight Moments (By Class)

- fighter: anchor for cover fights; keeps group cohesive.
- wizard: navigation tool; makes storms learnable.
- bots: must be pulse-aware and obey cover rules.

## Friction / Missing Content

- Bot AI needs "seek shelter on pulse" behavior.
- Need basic bot commands ("hold in shelter", "follow", "focus").

## Extracted TODOs

- TODO: define bot behavior profiles for storm hazards (pulse awareness, shelter seeking).
- TODO: add "shelter marker" system bots can recognize.

