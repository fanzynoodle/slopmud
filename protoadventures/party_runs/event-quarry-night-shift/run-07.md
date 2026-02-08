---
adventure_id: "event-quarry-night-shift"
run_id: "run-07"
party_size: 4
party_levels: [7, 7, 7, 7]
party_classes: ["wizard", "cleric", "warlock", "sorcerer"]
party_tags: ["all-caster", "visibility", "control", "no-brute-shortcut"]
expected_duration_min: 35
---

# Party Run: event-quarry-night-shift / run-07

## Party

- size: 4
- levels: 7/7/7/7
- classes: wizard, cleric, warlock, sorcerer
- tags: all-caster, visibility, control, no-brute-shortcut

## What They Did (Timeline)

1. hook / acceptance
   - Chose objectives: cache + elite. Declined the cursed ore option up front because they wanted a "clean" run.
2. travel / navigation decisions
   - Stayed on lantern lanes and used control to avoid fights rather than taking shadow routes.
3. key fights / obstacles
   - They accidentally pulled a patrol in the dark because telegraphs blended into lantern glare. Cleric burned resources just to recover from "I didn't see it" hits.
4. setpiece / spike
   - Elite fight was clean once they learned the flee tell: wizard locked lanes, sorcerer held burst for the escape window, warlock chose whether to spend a risky slow.
5. secret / shortcut (or why they missed it)
   - Missed the collapsed tunnel shortcut entirely; no brute opener and no obvious alternative connection.
6. resolution / rewards
   - Completed both objectives, but the run highlighted that lighting must help readability, not reduce it.

## Spotlight Moments (By Class)

- wizard: lane control to prevent patrol stacking in low visibility.
- cleric: debuff management and emergency recovery from unavoidable-feeling hits.
- sorcerer: burst discipline to finish the elite before the flee.
- warlock: opt-in risky slow choice (win fast vs pay a cost).

## Friction / Missing Content

- Lantern glare can hide telegraphs; need a consistent rule so "lit" always means "readable".
- Non-brute parties need an explicit alternate connection if the collapsed tunnel is the best chain route.
- Elite flee tell needs to be readable even when the arena is chaotic (audio callout, not just visuals).

## Extracted TODOs

- TODO: Define lighting contrast rules for telegraphs (never the same value range as lanterns).
- TODO: Add a non-brute connector route (catwalk/ladder) so all parties can chain objectives.
- TODO: Add an elite flee audio callout and UI ping so parties know when to commit burst.

