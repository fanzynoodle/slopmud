---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-07"
party_size: 5
party_levels: [7, 7, 6, 6, 6]
party_classes: ["barbarian", "fighter", "monk", "paladin", "bard"]
party_tags: ["all-melee", "grapple-chain", "lever-route", "chaotic"]
expected_duration_min: 28
---

# Party Run: event-rail-runaway-cargo-bot / run-07

## Party

- size: 5
- levels: 7/7/6/6/6
- classes: barbarian, fighter, monk, paladin, bard
- tags: all-melee, grapple-chain, lever-route, chaotic

## What They Did (Timeline)

1. hook / acceptance
   - Decided up front: monk hits lever, barbarian/fighter grapple at the stop, paladin/bard keep everyone alive.
2. travel / navigation decisions
   - Took the lever route early and accepted the ambush, because melee needed the extra slowdown.
3. key fights / obstacles
   - Yard hazards were a tax: melee had to respect lanes or they'd get knocked out of grapple range later.
4. setpiece / spike
   - Stop point became a wrestling match: barbarian grappled first, fighter joined when the ram tell started, paladin body-screened, bard called cadence so nobody got clipped by a crane swing.
5. secret / shortcut (or why they missed it)
   - Missed the ladder shortcut; they were too focused on staying together for the grapple plan.
6. resolution / rewards
   - Stopped the bot cleanly but at low HP. It worked, but only because the lever and grapple rules were understandable enough to execute under pressure.

## Spotlight Moments (By Class)

- monk: lever route execution under pressure.
- barbarian: primary grapple/body-block hero moment.
- fighter: secondary grapple and lane control when the bot tried to drift.
- paladin: protected the grapple stack and prevented panic deaths.
- bard: coordination and timing calls in a geometry-heavy arena.

## Friction / Missing Content

- Lever ambush needs a consistent spawn pattern so it doesn't feel like random time loss.
- Grapple/body-block rules must be explicit (range, duration, how to break) or melee plans fall apart.
- Hazard lanes need clearer "safe" telegraphs so melee can stay in position for the stop.

## Extracted TODOs

- TODO: Make lever ambush spawns deterministic and signposted (entry arrows, audio cue).
- TODO: Add clear grapple/body-block UI (attach indicator, break warning, success feedback).
- TODO: Add stronger lane markings for crane swings and ram pathing in the yard/stop.

