---
adventure_id: q12-bloom-contract
run_id: "run-08"
party_size: 5
party_levels: [15, 15, 15, 15, 15]
party_classes: [warlock, barbarian, cleric, wizard, bard]
party_tags: [bargain-heavy, mutation-curious, wipe-once]
expected_duration_min: 100
---

# Party Run: q12-bloom-contract / run-08

## Party

- size: 5
- levels: all 15
- classes: warlock, barbarian, cleric, wizard, bard
- tags: bargain-heavy, mutation-curious, wipe-once

## What They Did (Timeline)

1. Hook: warlock wants a bio-pact. Bard wants a story. Barbarian wants to smash a seed-vault.
2. Greenhouse: staff warns them that "mutations" have costs. Warlock takes the pact anyway for faster harvest tempo.
3. Grove 1: pact lets them ignore one spore pulse, but then spikes spore severity later. They misread the cost and over-commit.
4. They wipe in Grove 2 when pact cost + caretaker adds overlap.
5. Reset: bard forces a rule: "one pact max; costs must be loud; decon after every grove."
6. Grove 3: they win by timing harvest windows and saving burst for caretaker heal cycles.
7. Root Engine: the pact cost shows up as extra hazard intensity. They survive because cleric holds the line and wizard controls adds.
8. Resolution: pact systems are fun when costs are explicit, capped, and persistent.

## Spotlight Moments (By Class)

- warlock: bio-pact poster child; creates new texture for reruns.
- barbarian: seed-vault smash side objective; also learns discipline after wipe.
- cleric: stabilizes after wipe; makes pact debt survivable.
- wizard: converts chaos into a plan with control and positioning.
- bard: enforces cadence and rules; party coordination is the win condition.

## Friction / Missing Content

- If we add bio-pacts, the UI must be loud: token count, next consequence, how to pay it down.
- Need a cap on "mutation debt" so parties can't self-delete by stacking.

## Extracted TODOs

- TODO: define `mutation_debt` tokens (cap, consequences, paydown in decon chamber).
- TODO: add explicit pact dialogue warnings that summarize the next consequence.

