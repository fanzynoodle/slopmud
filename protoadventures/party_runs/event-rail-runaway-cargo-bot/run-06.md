---
adventure_id: "event-rail-runaway-cargo-bot"
run_id: "run-06"
party_size: 3
party_levels: [7, 7, 7]
party_classes: ["wizard", "warlock", "ranger"]
party_tags: ["all-ranged", "control", "no-grapple", "clean-stop"]
expected_duration_min: 22
---

# Party Run: event-rail-runaway-cargo-bot / run-06

## Party

- size: 3
- levels: 7/7/7
- classes: wizard, warlock, ranger
- tags: all-ranged, control, no-grapple, clean-stop

## What They Did (Timeline)

1. hook / acceptance
   - Treated it like a ranged control test: keep distance, deny lanes, hit weak points.
2. travel / navigation decisions
   - Stayed on the main chase line and skipped the lever route to avoid the ambush tradeoff.
3. key fights / obstacles
   - Hazard lanes forced them to stop shooting and move; the bot punished stationary play immediately.
   - Weak points were hard to see when the bot passed through shadow; ranger called them out by pattern rather than visuals.
4. setpiece / spike
   - At the stop point, wizard boxed in the lane so the bot couldn't drift; warlock committed an opt-in heavy slow that made the final burst window generous.
5. secret / shortcut (or why they missed it)
   - Found a "safe pocket" behind rail cover where you can shoot weak points without getting clipped by the ram lane; felt like an intended ranged perch.
6. resolution / rewards
   - Clean stop with no terminal damage. The win condition felt good: solve lanes, then burst.

## Spotlight Moments (By Class)

- ranger: weak point pattern calling while moving and dodging hazards.
- wizard: lane control at the stop point so the bot couldn't juke.
- warlock: opted into a risky heavy slow at the exact moment it mattered.

## Friction / Missing Content

- Weak points need a stronger highlight when the bot crosses shadowed segments.
- Ranged parties need explicit confirmation that skipping the lever is viable (otherwise lever feels mandatory).
- The stop-point lane needs clear geometry reads so players understand where the ram will go.

## Extracted TODOs

- TODO: Add weak point highlights that survive lighting changes (outline/shimmer).
- TODO: Add dispatcher hint text for multiple viable strategies (lever vs weak points vs grapple).
- TODO: Add a clear ram-lane indicator on the ground at the stop point.

