---
adventure_id: q12-bloom-contract
run_id: "run-01"
party_size: 5
party_levels: [14, 14, 14, 14, 14]
party_classes: [fighter, cleric, wizard, rogue, ranger]
party_tags: [balanced, first-time, methodical]
expected_duration_min: 80
---

# Party Run: q12-bloom-contract / run-01

## Party

- size: 5
- levels: all 14
- classes: fighter, cleric, wizard, rogue, ranger
- tags: balanced, first-time, methodical

## What They Did (Timeline)

1. Hook: greenhouse staff offers a contract for three bloom samples; warns about spores and "caretaker drones" that will try to cleanse intruders.
2. Greenhouse hub: they learn the rules: samples must be harvested at three groves, then returned to open the Root Engine gate.
3. Wilds approach: ranger uses landmarks and "maintenance markers" to avoid looping; rogue scouts for tunnel grates.
4. Bloom Grove 1: spore fog pulses. Cleric establishes a "decon on stacks >= 2" rule; wizard controls mites; fighter holds a funnel while rogue harvests.
5. Safe pocket unlock: returning one sample unlocks a decontam chamber in the greenhouse (clear spore stacks, restock).
6. Bloom Grove 2: vine labyrinth with reach attacks from striders. Ranger calls targets; wizard locks lanes; fighter drags elites into clear squares.
7. Secret: rogue finds a greenhouse service tunnel that bypasses one exposed wilds segment (feels like competence).
8. Bloom Grove 3: caretaker drone shows up mid-harvest and tries to "sanitize" the party with a heal/cleanse cycle. They learn to interrupt/burst during harvest windows.
9. Root Engine arena: multi-stage. Phase 1 is "roots + adds"; phase 2 is "engine core exposed in brief windows." They win by saving burst for windows and using decon between pulses.
10. Resolution: deliver the three samples; greenhouse issues a stamped access token and opens The Core gate.

## Spotlight Moments (By Class)

- fighter: holds vine funnels so harvest can happen; keeps melee from chasing into spore fog.
- rogue: finds the service tunnel secret; times harvest interactions under pressure.
- cleric: makes spores legible (stack rule) and keeps the party stable between grove pulses.
- wizard: turns groves into solvable geometry with control and AoE.
- ranger: navigation and target priority in open terrain; prevents backtracking death spirals.
- paladin: no paladin; would shine on "protect the biome" choice language.
- bard: no bard; would shine negotiating contract clauses and coordinating harvest windows.
- druid: no druid; would shine interfacing with the garden machine for a non-combat advantage.
- barbarian: no barbarian; would shine cracking a seed-vault door for risk/reward loot.
- warlock: no warlock; would shine in optional bio-pact bargains.
- sorcerer: no sorcerer; would shine bursting caretaker heal cycles.
- monk: no monk; would shine in vine-choked movement and hazard dodging.

## Friction / Missing Content

- Need a loud, consistent spore stack UI: what causes stacks, what thresholds matter, and where to clear them.
- Harvest interaction must be very explicit (start / interrupted / success).
- Caretaker drone "heal/cleanse cycle" needs clear telegraphs so parties learn the counterplay.

## Extracted TODOs

- TODO: define `spore_load` stacks + decon rules (thresholds, recovery, sources).
- TODO: add consistent tunnel-grate tells for the service tunnel secret.

