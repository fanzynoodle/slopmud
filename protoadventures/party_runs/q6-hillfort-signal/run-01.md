---
adventure_id: "q6-hillfort-signal"
run_id: "run-01"
party_size: 4
party_levels: [5, 5, 5, 6]
party_classes: ["fighter", "cleric", "wizard", "rogue"]
party_tags: ["brand-new", "balanced", "no-scout"]
expected_duration_min: 55
---

# Party Run: q6-hillfort-signal / run-01

## Party

- size: 4
- levels: 5/5/5/6
- classes: fighter, cleric, wizard, rogue
- tags: brand-new, balanced, no-scout

## What They Did (Timeline)

1. hook / acceptance
   - Entered the command bunker, took the brief, asked "what are pylons" and "do we have a map".
   - The party wanted a tangible objective marker: "how do we know a pylon is lit".
2. travel / navigation decisions
   - Moved into the courtyard fork, chose pylon 2 first because the approach sounded narrow and "safer".
   - Failed once due to a bad pull, then decided to clear slow and conserve resources.
3. key fights / obstacles
   - Pylon 2: ranged pressure forced advance; they learned quickly to not turtle.
   - Pylon 1: held a point for a timer; the wizard wanted a clear "30 seconds remaining" cue.
4. setpiece / spike
   - Pylon 3: hazard telegraphs were the star. They initially treated it like random damage until the room messaging made it obvious.
   - Banner hall boss: adds were manageable, but the party missed the interrupt lesson until the boss had already summoned twice.
5. secret / shortcut (or why they missed it)
   - No secret found. They assumed there was none because the courtyard was "obviously a hub".
   - They asked for a "collapsed side stair" or "banner slit door" clue to reward curiosity.
6. resolution / rewards
   - Returned to command, got the safehouse mark, then walked to the rail terminal and printed the pass.
   - They wanted a sign that the rail pass matters right now (even if it's a blocked exit).

## Spotlight Moments (By Class)

- fighter: anchored the pylon 1 hold; used body-block framing to keep adds off the wizard.
- rogue: took point in the narrow pylon 2 approach; found a flanking angle when the party stalled.
- cleric: stabilized two near-downs in the banner hall; prompted a retreat-and-reset plan after a bad summon cycle.
- wizard: controlled waves on pylon 1; learned to save a hard stop for the boss call.
- ranger: none (not in party)
- paladin: none (not in party)
- bard: none (not in party)
- druid: none (not in party)
- barbarian: none (not in party)
- warlock: none (not in party)
- sorcerer: none (not in party)
- monk: none (not in party)

## Friction / Missing Content

- Need a clear "pylon lit" confirmation (audio + text + visible change in the room).
- Need a clear sealed-exit message at `R_HILL_BANNER_01` before pylons are lit ("sealed until 3 pylons").
- Interrupt lesson: the boss summon should have stronger telegraph and explicit "this can be interrupted" hint once.
- Timer content needs readable feedback ("wave 2/3", "15s remaining") so parties don't feel punished by hidden time.

## Extracted TODOs

- TODO: Add a simple map prop in `R_HILL_CMD_01` that lists pylon directions in plain text.
- TODO: Add one optional secret in the courtyard cluster (small shortcut or lore cache).
- TODO: Add a one-time tutorial hint line on the first boss summon: "the call can be broken if you hit hard enough, fast enough".

