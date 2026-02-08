---
adventure_id: q13-heart-of-the-machine
run_id: "run-09"
party_size: 5
party_levels: [16, 16, 16, 16, 16]
party_classes: [cleric, cleric, fighter, wizard, ranger]
party_tags: [double-healer, safe, slow]
expected_duration_min: 115
---

# Party Run: q13-heart-of-the-machine / run-09

## Party

- size: 5
- levels: all 16
- classes: cleric, cleric, fighter, wizard, ranger
- tags: double-healer, safe, slow

## What They Did (Timeline)

1. Hook: they go in expecting pain and bring two clerics.
2. They brute-force some chip damage, but geometry still kills you if you ignore it.
3. Locks: sustain makes lock 3 easy, but lock 2 still requires correct movement.
4. Auditor arena: they win, but the run is slow. It demonstrates healing should not trivialize the mechanic.

## Spotlight Moments (By Class)

- cleric: sustain makes long run possible, but must respect geometry.
- fighter: holds center and keeps movement disciplined.
- wizard: control makes lock phases manageable.
- ranger: deletes priority adds so healers can focus.

## Friction / Missing Content

- Avoid making the dungeon purely a sustain check; keep "movement is the mechanic" dominant.
- Ensure hazard kills remain scary even with heavy healing (positioning failures).

## Extracted TODOs

- TODO: make geometry failures bypass healing (forced movement, knockback, instant zones).
- TODO: ensure lock 3 is not purely "healer check" (add an objective window).

