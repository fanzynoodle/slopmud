---
adventure_id: q7-conveyor-war
run_id: "run-08"
party_size: 2
party_levels: [9, 9]
party_classes: [wizard, fighter]
party_tags: [small-party, bot-autofill, methodical]
expected_duration_min: 55
---

# Party Run: q7-conveyor-war / run-08

## Party

- size: 2 humans (wizard, fighter)
- levels: both 9
- classes: wizard, fighter
- tags: small-party, bot-autofill, methodical

## What They Did (Timeline)

1. Hook: they accept it as a duo, but the quest warns the setpieces are tuned for 3-5. They opt into bot autofill to reach 3.
2. Entry: bot vanguard helps in corridors, but pathing on belts is dangerous. They learn bots must be belt-aware or they become liabilities.
3. Shutdowns: with control + tank, they clear reliably. The only scary moments are when the bot gets knocked into hazard and needs rescue.
4. Boss: the bot is surprisingly helpful on adds, but only if it prioritizes the add caller.
5. Resolution: they like the "duo + bot trio" model, but only if bots understand geometry.

## Spotlight Moments (By Class)

- fighter: holds every corridor cleanly; makes the run possible as a duo.
- wizard: solves turret corridors and add waves; turns hard rooms into puzzles.
- rogue: no rogue in party; would speed route-finding and enable quiet routes.
- cleric: no cleric in party; a healer bot or sustain item is required.
- ranger: no ranger in party; would reduce corridor risk with picks.
- paladin: no paladin in party; would shine in rescue sub-objectives.
- bard: no bard in party; would help coordinate bot behavior (commands).
- druid: no druid in party; would help with hazard mitigation.
- barbarian: no barbarian in party; would shine in lever-rip objective moments.
- warlock: no warlock in party; would shine in overclock bargains for speed.
- sorcerer: no sorcerer in party; would shine in burst windows.
- monk: no monk in party; traversal is slower and more dangerous.

## Friction / Missing Content

- Bot AI must treat conveyors as lethal terrain.
- Need a simple bot command set: "hold here", "cross now", "focus add caller".

## Extracted TODOs

- TODO: define bot behavior profiles for industrial geometry (belt-aware pathing).
- TODO: add a "safe square" marking system bots can read.

