---
adventure_id: "q6-hillfort-signal"
run_id: "run-02"
party_size: 3
party_levels: [4, 5, 5]
party_classes: ["fighter", "barbarian", "rogue"]
party_tags: ["under-leveled", "all-melee", "no-healer", "bot-fill-optional"]
expected_duration_min: 60
---

# Party Run: q6-hillfort-signal / run-02

## Party

- size: 3
- levels: 4/5/5
- classes: fighter, barbarian, rogue
- tags: under-leveled, all-melee, no-healer, bot-fill-optional

## What They Did (Timeline)

1. hook / acceptance
   - Took the job because "rail pass sounds like progression".
   - Asked immediately where they could buy bandages and whether the bunker has a safe bed.
2. travel / navigation decisions
   - Picked pylon 3 first (collapsed section) to "get the scary one out of the way".
   - Backtracked more than expected; movement cost felt fine, but the run felt long because failures reset progress.
3. key fights / obstacles
   - Pylon 3: hazard zones forced movement, which was good, but without ranged options they ate extra hits.
   - Pylon 1: defend-timer exposed the weakness of no healer; they wanted a "pull lever to pause the sequence" fail-safe.
4. setpiece / spike
   - Banner hall boss: too punishing when adds stacked; they couldn't reliably stop the call and got attritioned down.
   - They tried a "kite down the hall" plan and asked if retreat should reset the boss.
5. secret / shortcut (or why they missed it)
   - They found a broken arrow-slit that looked like a shortcut, but it didn't go anywhere (felt like a tease).
   - They wanted a minor payoff: a cache of consumables or a one-way drop back to the courtyard.
6. resolution / rewards
   - They failed the boss once, then opted to "pretend we brought an assist bot".
   - With a medic-style bot (or just extra sustain), the same content felt fair.

## Spotlight Moments (By Class)

- fighter: took leadership on "rotate cooldowns" and called retreats; held the narrow approach on pylon 2.
- rogue: found safe footing patterns in pylon 3 hazards; used opportunistic bursts to thin adds before the call.
- barbarian: deleted a dangerous add wave on pylon 1; traded health for tempo.
- cleric: none (not in party; this absence was the point)
- wizard: none (not in party)
- ranger: none (not in party)
- paladin: none (not in party)
- bard: none (not in party)
- druid: none (not in party)
- warlock: none (not in party)
- sorcerer: none (not in party)
- monk: none (not in party)

## Friction / Missing Content

- Under-leveled + no-healer needs a "reasonable retreat loop". Either bunker rest is accessible early, or pylon progress persists even if they wipe on the boss.
- The banner hall boss needs a clearer recovery story. Either a checkpoint after 3 pylons exists, or add pressure can be reduced via an optional objective.
- Consumables matter here. If we expect parties without healers, we need a way to stock up before committing.

## Extracted TODOs

- TODO: Make `pylons_lit` persist and unlock the banner hall even after a wipe.
- TODO: Add an optional "silence the banners" objective that reduces add spawns in the boss room.
- TODO: Add a small consumable cache in the bunker for parties that match "under-leveled" or "no-healer" tags.
