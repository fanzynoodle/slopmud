---
adventure_id: "q5-sunken-index"
run_id: "run-03"
party_size: 5
party_levels: [8, 8, 8, 8, 8]
party_classes: ["paladin", "bard", "ranger", "druid", "sorcerer"]
party_tags: ["over-leveled", "fast-clear", "lore-hounds"]
expected_duration_min: 35
---

# Party Run: q5-sunken-index / run-03

## Party

- size: 5
- levels: 8/8/8/8/8
- classes: paladin, bard, ranger, druid, sorcerer
- tags: over-leveled, fast-clear, lore-hounds

## What They Did (Timeline)

1. hook / acceptance
   - Treated the ranger camp as a briefing and asked for symbols that correspond to the plates.
   - Wanted a single sentence about why the index key matters beyond "unlock doors".
2. travel / navigation decisions
   - Speed-cleared the stacks, then did shrines in a route chosen by the ranger (felt good for spotlight).
   - Wanted visible progress in the antechamber socket after each plate so the run feels like accumulating state.
3. key fights / obstacles
   - Combat was easy at this level. The fun came from mechanics that forced movement and objective play.
   - They asked for one optional "overcharge the decoder" knob to make the final room interesting.
4. setpiece / spike
   - Shrine 1 pulse was ignored because they were strong; they wanted a failure mode that is not just damage (slow, separation, or forced movement).
   - Shrine 2 objective was good because it demanded focus, not DPS.
5. secret / shortcut (or why they missed it)
   - Found no secret. Wanted a lore-only secret that expands the Library without changing the unlock choice.
6. resolution / rewards
   - Decoder was satisfying, but the map should feel like it changes the overworld, not just flips a flag.
   - The choice should hint at catch-up: "you will unlock the other door later".

## Spotlight Moments (By Class)

- paladin: framed the choice as a responsibility and insisted the door selection should be explicit and logged.
- bard: decoded hints and pushed for a lore reward that pays off later.
- ranger: led the route through the stacks using environmental signs.
- druid: engaged with mold sprites as a "cleanse/restore" story instead of just enemies.
- sorcerer: executed a high-risk burst to end a pulse sequence before it compounded.
- fighter: none
- rogue: none
- cleric: none
- wizard: none
- barbarian: none
- warlock: none
- monk: none

## Friction / Missing Content

- Over-leveled parties need optional difficulty knobs (decoder overcharge, extra objective, or harder shrine variant).
- The antechamber socket should visibly track plate progress.
- The decoder output should include a tangible teaser: one sentence about what factory vs reservoir means.

## Extracted TODOs

- TODO: Add visible plate socket progress in `R_LIB_ANTE_01` after each plate.
- TODO: Add an optional decoder overcharge mechanic with clear risk/reward.
- TODO: Add a lore-only secret note somewhere in the stacks cluster.

