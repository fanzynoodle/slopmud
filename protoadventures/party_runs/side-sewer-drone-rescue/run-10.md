---
adventure_id: "side-sewer-drone-rescue"
run_id: "run-10"
party_size: 2
party_levels: [3, 3]
party_classes: ["rogue", "wizard"]
party_tags: ["under-leveled", "trap-hit", "failed-once", "needs-telegraphs"]
expected_duration_min: 55
---

# Party Run: side-sewer-drone-rescue / run-10

## Party

- size: 2
- levels: 3/3
- classes: rogue, wizard
- tags: under-leveled, trap-hit, failed-once, needs-telegraphs

## What They Did (Timeline)

1. hook / acceptance
   - Took the rescue too early, hoping stealth + control would compensate for low level.
2. travel / navigation decisions
   - Started the escort immediately after finding the drone, without clearing the return lane. That was the mistake.
3. key fights / obstacles
   - Trap room: the drone took a hit from a barely-visible trap and panic spiked. The duo couldn't recover and had to reset (effectively a failure run).
4. setpiece / spike
   - Second attempt: rogue scouted and disarmed everything first; wizard controlled ambushes so the drone never entered the fight lane.
   - They paid for a keycard shortcut to bypass the flooded pocket because under-level attrition would have killed them.
5. secret / shortcut (or why they missed it)
   - Keycard shortcut made the duo run possible, but the acquisition method needs to be obvious before the escort starts.
6. resolution / rewards
   - Delivered the drone with heavy resource use. The run proved escort+traps must be readable; otherwise failure feels arbitrary.

## Spotlight Moments (By Class)

- rogue: trap scouting/disarm made the second attempt viable.
- wizard: lane control prevented panic spikes from splash pulls.
- rogue/wizard: learned the correct escort pacing: clear lane first, then move.

## Friction / Missing Content

- Trap telegraphs must be strong when an escort is present; hidden escort damage is unacceptable.
- Under-leveled parties need an explicit warning that the wing is dangerous at the bottom of the band.
- Keycard shortcut planning needs to be possible before committing to the escort route.

## Extracted TODOs

- TODO: Improve trap readability and add escort-specific warnings (floor paint, audio, preview).
- TODO: Add a minimum level / difficulty warning at quest accept for party_levels near the bottom of the band.
- TODO: Add a clear keycard acquisition plan before escort begins (console offer, side chest, clear cost).

