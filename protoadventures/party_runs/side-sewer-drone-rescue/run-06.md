---
adventure_id: "side-sewer-drone-rescue"
run_id: "run-06"
party_size: 2
party_levels: [4, 4]
party_classes: ["monk", "paladin"]
party_tags: ["duo", "carry-drone", "valve-timing", "no-cleanse"]
expected_duration_min: 40
---

# Party Run: side-sewer-drone-rescue / run-06

## Party

- size: 2
- levels: 4/4
- classes: monk, paladin
- tags: duo, carry-drone, valve-timing, no-cleanse

## What They Did (Timeline)

1. hook / acceptance
   - Took the rescue as a duo and decided to treat it like an escort discipline tutorial.
2. travel / navigation decisions
   - Monk sprint-scouted ahead and cleared ambush pockets; paladin stayed with the drone and refused to move until the scout returned.
3. key fights / obstacles
   - Without cleanse, hazard puddles were the real damage source. The duo had to keep the drone completely out of sludge lanes.
4. setpiece / spike
   - Flooded section: they carried the drone across during a short "safe window" after draining one valve, rather than doing the full two-valve sequence.
   - Valve hold room forced role assignment: monk held the lever window while paladin body-screened the drone.
5. secret / shortcut (or why they missed it)
   - Missed keycard door; no time to search off-route while escorting.
6. resolution / rewards
   - Delivered the drone with low damage. Duo felt fair when "clear then move" was enforceable and panic triggers were readable.

## Spotlight Moments (By Class)

- monk: scout pacing and lever window execution under pressure.
- paladin: body-screened the drone and kept escort discipline intact.
- monk/paladin: executed a risky carry across the flooded pocket during the safe window.

## Friction / Missing Content

- Carry rules need to be explicit (speed penalty, what hazards apply, how panic changes).
- Valve windows need loud feedback so duos know when the crossing is safe.
- Drone pathing must respect safe lanes; if it wanders, duo play becomes impossible.

## Extracted TODOs

- TODO: Add explicit carry mechanics UI (movement penalty, hazard immunity/vulnerability, panic effects).
- TODO: Add clear valve window indicators (open/closed state + countdown).
- TODO: Make drone follow behavior lane-aware (stay in safe lane markers).

