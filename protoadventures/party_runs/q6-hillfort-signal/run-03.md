---
adventure_id: "q6-hillfort-signal"
run_id: "run-03"
party_size: 5
party_levels: [7, 7, 7, 8, 8]
party_classes: ["paladin", "bard", "ranger", "druid", "sorcerer"]
party_tags: ["over-leveled", "social-build", "fast-clear"]
expected_duration_min: 35
---

# Party Run: q6-hillfort-signal / run-03

## Party

- size: 5
- levels: 7/7/7/8/8
- classes: paladin, bard, ranger, druid, sorcerer
- tags: over-leveled, social-build, fast-clear

## What They Did (Timeline)

1. hook / acceptance
   - Treated the bunker like a negotiation: asked for hazard pay, a map, and a guarantee of a safe rest niche.
   - The patrol leader's "short, practical briefings" played well: it kept momentum.
2. travel / navigation decisions
   - Optimized route: pylon 2 -> pylon 1 -> pylon 3, minimal backtracking.
   - Suggested the courtyard should visibly change as pylons light (so route choice feels like world-state, not checkboxes).
3. key fights / obstacles
   - Pylon fights were too easy at this level band; they wanted optional escalation:
     - "overcharge the pylon for a better reward" style knob.
4. setpiece / spike
   - Banner hall boss died fast; the interrupt mechanic never mattered.
   - They asked for a second pressure layer: a soft enrage, or a mid-fight terrain hazard that forces movement.
5. secret / shortcut (or why they missed it)
   - They found a "banner rope" idea: cut banners to open a high balcony path (shortcut) into the hall.
   - They wanted the shortcut to cost something (time, noise, or risk) so it isn't always optimal.
6. resolution / rewards
   - The rail terminal pass printing was satisfying, but they wanted a preview of rail content:
     - a locked timetable board, or
     - a one-room "platform event" teaser.

## Spotlight Moments (By Class)

- paladin: held the line while others negotiated; framed the safehouse as a duty (good ethos moment).
- bard: de-escalated an argument with a patrol NPC; earned a small non-combat perk (a hint about the boss).
- ranger: navigated hazard rooms cleanly; called route choices based on environmental signs.
- druid: used terrain awareness in the collapsed section; suggested a nonlethal option for a feral patrol unit.
- sorcerer: burst down add waves; proposed an "overcharge" variant to keep the run interesting.
- fighter: none (not in party)
- rogue: none (not in party)
- cleric: none (not in party)
- wizard: none (not in party)
- barbarian: none (not in party)
- warlock: none (not in party)
- monk: none (not in party)

## Friction / Missing Content

- Over-leveled parties need optional challenge toggles so the content remains fun and doesn't become a speedbump.
- The boss interrupt lesson can be missed entirely if damage is too high. Add a backup teaching beat (a forced-interrupt micro setpiece earlier, or a boss phase gate that requires interacting with the call mechanic at least once).
- Rail terminal should tease the next zone so "pass unlocked" feels immediately meaningful.

## Extracted TODOs

- TODO: Add an optional "pylon overcharge" mechanic with clear risk/reward.
- TODO: Add a small forced-interrupt tutorial beat before the banner hall.
- TODO: Add a one-room rail terminal teaser event behind `gate.rail_spur.pass`.
