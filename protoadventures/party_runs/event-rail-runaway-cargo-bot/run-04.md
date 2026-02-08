---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-04"
party_size: 4
party_levels: [6, 6, 6, 6]
party_classes: ["ranger", "sorcerer", "rogue", "monk"]
party_tags: ["weak-points", "rescue-bystanders", "ladder-shortcut", "fast"]
expected_duration_min: 25
---

# Party Run: event-rail-runaway-cargo-bot / run-04

## Party

- size: 4
- levels: 6/6/6/6
- classes: ranger, sorcerer, rogue, monk
- tags: weak-points, rescue-bystanders, ladder-shortcut, fast

## What They Did (Timeline)

1. hook / acceptance
   - Dispatcher callout hit; they immediately agreed to rescue bystanders even if it cost time.
2. travel / navigation decisions
   - Monk sprinted for the lever route while rogue handled bystander pulls; ranger/sorcerer stayed on the main line to keep damage/slow pressure on the bot.
3. key fights / obstacles
   - Yard hazards were a bigger threat than enemies: crane swings forced them to choose "safe lane" over greed hits.
   - Rogue lost time because bystanders were not clearly marked as "optional"; they second-guessed whether rescue was required.
4. setpiece / spike
   - Lever pull slowed the bot, but spawned an ambush. Sorcerer cleared the ambush fast while ranger stayed on weak points so the bot didn't regain speed.
5. secret / shortcut (or why they missed it)
   - Found a ladder chain that bypassed one hazard segment and let the monk rejoin the group without eating a crane swing.
6. resolution / rewards
   - Clean stop at the final point: ranger finished weak points during the ram tell while monk/rogue stayed alive dodging geometry. Rescued bystanders and saved the terminal.

## Spotlight Moments (By Class)

- monk: reached the lever under timer pressure and still made it back for the stop.
- rogue: bystander rescue under time pressure without getting clipped by hazards.
- ranger: weak point focus while moving; solved the bot as a moving target, not a DPS dummy.
- sorcerer: burst-cleared lever ambush so the lever choice paid off.

## Friction / Missing Content

- Rescue objective needs explicit UI: who is rescuable, how many, and what it costs.
- Ladder/shortcut routes should be learnable via signage, not discovered by luck.
- Yard hazards need consistent lane markers so "safe" isn't trial-and-error.

## Extracted TODOs

- TODO: Add a rescue objective tracker (count + distance + time cost callout).
- TODO: Add consistent shortcut indicators (paint, arrows, ladder icons) for speed lanes.
- TODO: Add reliable lane telegraphs for yard hazards (floor markings + audio cues).

