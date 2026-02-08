---
adventure_id: q9-beacon-protocol
run_id: "run-01"
party_size: 5
party_levels: [10, 10, 10, 10, 10]
party_classes: [fighter, cleric, wizard, rogue, ranger]
party_tags: [balanced, first-time, methodical]
expected_duration_min: 55
---

# Party Run: q9-beacon-protocol / run-01

## Party

- size: 5
- levels: all 10
- classes: fighter, cleric, wizard, rogue, ranger
- tags: balanced, first-time, methodical

## What They Did (Timeline)

1. Hook: Underrail platform asks them to relight four beacons to stabilize routing and open the Skybridge line.
2. Platform: they get a map with four beacon IDs and a warning: "switchyard patrols loop; don’t fight the whole rail."
3. Beacon 1: simple defend-the-point; they learn the beacon interaction UX and the "beacon charge" timer.
4. Tunnels: rogue scouts, ranger picks off fast threats, wizard controls choke pulls.
5. Beacon 2: they trigger a patrol mid-charge and almost wipe; lesson: clear patrols before touching beacon.
6. Switchyard: fighter holds center while party learns timing windows; cleric keeps attrition manageable.
7. Secret: they find a maintenance crawl that skips the worst tunnel bend (feels like a reward for scouting).
8. Beacon 3 and 4: they start doing clean pulls and charge smoothly.
9. Boss: Platform Warden arena. First attempt fails to add snowball. Second attempt succeeds by prioritizing add caller / phase adds.
10. Resolution: they get a stamped "line authorization" and the platform gate opens.

## Spotlight Moments (By Class)

- fighter: anchors platforms and makes timing mistakes survivable.
- rogue: scouting and maintenance crawl secret; prevents ambush wipes.
- cleric: keeps the run stable under long traversal attrition.
- wizard: turns beacon defense waves into solvable chunks.
- ranger: threat removal at range, especially fast crawlers and ranged sentries.
- paladin: no paladin; would shine protecting stranded travelers on platforms.
- bard: no bard; would shine coordinating patrol timing and beacon order.
- druid: no druid; would shine navigating fungal hazards / nonlethal options.
- barbarian: no barbarian; would shine forcing open a sealed maintenance gate.
- warlock: no warlock; would shine binding a beacon faster for a cost.
- sorcerer: no sorcerer; would shine bursting add windows in boss.
- monk: no monk; would shine in switchyard timing and fast traversal.

## Friction / Missing Content

- Beacon UX needs loud text: start / interrupted / charged / cooldown.
- Patrol timing must be readable (audio cue, footsteps, signal lights).
- Need at least one safe pocket so wipes aren’t full re-clear pain.

## Extracted TODOs

- TODO: standardize beacon setpiece interface (charge timer + wave logic + completion flag).
- TODO: codify patrol timing grammar (signal lights, announcements, consistent loop length).

