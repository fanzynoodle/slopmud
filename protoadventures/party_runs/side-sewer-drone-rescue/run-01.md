---
adventure_id: "side-sewer-drone-rescue"
run_id: "run-01"
party_size: 3
party_levels: [4, 4, 4]
party_classes: ["fighter", "cleric", "rogue"]
party_tags: ["escort", "status-hazards", "careful", "first-time"]
expected_duration_min: 35
---

# Party Run: side-sewer-drone-rescue / run-01

## Party

- size: 3
- levels: 4/4/4
- classes: fighter, cleric, rogue
- tags: escort, status-hazards, careful, first-time

## What They Did (Timeline)

1. hook / acceptance
   - Got a side contract at the sewer maintenance office: "A drone is stuck in an optional wing. Bring it to the junction."
   - They assumed escort would be easy. It wasn't.
2. travel / navigation decisions
   - Rogue scouted the wing first and marked a safe return route (avoid flooded pockets).
   - Fighter insisted on clearing ambush rooms before moving the escort.
3. key fights / obstacles
   - Status hazards: sludge slow + minor toxin stacks.
   - The drone panicked when hit and tried to flee into side corridors unless calmed.
   - Cleric had to keep the drone alive and cleanse party so they could keep pace.
4. setpiece / spike
   - A valve room locked the return path until a lever was held while the drone crossed.
   - Rogue held the lever, fighter held the choke, cleric escorted the drone through.
5. secret / shortcut (or why they missed it)
   - They saw a locked maintenance door that looked like a shortcut, but no keycard.
6. resolution / rewards
   - Delivered the drone to the junction. It unlocked a small utility vendor and promised a future upgrade.

## Spotlight Moments (By Class)

- fighter: protected the escort through narrow corridors and held chokes during lever windows.
- cleric: kept drone alive through status hazards and prevented pace collapse.
- rogue: scouted ahead, disarmed a trap, and held the lever under pressure.

## Friction / Missing Content

- Drone behavior needs to be readable: what triggers panic and how to calm it.
- Escort pathing must avoid obvious suicide routes (flooded pockets).
- Status hazards should be loud and consistent so parties can plan.

## Extracted TODOs

- TODO: Define drone panic triggers + calm interaction (bard/cleric/command).
- TODO: Add one clear shortcut unlock (keycard) as optional reward.
- TODO: Ensure escort pathing respects "safe lane" markers.

