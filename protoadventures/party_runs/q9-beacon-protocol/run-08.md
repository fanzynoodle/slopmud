---
adventure_id: q9-beacon-protocol
run_id: "run-08"
party_size: 2
party_levels: [11, 11]
party_classes: [wizard, fighter]
party_tags: [small-party, bot-autofill, methodical]
expected_duration_min: 70
---

# Party Run: q9-beacon-protocol / run-08

## Party

- size: 2 humans (wizard, fighter)
- levels: both 11
- classes: wizard, fighter
- tags: small-party, bot-autofill, methodical

## What They Did (Timeline)

1. Hook: they accept as a duo and opt into bot autofill to reach 3 for beacon setpieces.
2. Tunnels: bot pathing is the biggest risk. It stops in ambush bends and triggers extra fights.
3. Beacons: duo + bot works if bot understands target priority (add caller / phase adds).
4. Switchyard: bot needs timing awareness or it gets deleted by patrol stacks.
5. Boss: bot is useful only if commanded (hold / focus / cross).
6. Resolution: the duo likes the trio model if bots are geometry- and timing-aware.

## Spotlight Moments (By Class)

- fighter: makes objectives possible with small party size.
- wizard: makes beacon defenses clean and controlled.
- bots: need timing + ambush discipline.

## Friction / Missing Content

- Bot AI needs patrol/timing awareness and "don’t stop in bends" behavior.
- Need basic bot commands and safe pocket rules.

## Extracted TODOs

- TODO: define bot behavior profiles for Underrail timing (scout, hold, cross).
- TODO: add safe pocket markers bots can recognize.

