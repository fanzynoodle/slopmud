---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-09"
party_size: 5
party_levels: [8, 8, 8, 8, 8]
party_classes: ["fighter", "wizard", "sorcerer", "ranger", "rogue"]
party_tags: ["timer-confusion", "nearly-failed", "learned-fast", "rescue-skip"]
expected_duration_min: 30
---

# Party Run: event-rail-runaway-cargo-bot / run-09

## Party

- size: 5
- levels: 8/8/8/8/8
- classes: fighter, wizard, sorcerer, ranger, rogue
- tags: timer-confusion, nearly-failed, learned-fast, rescue-skip

## What They Did (Timeline)

1. hook / acceptance
   - All five were high level and assumed the event would be trivial.
2. travel / navigation decisions
   - They wasted time early because the distance-to-terminal timer wasn't obvious; they thought they had time to fight "one extra pack".
3. key fights / obstacles
   - Yard hazards punished the delay: the bot arrived earlier than expected and they had to sprint through hazards to catch up.
4. setpiece / spike
   - At the stop point, they stabilized by assigning roles mid-fight: fighter body-block, wizard lane control, ranger/rogue weak points, sorcerer saved burst for the ram window.
5. secret / shortcut (or why they missed it)
   - Missed the lever route entirely; the late scramble made exploration impossible.
6. resolution / rewards
   - Clean stop by raw execution, but the run highlighted a UX problem: a "timer event" must feel like a timer event immediately.

## Spotlight Moments (By Class)

- fighter: emergency body-block when the team realized they were late.
- wizard: lane control turned a panic scramble into a solvable fight.
- sorcerer: burst discipline during the ram window, not on cooldown.
- ranger/rogue: weak point focus while dodging hazards and staying on pace.

## Friction / Missing Content

- Distance/time remaining must be loud at all times; otherwise parties misjudge pacing.
- Lever route needs earlier signage so it can be a deliberate plan, not a post-wipe discovery.
- Hazard lanes should have consistent "safe vs danger" telegraphs so sprinting under pressure is fair.

## Extracted TODOs

- TODO: Add persistent timer UI + dispatcher callouts ("halfway", "last segment", "stop point ahead").
- TODO: Add early lever route signage and a one-line hint about its tradeoff.
- TODO: Add clearer lane telegraphs for yard hazards (floor paint, cones, audio).

