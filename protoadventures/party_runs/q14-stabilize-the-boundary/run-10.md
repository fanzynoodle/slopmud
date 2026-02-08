---
adventure_id: q14-stabilize-the-boundary
run_id: "run-10"
claimed_by: "codex"
party_size: 5
party_levels: [19, 19, 19, 19, 19]
party_classes: [warlock, barbarian, cleric, wizard, bard]
party_tags: [chaotic, temptation-heavy, wipe-once]
expected_duration_min: 150
---

# Party Run: q14-stabilize-the-boundary / run-10

## Party

- size: 5
- levels: all 19
- classes: warlock, barbarian, cleric, wizard, bard
- tags: chaotic, temptation-heavy, wipe-once

## What They Did (Timeline)

1. Hook: warlock wants temptations; barbarian wants to break reality; bard wants a story.
2. Rift 4: they take a temptation to ignore glass choir once. The UI is not loud enough; they don’t realize they took debt.
3. They wipe in rift 2 when silence pockets drift faster due to the debt consequence.
4. Reset: bard imposes rules (cap debt at 2, banner calls, never panic-run). Cleric stabilizes.
5. They finish rifts cleanly once disciplined. They pick `glass_choir` reinforcement because they like rhythm.
6. Nexus: warlock takes one more temptation; costs are explicit this time; they pay down at edge station before nexus attempt.

## Spotlight Moments (By Class)

- warlock: temptation system poster child; adds risk texture when explicit.
- barbarian: window breaker; learns discipline after wipe.
- cleric: recovery after wipe; keeps chaos survivable.
- wizard: rule caller and control; turns chaos into solvable phases.
- bard: comms and cadence; the win condition for nexus swaps.

## Friction / Missing Content

- Temptation costs must be loud, capped, and reversible via paydown. Otherwise it feels like delayed punishment.
- Need a clear, guaranteed paydown location (edge station or reset pocket) before nexus.

## Extracted TODOs

- TODO: implement `reality_debt` UX: token count, next consequence, cap, paydown.
- TODO: add explicit "pay down debt" option at edge station before nexus.

