---
adventure_id: "side-library-misfiled-wing"
run_id: "run-05"
party_size: 1
party_levels: [10]
party_classes: ["wizard"]
party_tags: ["solo", "puzzle-first", "preserve", "high-risk-escape"]
expected_duration_min: 45
---

# Party Run: side-library-misfiled-wing / run-05

## Party

- size: 1
- levels: 10
- classes: wizard
- tags: solo, puzzle-first, preserve, high-risk-escape

## What They Did (Timeline)

1. hook / acceptance
   - Entered specifically for the lore key and wanted to preserve the archive if possible.
2. travel / navigation decisions
   - Used safe pockets like checkpoints: move -> stop -> clear -> move. Treated deep water as a hard fail.
3. key fights / obstacles
   - Mold stacks forced conservative routing. The wizard had to choose between spending resources to cleanse vs spending time to reset at pockets.
4. setpiece / spike
   - Archive puzzle was solvable solo, but sentinel spawns punished guesswork. The wizard solved by reading tells and controlling the room rather than brute forcing.
5. secret / shortcut (or why they missed it)
   - Found the vent route late and used it on the way out; it felt like the correct solo escape lane.
6. resolution / rewards
   - Preserved the archive and escaped with low HP. Solo is possible, but only if the escape difficulty scales to party_size=1.

## Spotlight Moments (By Class)

- wizard: solved the archive puzzle by reading consistent tells, not brute forcing.
- wizard: controlled sentinel spawns in tight corridors to create safe movement windows.
- wizard: used vent route as a solo escape lane and avoided a flooded lock on the way out.

## Friction / Missing Content

- Solo escape after preserve needs tuning; too many sentinels makes it feel like "correct choice, wrong for solo."
- Puzzle tells must be consistent enough for solo to deduce without sacrificing all resources to trial-and-error.
- Vent route should be hinted earlier; discovering it only after suffering doesn't feel fair.

## Extracted TODOs

- TODO: Add party_size scaling to preserve-escape sentinel count/pacing.
- TODO: Add consistent, learnable puzzle tells (color/shape/audio) to avoid guess-spawn loops.
- TODO: Add antechamber hinting for vent route so solo/small parties can plan around it.

