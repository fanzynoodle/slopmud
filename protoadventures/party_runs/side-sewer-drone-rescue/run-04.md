---
adventure_id: "side-sewer-drone-rescue"
run_id: "run-04"
party_size: 5
party_levels: [3, 3, 3, 3, 3]
party_classes: ["fighter", "rogue", "bard", "wizard", "ranger"]
party_tags: ["under-leveled", "escort", "panic-meter", "slow-clear"]
expected_duration_min: 45
---

# Party Run: side-sewer-drone-rescue / run-04

## Party

- size: 5
- levels: 3/3/3/3/3
- classes: fighter, rogue, bard, wizard, ranger
- tags: under-leveled, escort, panic-meter, slow-clear

## What They Did (Timeline)

1. hook / acceptance
   - Took the maintenance contract: "Find the missing drone, escort it to the junction." They were under-leveled but had five bodies.
2. travel / navigation decisions
   - Ranger navigated and marked safe pockets. Rogue scouted the optional wing gate first so the group didn't start the escort in an unsafe room.
3. key fights / obstacles
   - Under-leveled damage meant fights took longer, which made the escort harder. Drone panic spiked whenever it took splash damage, and the party struggled to keep it out of hazard puddles.
4. setpiece / spike
   - Flooded section: they chose to drain water with a two-valve sequence instead of carrying the drone, because carrying under level 3 felt like walking through poison.
5. secret / shortcut (or why they missed it)
   - Saw the keycard door but didn't have a keycard; they wished there was a clear "earn it here" step before the escort starts.
6. resolution / rewards
   - Delivered the drone with moderate damage. Succeeded, but it was a stress test that showed under-leveled runs need very readable panic triggers and safe lanes.

## Spotlight Moments (By Class)

- rogue: scouted and disarmed a trap so the escort didn't eat unavoidable damage.
- bard: calmed the drone repeatedly and prevented panic fleeing into side corridors.
- fighter: held chokes while the group did valve interactions and moved the drone.
- wizard: controlled ambush rooms so the escort didn't get stuck fighting in hazards.
- ranger: route selection and safe pocket marking through sludge loops.

## Friction / Missing Content

- Drone panic triggers must be explicit (what counts as "hit", what is "safe") or under-leveled runs feel unfair.
- Escort safe lane markers need to exist; follow-AI wandering into sludge is a disaster.
- Valve sequence feedback needs to be loud so parties know whether they're progressing.

## Extracted TODOs

- TODO: Add a visible panic meter with explicit triggers and "calm" affordances.
- TODO: Add a consistent escort safe lane language (paint, cones, arrows) that the drone respects.
- TODO: Add valve puzzle UI feedback (which valve is correct, state changes, reset path).

