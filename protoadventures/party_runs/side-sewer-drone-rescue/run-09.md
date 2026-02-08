---
adventure_id: "side-sewer-drone-rescue"
run_id: "run-09"
party_size: 5
party_levels: [5, 5, 5, 5, 5]
party_classes: ["fighter", "cleric", "wizard", "rogue", "monk"]
party_tags: ["speed-bonus-attempt", "low-drone-damage", "clear-then-move", "clean"]
expected_duration_min: 30
---

# Party Run: side-sewer-drone-rescue / run-09

## Party

- size: 5
- levels: 5/5/5/5/5
- classes: fighter, cleric, wizard, rogue, monk
- tags: speed-bonus-attempt, low-drone-damage, clear-then-move, clean

## What They Did (Timeline)

1. hook / acceptance
   - Took the quest with an explicit goal: "deliver with low damage for the better vendor tier."
2. travel / navigation decisions
   - Monk and rogue alternated scouting ahead, always returning before the drone moved. Fighter/cleric stayed escort-side; wizard controlled from midline.
3. key fights / obstacles
   - They avoided all splash near the drone and treated safe pockets as mandatory stops. Panic never spiked because the drone was never in the fight lane.
4. setpiece / spike
   - Valve lock room: rogue held lever, fighter held choke, cleric kept drone topped, monk cleared adds that leaked, wizard denied the ambush lane.
   - Flooded pocket: they chose the full drain sequence because it guaranteed drone safety.
5. secret / shortcut (or why they missed it)
   - Skipped the keycard shortcut on purpose; they didn't want to gamble on finding it mid-run.
6. resolution / rewards
   - Delivered the drone quickly and cleanly, earning a better payout/discount. This felt like the "intended perfect run" pattern.

## Spotlight Moments (By Class)

- rogue/monk: disciplined scout pattern kept the escort out of danger.
- fighter: choke control during lever windows and ambush leaks.
- cleric: kept the drone stable and enforced stop discipline in safe pockets.
- wizard: lane control so fights happened away from the escort.

## Friction / Missing Content

- The game needs a clear "park the escort here" affordance so teams know when it's safe to fight.
- Valve/drain sequence needs unmistakable feedback; otherwise teams won't trust it for clean runs.
- "Low drone damage" bonus criteria should be explicit so players know what they're aiming for.

## Extracted TODOs

- TODO: Add a "safe pocket" UI ping where escorts should be parked during fights.
- TODO: Add loud valve/drain feedback (state indicators + water level changes).
- TODO: Add explicit bonus criteria and reward messaging for low escort damage runs.

