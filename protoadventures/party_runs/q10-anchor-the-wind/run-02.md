---
adventure_id: "q10-anchor-the-wind"
run_id: "run-02"
party_size: 3
party_levels: [11, 11, 11]
party_classes: ["paladin", "rogue", "monk"]
party_tags: ["under-leveled", "no-caster", "fast-then-punished", "edge-deaths"]
expected_duration_min: 70
---

# Party Run: q10-anchor-the-wind / run-02

## Party

- size: 3
- levels: 11/11/11
- classes: paladin, rogue, monk
- tags: under-leveled, no-caster, fast-then-punished, edge-deaths

## What They Did (Timeline)

1. hook / acceptance
   - Took the anchor job and assumed it was "three rooms, three levers."
2. travel / navigation decisions
   - Tried to sprint the main spans. Got punished: one knockback punted the rogue off the edge (death/respawn).
   - Swapped to side scaffolds where possible to avoid direct wind lanes.
3. key fights / obstacles
   - Without ranged control, span shooters were a problem. Monk had to dive them while paladin held the line.
   - Anchor 1 interact time felt too long for a trio; waves stacked up.
4. setpiece / spike
   - Anchor 2 rescue beat: they skipped it for speed.
   - That choice backfired: without the shortcut, the return path forced them through an extra dangerous span segment.
5. secret / shortcut (or why they missed it)
   - Rogue found a scaffold bypass but it was unclear where it reconnected; they hesitated and lost time.
6. resolution / rewards
   - Reached Wind Marshal low on resources.
   - First attempt: wipe due to chained knockbacks with no safe squares to anchor on.
   - Second attempt: they hugged an interior pillar and timed movement with wind audio; won, but it felt like learning by dying.

## Spotlight Moments (By Class)

- paladin: stood on the bridgehead and kept the party together when the first edge death happened.
- rogue: found alternate scaffold routes and managed interact windows by picking off shooters.
- monk: movement mastery; dove priority targets and dodged knockbacks consistently.

## Friction / Missing Content

- Trio scaling: anchor setpieces need knobs (interact time, wave size, shooter count) for party size 3.
- Edge deaths need to feel fair: clear wind lanes, reliable audio tell, and at least one "brace" mechanic or safe square.
- Skipping rescue should be a valid speed choice, but it should be clear what you lose (shortcut) up front.

## Extracted TODOs

- TODO: Add party-size scaling to anchor setpieces.
- TODO: Add a readable "brace" or "anchor" mechanic (hold position to resist knockback) with clear UI.
- TODO: Explicitly telegraph rescue tradeoff: time now vs shortcut later.
- TODO: Ensure every span segment has at least one safe reset pocket.

