---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-08"
party_size: 2
party_levels: [6, 6]
party_classes: ["cleric", "bard"]
party_tags: ["support-duo", "lever-or-fail", "bystander-rescue", "low-dps"]
expected_duration_min: 35
---

# Party Run: event-rail-runaway-cargo-bot / run-08

## Party

- size: 2
- levels: 6/6
- classes: cleric, bard
- tags: support-duo, lever-or-fail, bystander-rescue, low-dps

## What They Did (Timeline)

1. hook / acceptance
   - Took the event because they were already at the terminal and didn't want travel to lock down.
2. travel / navigation decisions
   - Committed to the lever route because their damage alone wouldn't stop the bot in time.
3. key fights / obstacles
   - Lever ambush ate time because they couldn't burst it; they had to kite and reset rather than clear instantly.
4. setpiece / spike
   - At the stop point, they chained "slow then dodge" cycles: pull lever slow, survive the ram tell, then chip weak points during the safe window.
5. secret / shortcut (or why they missed it)
   - Missed the ladder shortcut because they were stuck managing the ambush and couldn't explore.
6. resolution / rewards
   - Stopped the bot late with terminal damage, but succeeded. Rescued one bystander because bard insisted; it nearly cost the run.

## Spotlight Moments (By Class)

- bard: kept bystanders moving and prevented panic; made the rescue choice explicit.
- cleric: sustained through the no-burst playstyle and kept the duo alive through mistakes.
- bard/cleric: executed a repeatable "slow -> survive -> chip" plan that worked for low DPS.

## Friction / Missing Content

- Low-damage parties need explicit guidance that multiple slow interactions can substitute for burst.
- Lever ambush time cost should scale with party damage or provide a "skip ambush, pay cost" option.
- Terminal damage consequences need to be communicated immediately so late wins don't feel ambiguous.

## Extracted TODOs

- TODO: Add dispatcher hints for low-DPS parties (lever cycles and safe windows).
- TODO: Add an ambush scaling rule or alternate cost so lever route isn't a trap for support duos.
- TODO: Add clear terminal damage feedback and what it changes post-event.

