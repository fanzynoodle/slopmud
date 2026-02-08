---
adventure_id: "event-quarry-night-shift"
run_id: "run-08"
party_size: 3
party_levels: [6, 6, 6]
party_classes: ["bard", "ranger", "paladin"]
party_tags: ["rescue-chain", "patrol-avoidance", "no-arcane", "protect-npcs"]
expected_duration_min: 45
---

# Party Run: event-quarry-night-shift / run-08

## Party

- size: 3
- levels: 6/6/6
- classes: bard, ranger, paladin
- tags: rescue-chain, patrol-avoidance, no-arcane, protect-npcs

## What They Did (Timeline)

1. hook / acceptance
   - Picked objectives: rescue + cache. Explicit goal was "get workers out alive", not maximize ore.
2. travel / navigation decisions
   - Ranger read patrol routes by footprints and lantern cadence, and kept them on the safe lane edges.
   - Bard used call-and-response with workers to keep them together when visibility dropped.
3. key fights / obstacles
   - Without arcane control, they avoided most fights. When forced, paladin held a narrow lane while the bard kept workers from panicking.
4. setpiece / spike
   - Rescue hazard lane: an audio-only telegraph was the difference between "fair" and "wipe" because they couldn't see the wind-up.
5. secret / shortcut (or why they missed it)
   - They saw the collapsed tunnel but refused it; too loud and too risky for escorts.
6. resolution / rewards
   - Completed rescue and secured one cache with zero patrol escalation. Payout was smaller, but the run felt clean and teachable.

## Spotlight Moments (By Class)

- ranger: patrol literacy and safe-lane navigation under a timer.
- bard: kept workers calm and moving; prevented escort chaos from becoming combat chaos.
- paladin: protect line in a low-visibility choke when avoidance failed.

## Friction / Missing Content

- Worker escort behavior needs a clear "stay in the light lane" instruction so it isn't babysitting AI.
- Low-visibility telegraphs need audio cues as first-class signals, not optional polish.
- Avoidance play needs a clear reward callout so it doesn't feel like "we skipped content".

## Extracted TODOs

- TODO: Add an escort formation command (tight / follow leader / hold) to reduce AI babysitting.
- TODO: Add strong audio telegraphs for the hazard lane and patrol approach.
- TODO: Add an explicit "stealth/avoidance bonus" payout note so safe play feels valued.

