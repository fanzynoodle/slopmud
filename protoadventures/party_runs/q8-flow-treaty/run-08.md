---
adventure_id: q8-flow-treaty
run_id: "run-08"
party_size: 2
party_levels: [10, 10]
party_classes: [wizard, fighter]
party_tags: [small-party, bot-autofill, methodical]
expected_duration_min: 60
---

# Party Run: q8-flow-treaty / run-08

## Party

- size: 2 humans (wizard, fighter)
- levels: both 10
- classes: wizard, fighter
- tags: small-party, bot-autofill, methodical

## What They Did (Timeline)

1. Hook: they accept as a duo, but opt into bot autofill to reach 3 for node setpieces.
2. Shores: bot behavior is mostly fine in open areas.
3. Tunnels: bot pathing is the hazard. It stops in bad places and triggers ambushes. They learn bots must be "ambush aware".
4. Nodes: duo + bot clears objectives reliably. Fighter holds the interact; wizard controls adds; bot handles stragglers.
5. Boss: bot is helpful only if it understands target priority (add caller / phase adds).
6. Resolution: the duo likes the model; they demand bot command tools: "hold", "follow", "focus".

## Spotlight Moments (By Class)

- fighter: makes objectives possible even with small party size.
- wizard: turns wide pulls and boss phases into puzzles.
- bots: need explicit behavior around ambush bends and hazard telegraphs.

## Friction / Missing Content

- Bot AI needs tunnel/ambush discipline.
- Need simple bot commands and safe pocket rules.

## Extracted TODOs

- TODO: define bot behavior profiles for tunnel ambushes (scout, hold, regroup).
- TODO: add safe pocket markers bots can recognize.

