---
adventure_id: q10-anchor-the-wind
run_id: "run-08"
party_size: 2
party_levels: [13, 13]
party_classes: [wizard, fighter]
party_tags: [small-party, bot-autofill, methodical]
expected_duration_min: 80
---

# Party Run: q10-anchor-the-wind / run-08

## Party

- size: 2 humans (wizard, fighter)
- levels: both 13
- classes: wizard, fighter
- tags: small-party, bot-autofill, methodical

## What They Did (Timeline)

1. Hook: duo accepts, but opts into bot autofill to reach 3 for anchor setpieces.
2. Spans: bot pathing is the hazard. If bots don’t respect edges, they die to gusts. The duo ends up babysitting bot movement.
3. Anchors: duo + bot clears if bot can be commanded (hold / cross / focus).
4. Marshal: bot is useful only if it understands target priority and doesn’t chase adds to rails.
5. Resolution: duo loves the model if bots are "edge-aware" and gust-aware.

## Spotlight Moments (By Class)

- fighter: anchors objectives and keeps the run stable.
- wizard: solves add windows and traversal with utility.
- bots: must be edge-aware and gust-aware.

## Friction / Missing Content

- Bot AI needs strict "never path near rails during gust windows".
- Need simple bot commands and safe pocket rules.

## Extracted TODOs

- TODO: define bot behavior profiles for Skybridge geometry (edge avoidance, gust timing).
- TODO: add "safe square" marking system bots can read.

