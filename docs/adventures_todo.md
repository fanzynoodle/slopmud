# Adventures TODO (20 Broad Adventures + Party Runs)

This is the work queue for turning Gaia into a shippable, teachable set of linear quests.

Read the process first:

- `docs/adventure_iteration.md`

Status note:

- File existence is the source of truth (not checkboxes in this doc).
- Use `scripts/party_runs_status.sh` to see what is missing.

## How To Contribute (Agent-Friendly)

- Pick an `adventure_id` below.
- Pick a run number you will own (`run-01`..`run-10`).
- Create a party run report at `protoadventures/party_runs/<adventure_id>/run-XX.md`.
- Do not block on engine features. Just write what the party *tries*, where they get stuck, and what the world needs.
- After a few runs exist, draft/iterate the corresponding protoadventure at `protoadventures/<adventure_id>.md`.
- Optional playtest loop: `just dev-run` then `proto <adventure_id>` (see `protoadventures/README.md`).

To avoid merge conflicts: prefer adding new run files over editing this doc. It is OK to flip the checkbox for a run you completed from `[ ]` to `[x]` (minimal edit).

### Definition Of Done (Party Run)

A run is "done" when the file exists and contains:

- filled YAML front matter (including `party_size`, `party_levels`, `party_classes`, `party_tags`)
- a complete 1..6 timeline (hook -> resolution)
- at least 3 spotlight moments (or explicit "no moment" callouts)
- at least 3 friction notes and 3 extracted TODOs

### Tip: Claiming Work (No Conflicts)

Don't edit this file to claim runs.

Create the run file you will own; that is the claim.

If you want to show ownership, set `claimed_by` in the run file YAML.

```bash
adventure_id="q2-job-board-never-sleeps"
run_id="run-01"
mkdir -p "protoadventures/party_runs/${adventure_id}"
cp protoadventures/party_runs/_RUN_TEMPLATE.md "protoadventures/party_runs/${adventure_id}/${run_id}.md"
```

## Classes (Provisional)

We are using a provisional "classic fantasy" class list for spotlight planning:

- fighter, rogue, cleric, wizard, ranger, paladin, bard, druid, barbarian, warlock, sorcerer, monk

If/when slopmud uses a different class system, we will remap spotlights.

## Adventure List

Each adventure should eventually have:

- 10 party runs in `protoadventures/party_runs/<adventure_id>/`
- a protoadventure file in `protoadventures/<adventure_id>.md`
- at least 1 spotlight hook per class on paper (not necessarily unique mechanics yet)

---

## 01) `q1-first-day-on-gaia`

- Level band: 1-2; party: 1
- Zones: Newbie School -> Town Gate
- Protoadventure: `protoadventures/q1-first-day-on-gaia.md` (exists)
- Spotlights (sketch):
- [ ] fighter: practice a "protect the trainee" drill in labs
- [ ] rogue: find a hidden supply closet in dorms (search/stealth)
- [ ] cleric: triage tutorial (revive/recovery framing)
- [ ] wizard: hazard strip teaches positioning and "spell timing"
- [ ] ranger: navigation drill: signage and breadcrumbing
- [ ] paladin: oath-like "town pass" acceptance moment (ethos)
- [ ] bard: social room: defuse an argument, earn a small perk
- [ ] druid: sim yard "environmental hazard" awareness
- [ ] barbarian: smash-test a training dummy, learn restraint
- [ ] warlock: "deal" flavor with an instructor AI (consent + warning)
- [ ] sorcerer: uncontrollable surge drill (teach safe failure)
- [ ] monk: movement drill (don’t stand in hazards)
- Party runs:
- [x] run-01: `protoadventures/party_runs/q1-first-day-on-gaia/run-01.md`
- [x] run-02: `protoadventures/party_runs/q1-first-day-on-gaia/run-02.md`
- [x] run-03: `protoadventures/party_runs/q1-first-day-on-gaia/run-03.md`
- [x] run-04: `protoadventures/party_runs/q1-first-day-on-gaia/run-04.md`
- [x] run-05: `protoadventures/party_runs/q1-first-day-on-gaia/run-05.md`
- [x] run-06: `protoadventures/party_runs/q1-first-day-on-gaia/run-06.md`
- [x] run-07: `protoadventures/party_runs/q1-first-day-on-gaia/run-07.md`
- [x] run-08: `protoadventures/party_runs/q1-first-day-on-gaia/run-08.md`
- [x] run-09: `protoadventures/party_runs/q1-first-day-on-gaia/run-09.md`
- [x] run-10: `protoadventures/party_runs/q1-first-day-on-gaia/run-10.md`

---

## 02) `q2-job-board-never-sleeps`

- Level band: 1-4; party: 1-3
- Zones: Town + Meadowline + Scrap Orchard
- Protoadventure: `protoadventures/q2-job-board-never-sleeps.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold a choke in pestsweep while others do objective
- [ ] rogue: spot a backlane shortcut and shave time
- [ ] cleric: cleanse a mild status from orchard drones (after fight)
- [ ] wizard: control a multi-pack pull (avoid over-aggro)
- [ ] ranger: track a contract target via environmental signs
- [ ] paladin: enforce "no soliciting" on a shady courier
- [ ] bard: negotiate better pay (flavor reward)
- [ ] druid: calm a panicked animal drone (nonlethal option)
- [ ] barbarian: break a barricade blocking trail (alternate path)
- [ ] warlock: accept a risky bonus clause (opt-in difficulty)
- [ ] sorcerer: improvise a fast-clear strategy (speedrun)
- [ ] monk: deliver package under time pressure (movement)
- Party runs:
- [x] run-01: `protoadventures/party_runs/q2-job-board-never-sleeps/run-01.md`
- [x] run-02: `protoadventures/party_runs/q2-job-board-never-sleeps/run-02.md`
- [x] run-03: `protoadventures/party_runs/q2-job-board-never-sleeps/run-03.md`
- [x] run-04: `protoadventures/party_runs/q2-job-board-never-sleeps/run-04.md`
- [x] run-05: `protoadventures/party_runs/q2-job-board-never-sleeps/run-05.md`
- [x] run-06: `protoadventures/party_runs/q2-job-board-never-sleeps/run-06.md`
- [x] run-07: `protoadventures/party_runs/q2-job-board-never-sleeps/run-07.md`
- [x] run-08: `protoadventures/party_runs/q2-job-board-never-sleeps/run-08.md`
- [x] run-09: `protoadventures/party_runs/q2-job-board-never-sleeps/run-09.md`
- [x] run-10: `protoadventures/party_runs/q2-job-board-never-sleeps/run-10.md`

---

## 03) `q3-sewer-valves`

- Level band: 3-4; party: 3-5
- Zones: Town Maint -> Under-Town Sewers
- Protoadventure: `protoadventures/q3-sewer-valves.md` (exists)
- Spotlights (sketch):
- [ ] fighter: anchor a valve room while others turn the wheel
- [ ] rogue: bypass a flooded side tunnel to reach a valve faster
- [ ] cleric: manage sludge/poison-ish effects (recovery)
- [ ] wizard: solve a "grate archers" room via control/LOS
- [ ] ranger: map the hub-and-spoke layout for the party
- [ ] paladin: cleanse fear/rot flavor from the boss arena
- [ ] bard: keep party morale up after wipes; grant a "rally" perk
- [ ] druid: interact with sewer fauna (leeches) via non-combat option
- [ ] barbarian: brute-force open a stuck maintenance hatch (shortcut)
- [ ] warlock: bargain with a "maintenance daemon" for a keycard (opt-in)
- [ ] sorcerer: burst-clear adds in the grease king arena
- [ ] monk: movement-heavy valve run (don’t stand in sludge)
- Party runs:
- [x] run-01: `protoadventures/party_runs/q3-sewer-valves/run-01.md`
- [x] run-02: `protoadventures/party_runs/q3-sewer-valves/run-02.md`
- [x] run-03: `protoadventures/party_runs/q3-sewer-valves/run-03.md`
- [x] run-04: `protoadventures/party_runs/q3-sewer-valves/run-04.md`
- [x] run-05: `protoadventures/party_runs/q3-sewer-valves/run-05.md`
- [x] run-06: `protoadventures/party_runs/q3-sewer-valves/run-06.md`
- [x] run-07: `protoadventures/party_runs/q3-sewer-valves/run-07.md`
- [x] run-08: `protoadventures/party_runs/q3-sewer-valves/run-08.md`
- [x] run-09: `protoadventures/party_runs/q3-sewer-valves/run-09.md`
- [x] run-10: `protoadventures/party_runs/q3-sewer-valves/run-10.md`

---

## 04) `q4-quarry-rights`

- Level band: 4-6; party: 3-5
- Zones: Quarry -> Old Road Checkpoint
- Protoadventure: `protoadventures/q4-quarry-rights.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold an open-pit pull and protect squishies
- [ ] rogue: steal proof from an illegal dig without aggroing the whole camp
- [ ] cleric: keep party up through heavy hits; teach interrupts via "save"
- [ ] wizard: solve a worm-ambush room via area denial
- [ ] ranger: track bandits along the old road
- [ ] paladin: adjudicate union vs foreman choice (ethos)
- [ ] bard: talk down a strike; reduce patrol spawns (flavor)
- [ ] druid: stabilize a "rock-biter" burrow (optional)
- [ ] barbarian: smash a barricade to access elite arena faster
- [ ] warlock: accept a cursed ore chunk for power (optional)
- [ ] sorcerer: burst the named elite’s add phase
- [ ] monk: dodge-heavy duel vs Breaker-7
- Party runs:
- [x] run-01: `protoadventures/party_runs/q4-quarry-rights/run-01.md`
- [x] run-02: `protoadventures/party_runs/q4-quarry-rights/run-02.md`
- [x] run-03: `protoadventures/party_runs/q4-quarry-rights/run-03.md`
- [x] run-04: `protoadventures/party_runs/q4-quarry-rights/run-04.md`
- [x] run-05: `protoadventures/party_runs/q4-quarry-rights/run-05.md`
- [x] run-06: `protoadventures/party_runs/q4-quarry-rights/run-06.md`
- [x] run-07: `protoadventures/party_runs/q4-quarry-rights/run-07.md`
- [x] run-08: `protoadventures/party_runs/q4-quarry-rights/run-08.md`
- [x] run-09: `protoadventures/party_runs/q4-quarry-rights/run-09.md`
- [x] run-10: `protoadventures/party_runs/q4-quarry-rights/run-10.md`

---

## 05) `q5-sunken-index`

- Level band: 6-8; party: 3-5
- Zones: Rustwood -> Sunken Library
- Protoadventure: `protoadventures/q5-sunken-index.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold a stacks choke while plates are retrieved
- [ ] rogue: bypass a flooded wing to reach a plate shrine safely
- [ ] cleric: manage debuffs and exhaustion-like attrition
- [ ] wizard: solve sentry pins with control/teleports (later)
- [ ] ranger: navigation hints in rustwood trails; track plates via symbols
- [ ] paladin: choose the first unlock (factory vs reservoir) with consequences
- [ ] bard: decode hints via lore skill; speed up the decoder
- [ ] druid: interact with mold sprites via cleanse/nonlethal option
- [ ] barbarian: brute-force open a sealed archive door (shortcut)
- [ ] warlock: accept a "forbidden index" side deal (optional)
- [ ] sorcerer: high-risk burst to avoid flood pulses
- [ ] monk: movement puzzle shrine (timed plates)
- Party runs:
- [x] run-01: `protoadventures/party_runs/q5-sunken-index/run-01.md`
- [x] run-02: `protoadventures/party_runs/q5-sunken-index/run-02.md`
- [x] run-03: `protoadventures/party_runs/q5-sunken-index/run-03.md`
- [x] run-04: `protoadventures/party_runs/q5-sunken-index/run-04.md`
- [x] run-05: `protoadventures/party_runs/q5-sunken-index/run-05.md`
- [x] run-06: `protoadventures/party_runs/q5-sunken-index/run-06.md`
- [x] run-07: `protoadventures/party_runs/q5-sunken-index/run-07.md`
- [x] run-08: `protoadventures/party_runs/q5-sunken-index/run-08.md`
- [x] run-09: `protoadventures/party_runs/q5-sunken-index/run-09.md`
- [x] run-10: `protoadventures/party_runs/q5-sunken-index/run-10.md`

---

## 06) `q6-hillfort-signal`

- Level band: 5-7; party: 3-5
- Zones: Hillfort -> Rail Spur
- Protoadventure: `protoadventures/q6-hillfort-signal.md` (exists)
- Spotlights (sketch):
- [ ] fighter: defend pylons under waves
- [ ] rogue: scout patrol paths and mark safe routes
- [ ] cleric: keep team upright through attrition waves
- [ ] wizard: control a courtyard pull; solve banner-hall add call
- [ ] ranger: track pylon locations; guide the route
- [ ] paladin: safehouse unlock moment (protect the travelers)
- [ ] bard: rally after a wipe; reduce morale penalty (future)
- [ ] druid: stabilize collapsed section hazards
- [ ] barbarian: smash through a blocked courtyard gate (shortcut)
- [ ] warlock: risk a pylon "overcharge" for faster clear (optional)
- [ ] sorcerer: burst banner-hall adds before overwhelm
- [ ] monk: knockback-proof traversal on spans
- Party runs:
- [x] run-01: `protoadventures/party_runs/q6-hillfort-signal/run-01.md`
- [x] run-02: `protoadventures/party_runs/q6-hillfort-signal/run-02.md`
- [x] run-03: `protoadventures/party_runs/q6-hillfort-signal/run-03.md`
- [x] run-04: `protoadventures/party_runs/q6-hillfort-signal/run-04.md`
- [x] run-05: `protoadventures/party_runs/q6-hillfort-signal/run-05.md`
- [x] run-06: `protoadventures/party_runs/q6-hillfort-signal/run-06.md`
- [x] run-07: `protoadventures/party_runs/q6-hillfort-signal/run-07.md`
- [x] run-08: `protoadventures/party_runs/q6-hillfort-signal/run-08.md`
- [x] run-09: `protoadventures/party_runs/q6-hillfort-signal/run-09.md`
- [x] run-10: `protoadventures/party_runs/q6-hillfort-signal/run-10.md`

---

## 07) `q7-conveyor-war`

- Level band: 7-10; party: 3-5
- Zones: Factory District (Outskirts)
- Brief: sabotage 3 conveyor lines, then defeat Foreman Clank in the arena.
- Setpieces: `setpiece.q7.line_shutdown_1..3`, `setpiece.q7.foreman_clank_arena`
- Protoadventure: `protoadventures/q7-conveyor-war.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold tight corridors under turret pressure
- [ ] rogue: bypass switch locks; disable a line quietly
- [ ] cleric: mitigate burns/bleeds from weldspray hazards
- [ ] wizard: solve LOS fights with control; counter turrets
- [ ] ranger: pick off belt-runner drones; spot ambush angles
- [ ] paladin: "protect workers" choice vs pure sabotage
- [ ] bard: rally in the switchroom; coordinate shutdown order
- [ ] druid: jam gears with vines/roots flavor (environment tool)
- [ ] barbarian: rip a jammed lever free under fire
- [ ] warlock: overclock a shutdown for speed at a cost
- [ ] sorcerer: burst a line’s add wave before it snowballs
- [ ] monk: conveyor traversal without taking knockback deaths
- Party runs:
- [x] run-01: `protoadventures/party_runs/q7-conveyor-war/run-01.md`
- [x] run-02: `protoadventures/party_runs/q7-conveyor-war/run-02.md`
- [x] run-03: `protoadventures/party_runs/q7-conveyor-war/run-03.md`
- [x] run-04: `protoadventures/party_runs/q7-conveyor-war/run-04.md`
- [x] run-05: `protoadventures/party_runs/q7-conveyor-war/run-05.md`
- [x] run-06: `protoadventures/party_runs/q7-conveyor-war/run-06.md`
- [x] run-07: `protoadventures/party_runs/q7-conveyor-war/run-07.md`
- [x] run-08: `protoadventures/party_runs/q7-conveyor-war/run-08.md`
- [x] run-09: `protoadventures/party_runs/q7-conveyor-war/run-09.md`
- [x] run-10: `protoadventures/party_runs/q7-conveyor-war/run-10.md`

---

## 08) `q8-flow-treaty`

- Level band: 8-11; party: 3-5
- Zones: Reservoir
- Brief: restore 3 control nodes, then defeat The Flow Regulator.
- Setpieces: `setpiece.q8.control_node_1..3`, `setpiece.q8.flow_regulator_chamber`
- Protoadventure: `protoadventures/q8-flow-treaty.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold shoreline pulls under ranged pressure
- [ ] rogue: sneak through tunnels to reach a control node first
- [ ] cleric: cleanse waterborne debuffs; keep team stable
- [ ] wizard: manage wide pulls; interrupt regulator phases
- [ ] ranger: long-range "kite" threat packs on shores
- [ ] paladin: negotiate treaty terms (rep flavor)
- [ ] bard: coordinate node repair order; prevent backtracking
- [ ] druid: water/nature interaction: calm sump eels (optional)
- [ ] barbarian: force a stuck pump to turn (objective under fire)
- [ ] warlock: accept a "power draw" bargain (optional difficulty)
- [ ] sorcerer: burst phase adds in regulator fight
- [ ] monk: traversal through ambush tunnels without getting pinned
- Party runs:
- [x] run-01: `protoadventures/party_runs/q8-flow-treaty/run-01.md`
- [x] run-02: `protoadventures/party_runs/q8-flow-treaty/run-02.md`
- [x] run-03: `protoadventures/party_runs/q8-flow-treaty/run-03.md`
- [x] run-04: `protoadventures/party_runs/q8-flow-treaty/run-04.md`
- [x] run-05: `protoadventures/party_runs/q8-flow-treaty/run-05.md`
- [x] run-06: `protoadventures/party_runs/q8-flow-treaty/run-06.md`
- [x] run-07: `protoadventures/party_runs/q8-flow-treaty/run-07.md`
- [x] run-08: `protoadventures/party_runs/q8-flow-treaty/run-08.md`
- [x] run-09: `protoadventures/party_runs/q8-flow-treaty/run-09.md`
- [x] run-10: `protoadventures/party_runs/q8-flow-treaty/run-10.md`

---

## 09) `q9-beacon-protocol`

- Level band: 9-12; party: 3-5
- Zones: The Underrail
- Brief: light 4 beacons through tunnels, then defeat the Platform Warden.
- Setpieces: `setpiece.q9.beacon_1..4`, `setpiece.q9.platform_warden_arena`
- Protoadventure: `protoadventures/q9-beacon-protocol.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold platforms; prevent backline deletes
- [ ] rogue: scout switchyard patrol timing
- [ ] cleric: manage attrition over long tunnels
- [ ] wizard: solve beacon defense waves with control
- [ ] ranger: pick off signal wights; read sign glyphs
- [ ] paladin: protect civilians trapped on a platform (optional)
- [ ] bard: keep group coordinated in long traversal; prevent splits
- [ ] druid: navigate fungus/underground ecology hazards
- [ ] barbarian: force open a sealed maintenance gate (shortcut)
- [ ] warlock: bind a beacon with a risky pact for speed
- [ ] sorcerer: burst warden add phase
- [ ] monk: switchyard timing puzzles; fast movement
- Party runs:
- [x] run-01: `protoadventures/party_runs/q9-beacon-protocol/run-01.md`
- [x] run-02: `protoadventures/party_runs/q9-beacon-protocol/run-02.md`
- [x] run-03: `protoadventures/party_runs/q9-beacon-protocol/run-03.md`
- [x] run-04: `protoadventures/party_runs/q9-beacon-protocol/run-04.md`
- [x] run-05: `protoadventures/party_runs/q9-beacon-protocol/run-05.md`
- [x] run-06: `protoadventures/party_runs/q9-beacon-protocol/run-06.md`
- [x] run-07: `protoadventures/party_runs/q9-beacon-protocol/run-07.md`
- [x] run-08: `protoadventures/party_runs/q9-beacon-protocol/run-08.md`
- [x] run-09: `protoadventures/party_runs/q9-beacon-protocol/run-09.md`
- [x] run-10: `protoadventures/party_runs/q9-beacon-protocol/run-10.md`

---

## 10) `q10-anchor-the-wind`

- Level band: 11-13; party: 3-5
- Zones: Skybridge
- Brief: repair 3 anchors across spans, then defeat the Wind Marshal.
- Setpieces: `setpiece.q10.anchor_1..3`, `setpiece.q10.wind_marshal_span`
- Protoadventure: `protoadventures/q10-anchor-the-wind.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold bridgehead while anchors are repaired
- [ ] rogue: traverse side-scaffolds to bypass dangerous spans
- [ ] cleric: stabilize knockback/vertigo debuffs
- [ ] wizard: control winds; solve traversal with utility
- [ ] ranger: long sightlines: counter snipers
- [ ] paladin: protect travelers; choose rescue vs speed
- [ ] bard: call cadence for movement and timing
- [ ] druid: interact with air currents / birds / storms
- [ ] barbarian: brute-force a stuck anchor crank mid-fight
- [ ] warlock: accept a wind pact to cross faster (risk)
- [ ] sorcerer: burst air-element adds (or drones) fast
- [ ] monk: movement mastery; dodge knockbacks
- Party runs:
- [x] run-01: `protoadventures/party_runs/q10-anchor-the-wind/run-01.md`
- [x] run-02: `protoadventures/party_runs/q10-anchor-the-wind/run-02.md`
- [x] run-03: `protoadventures/party_runs/q10-anchor-the-wind/run-03.md`
- [x] run-04: `protoadventures/party_runs/q10-anchor-the-wind/run-04.md`
- [x] run-05: `protoadventures/party_runs/q10-anchor-the-wind/run-05.md`
- [x] run-06: `protoadventures/party_runs/q10-anchor-the-wind/run-06.md`
- [x] run-07: `protoadventures/party_runs/q10-anchor-the-wind/run-07.md`
- [x] run-08: `protoadventures/party_runs/q10-anchor-the-wind/run-08.md`
- [x] run-09: `protoadventures/party_runs/q10-anchor-the-wind/run-09.md`
- [x] run-10: `protoadventures/party_runs/q10-anchor-the-wind/run-10.md`

---

## 11) `q11-stormglass-route`

- Level band: 12-15; party: 3-5
- Zones: Glass Wastes
- Brief: survive dune traversal, locate storm eye, and complete the storm setpiece.
- Setpieces: `setpiece.q11.storm_eye_event`
- Protoadventure: `protoadventures/q11-stormglass-route.md` (exists)
- Spotlights (sketch):
- [ ] fighter: protect caravan line through ranged pressure
- [ ] rogue: scout safe dunes; find hidden shelter
- [ ] cleric: manage attrition and exhaustion-like effects
- [ ] wizard: weather control / navigation magic flavor
- [ ] ranger: survival navigation and route-finding
- [ ] paladin: "carry the weak" moral beat under pressure
- [ ] bard: keep morale; coordinate shelter rotations
- [ ] druid: interact with storms and desert life
- [ ] barbarian: brute-force through a glassfield hazard gate
- [ ] warlock: accept a storm boon for power at a cost
- [ ] sorcerer: burst through storm adds quickly
- [ ] monk: dodge hazard zones; fast traversal
- Party runs:
- [x] run-01: `protoadventures/party_runs/q11-stormglass-route/run-01.md`
- [x] run-02: `protoadventures/party_runs/q11-stormglass-route/run-02.md`
- [x] run-03: `protoadventures/party_runs/q11-stormglass-route/run-03.md`
- [x] run-04: `protoadventures/party_runs/q11-stormglass-route/run-04.md`
- [x] run-05: `protoadventures/party_runs/q11-stormglass-route/run-05.md`
- [x] run-06: `protoadventures/party_runs/q11-stormglass-route/run-06.md`
- [x] run-07: `protoadventures/party_runs/q11-stormglass-route/run-07.md`
- [x] run-08: `protoadventures/party_runs/q11-stormglass-route/run-08.md`
- [x] run-09: `protoadventures/party_runs/q11-stormglass-route/run-09.md`
- [x] run-10: `protoadventures/party_runs/q11-stormglass-route/run-10.md`

---

## 12) `q12-bloom-contract`

- Level band: 13-16; party: 3-5
- Zones: Crater Gardens
- Brief: collect 3 bloom samples, then face a caretaker drone + root engine threat.
- Setpieces: `setpiece.q12.bloom_grove_1..3`
- Protoadventure: `protoadventures/q12-bloom-contract.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold grove perimeter under swarm pressure
- [ ] rogue: infiltrate greenhouse tunnels to access a grove
- [ ] cleric: cleanse spores and swarms (status management)
- [ ] wizard: solve grove geometry with control and AoE
- [ ] ranger: track rare spawns; harvest safely
- [ ] paladin: protect the biome (choice: burn vs preserve)
- [ ] bard: negotiate with greenhouse staff; better contracts
- [ ] druid: the big druid moment: speak to the garden machine
- [ ] barbarian: smash open a seed-vault (risk/reward)
- [ ] warlock: accept a bio-pact for mutation-like power (optional)
- [ ] sorcerer: burst down caretaker heal cycles
- [ ] monk: movement in vine-choked terrain
- Party runs:
- [x] run-01: `protoadventures/party_runs/q12-bloom-contract/run-01.md`
- [x] run-02: `protoadventures/party_runs/q12-bloom-contract/run-02.md`
- [x] run-03: `protoadventures/party_runs/q12-bloom-contract/run-03.md`
- [x] run-04: `protoadventures/party_runs/q12-bloom-contract/run-04.md`
- [x] run-05: `protoadventures/party_runs/q12-bloom-contract/run-05.md`
- [x] run-06: `protoadventures/party_runs/q12-bloom-contract/run-06.md`
- [x] run-07: `protoadventures/party_runs/q12-bloom-contract/run-07.md`
- [x] run-08: `protoadventures/party_runs/q12-bloom-contract/run-08.md`
- [x] run-09: `protoadventures/party_runs/q12-bloom-contract/run-09.md`
- [x] run-10: `protoadventures/party_runs/q12-bloom-contract/run-10.md`

---

## 13) `q13-heart-of-the-machine`

- Level band: 15-17; party: 3-5
- Zones: The Core
- Brief: multi-stage "raid-lite" run through gate ring, then a machine-heart boss.
- Protoadventure: `protoadventures/q13-heart-of-the-machine.md` (exists)
- Spotlights (sketch):
- [ ] fighter: tank swap / hold center under multi-stage pressure
- [ ] rogue: disable a heart lock via precision sequence
- [ ] cleric: manage sustained incoming damage and recoveries
- [ ] wizard: solve puzzle mechanics; control add swarms
- [ ] ranger: target priority calls on high-threat adds
- [ ] paladin: vow beat; "we do not abandon the run"
- [ ] bard: cadence/coordination is the raid win condition
- [ ] druid: interface with the core’s biosystems
- [ ] barbarian: break a shield phase with raw damage (timed window)
- [ ] warlock: accept a forbidden core shard for power (optional)
- [ ] sorcerer: burst phase transitions
- [ ] monk: precision movement in lethal geometry
- Party runs:
- [x] run-01: `protoadventures/party_runs/q13-heart-of-the-machine/run-01.md`
- [x] run-02: `protoadventures/party_runs/q13-heart-of-the-machine/run-02.md`
- [x] run-03: `protoadventures/party_runs/q13-heart-of-the-machine/run-03.md`
- [x] run-04: `protoadventures/party_runs/q13-heart-of-the-machine/run-04.md`
- [x] run-05: `protoadventures/party_runs/q13-heart-of-the-machine/run-05.md`
- [x] run-06: `protoadventures/party_runs/q13-heart-of-the-machine/run-06.md`
- [x] run-07: `protoadventures/party_runs/q13-heart-of-the-machine/run-07.md`
- [x] run-08: `protoadventures/party_runs/q13-heart-of-the-machine/run-08.md`
- [x] run-09: `protoadventures/party_runs/q13-heart-of-the-machine/run-09.md`
- [x] run-10: `protoadventures/party_runs/q13-heart-of-the-machine/run-10.md`

---

## 14) `q14-stabilize-the-boundary`

- Level band: 17-20; party: 3-5
- Zones: The Seam
- Brief: stabilize 3 boundary nodes, then confront a reality-glitch boss.
- Protoadventure: `protoadventures/q14-stabilize-the-boundary.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold the line when rules change mid-fight
- [ ] rogue: exploit glitch "blind spots" to reach nodes
- [ ] cleric: mitigate chaos debuffs; keep party sane
- [ ] wizard: solve rule-bending mechanics; counter-phase shifts
- [ ] ranger: track unstable paths; guide through anomalies
- [ ] paladin: anchor the party’s purpose against corruption
- [ ] bard: stabilize morale; keep comms coherent in chaos
- [ ] druid: restore reality around living things (biome anchor)
- [ ] barbarian: brute-force a collapsing node window
- [ ] warlock: temptations are explicit; refuse or accept with cost
- [ ] sorcerer: wild-magic style surges as a theme beat
- [ ] monk: movement mastery in shifting geometry
- Party runs:
- [x] run-01: `protoadventures/party_runs/q14-stabilize-the-boundary/run-01.md`
- [x] run-02: `protoadventures/party_runs/q14-stabilize-the-boundary/run-02.md`
- [x] run-03: `protoadventures/party_runs/q14-stabilize-the-boundary/run-03.md`
- [x] run-04: `protoadventures/party_runs/q14-stabilize-the-boundary/run-04.md`
- [x] run-05: `protoadventures/party_runs/q14-stabilize-the-boundary/run-05.md`
- [x] run-06: `protoadventures/party_runs/q14-stabilize-the-boundary/run-06.md`
- [x] run-07: `protoadventures/party_runs/q14-stabilize-the-boundary/run-07.md`
- [x] run-08: `protoadventures/party_runs/q14-stabilize-the-boundary/run-08.md`
- [x] run-09: `protoadventures/party_runs/q14-stabilize-the-boundary/run-09.md`
- [x] run-10: `protoadventures/party_runs/q14-stabilize-the-boundary/run-10.md`

---

## 15) `side-alley-fence`

- Level band: 2-5; party: 1-5
- Zones: Town Alleys (black market)
- Brief: find the fence, do a clean favor, unlock buy/sell of odd loot.
- Protoadventure: `protoadventures/side-alley-fence.md` (exists)
- Spotlights (sketch):
- [ ] fighter: intimidate a gang without violence (or with controlled violence)
- [ ] rogue: stealth route through alleys; pick a lock
- [ ] cleric: stop a bad deal; protect a victim
- [ ] wizard: solve a warded door; dispel trap
- [ ] ranger: tail a courier; track footsteps
- [ ] paladin: morality test: profit vs justice
- [ ] bard: negotiation mini-game; better prices
- [ ] druid: reveal hidden path via roots/growth
- [ ] barbarian: smash a stash wall (loud option)
- [ ] warlock: accept an "unmarked favor" (future hook)
- [ ] sorcerer: chaos in a crowded alley; control collateral
- [ ] monk: rooftop traversal shortcut
- Party runs:
- [x] run-01: `protoadventures/party_runs/side-alley-fence/run-01.md`
- [x] run-02: `protoadventures/party_runs/side-alley-fence/run-02.md`
- [x] run-03: `protoadventures/party_runs/side-alley-fence/run-03.md`
- [x] run-04: `protoadventures/party_runs/side-alley-fence/run-04.md`
- [x] run-05: `protoadventures/party_runs/side-alley-fence/run-05.md`
- [x] run-06: `protoadventures/party_runs/side-alley-fence/run-06.md`
- [x] run-07: `protoadventures/party_runs/side-alley-fence/run-07.md`
- [x] run-08: `protoadventures/party_runs/side-alley-fence/run-08.md`
- [x] run-09: `protoadventures/party_runs/side-alley-fence/run-09.md`
- [x] run-10: `protoadventures/party_runs/side-alley-fence/run-10.md`

---

## 16) `side-sewer-drone-rescue`

- Level band: 3-5; party: 1-5
- Zones: Sewers (optional wing)
- Brief: rescue a maintenance drone and escort it to the junction; unlock a utility vendor.
- Protoadventure: `protoadventures/side-sewer-drone-rescue.md` (exists)
- Spotlights (sketch):
- [ ] fighter: protect the escort through narrow corridors
- [ ] rogue: scout ahead; disarm a trap
- [ ] cleric: keep escort alive through status hazards
- [ ] wizard: lock down ambush rooms while escort moves
- [ ] ranger: route selection to avoid worst packs
- [ ] paladin: "leave no one behind" beat
- [ ] bard: calm the drone; prevent panic behavior
- [ ] druid: treat sewer fauna nonlethally (optional)
- [ ] barbarian: carry the drone through a flooded section (brute)
- [ ] warlock: bargain for a shortcut keycard (risk)
- [ ] sorcerer: burst ambushes quickly to protect escort
- [ ] monk: run ahead and clear a valve under time pressure
- Party runs:
- [x] run-01: `protoadventures/party_runs/side-sewer-drone-rescue/run-01.md`
- [x] run-02: `protoadventures/party_runs/side-sewer-drone-rescue/run-02.md`
- [x] run-03: `protoadventures/party_runs/side-sewer-drone-rescue/run-03.md`
- [x] run-04: `protoadventures/party_runs/side-sewer-drone-rescue/run-04.md`
- [x] run-05: `protoadventures/party_runs/side-sewer-drone-rescue/run-05.md`
- [x] run-06: `protoadventures/party_runs/side-sewer-drone-rescue/run-06.md`
- [x] run-07: `protoadventures/party_runs/side-sewer-drone-rescue/run-07.md`
- [x] run-08: `protoadventures/party_runs/side-sewer-drone-rescue/run-08.md`
- [x] run-09: `protoadventures/party_runs/side-sewer-drone-rescue/run-09.md`
- [x] run-10: `protoadventures/party_runs/side-sewer-drone-rescue/run-10.md`

---

## 17) `event-rail-runaway-cargo-bot`

- Level band: 5-8; party: 1-5
- Zones: Rail Spur (event node)
- Brief: stop a runaway cargo bot before it hits the terminal; time pressure + geometry.
- Protoadventure: `protoadventures/event-rail-runaway-cargo-bot.md` (exists)
- Spotlights (sketch):
- [ ] fighter: body-block the bot’s path; soak hits
- [ ] rogue: climb and pull an emergency lever (stealth route)
- [ ] cleric: keep party alive under time pressure
- [ ] wizard: immobilize/slow the bot; solve geometry
- [ ] ranger: disable it at range; target weak points
- [ ] paladin: choose rescue of bystanders vs fastest stop
- [ ] bard: coordinate timing; call shots
- [ ] druid: entangle the path; interact with trackside life
- [ ] barbarian: wrestle the bot and force it to stop (hero moment)
- [ ] warlock: overcharge the brake system (cost)
- [ ] sorcerer: burst weak points quickly
- [ ] monk: sprint traversal; dodge moving hazards
- Party runs:
- [x] run-01: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-01.md`
- [x] run-02: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-02.md`
- [x] run-03: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-03.md`
- [x] run-04: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-04.md`
- [x] run-05: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-05.md`
- [x] run-06: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-06.md`
- [x] run-07: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-07.md`
- [x] run-08: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-08.md`
- [x] run-09: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-09.md`
- [x] run-10: `protoadventures/party_runs/event-rail-runaway-cargo-bot/run-10.md`

---

## 18) `hunt-rustwood-stump-stalker`

- Level band: 5-9; party: 1-5
- Zones: Rustwood (hunt loop)
- Brief: hunt a stealth predator that relocates when damaged; learn patience and tracking.
- Protoadventure: `protoadventures/hunt-rustwood-stump-stalker.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold the line when it ambushes the backline
- [ ] rogue: track it via subtle clues; set an ambush
- [ ] cleric: prevent panic deaths from stealth bursts
- [ ] wizard: reveal/mark the target; control its escape route
- [ ] ranger: signature ranger moment: track + trap + finish
- [ ] paladin: protect weaker travelers in the forest
- [ ] bard: lure it out with sound; coordinate bait
- [ ] druid: speak to rustwood life; learn its patterns
- [ ] barbarian: rage moment: finish it in melee when it tries to flee
- [ ] warlock: accept a "mark" to see it (risk)
- [ ] sorcerer: burst when it reveals itself
- [ ] monk: pursuit and positioning through trails
- Party runs:
- [x] run-01: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-01.md`
- [x] run-02: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-02.md`
- [x] run-03: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-03.md`
- [x] run-04: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-04.md`
- [x] run-05: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-05.md`
- [x] run-06: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-06.md`
- [x] run-07: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-07.md`
- [x] run-08: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-08.md`
- [x] run-09: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-09.md`
- [x] run-10: `protoadventures/party_runs/hunt-rustwood-stump-stalker/run-10.md`

---

## 19) `event-quarry-night-shift`

- Level band: 4-8; party: 1-5
- Zones: Quarry (timed event)
- Brief: a night shift event changes spawns and rewards; high risk, high ore.
- Protoadventure: `protoadventures/event-quarry-night-shift.md` (exists)
- Spotlights (sketch):
- [ ] fighter: hold against heavier hits in low visibility
- [ ] rogue: steal a rare ore cache during chaos
- [ ] cleric: sustain through longer attrition pulls
- [ ] wizard: illuminate/control battlefield; counter ambushes
- [ ] ranger: track event elites; avoid patrols
- [ ] paladin: protect workers; choose to evacuate or push
- [ ] bard: coordinate shift objectives; manage morale
- [ ] druid: interact with quarry fauna and night hazards
- [ ] barbarian: smash through a collapsed tunnel for a shortcut
- [ ] warlock: accept a cursed ore vein for power (optional)
- [ ] sorcerer: burst event elite before it escapes
- [ ] monk: mobility in low-visibility hazard zones
- Party runs:
- [x] run-01: `protoadventures/party_runs/event-quarry-night-shift/run-01.md`
- [x] run-02: `protoadventures/party_runs/event-quarry-night-shift/run-02.md`
- [x] run-03: `protoadventures/party_runs/event-quarry-night-shift/run-03.md`
- [x] run-04: `protoadventures/party_runs/event-quarry-night-shift/run-04.md`
- [x] run-05: `protoadventures/party_runs/event-quarry-night-shift/run-05.md`
- [x] run-06: `protoadventures/party_runs/event-quarry-night-shift/run-06.md`
- [x] run-07: `protoadventures/party_runs/event-quarry-night-shift/run-07.md`
- [x] run-08: `protoadventures/party_runs/event-quarry-night-shift/run-08.md`
- [x] run-09: `protoadventures/party_runs/event-quarry-night-shift/run-09.md`
- [x] run-10: `protoadventures/party_runs/event-quarry-night-shift/run-10.md`

---

## 20) `side-library-misfiled-wing`

- Level band: 6-10; party: 1-5
- Zones: Sunken Library (optional wing)
- Brief: a misfiled wing contains a rare plate fragment and a lore key; navigation + debuffs.
- Protoadventure: `protoadventures/side-library-misfiled-wing.md` (exists)
- Spotlights (sketch):
- [ ] fighter: protect team through tight corridors and debuffs
- [ ] rogue: bypass a flooded lock via alternate vent route
- [ ] cleric: cleanse mold debuffs; keep party functional
- [ ] wizard: solve a warded archive puzzle
- [ ] ranger: map the wing; find the real exit under pressure
- [ ] paladin: choose to destroy or preserve a dangerous archive
- [ ] bard: interpret archive hints; reduce wrong-turns
- [ ] druid: interact with mold sprites nonlethally
- [ ] barbarian: smash through a jammed shelf wall (loud shortcut)
- [ ] warlock: accept a forbidden index as a boon (optional)
- [ ] sorcerer: burst sentinel spawns to avoid wipe
- [ ] monk: traversal puzzle through collapsing stacks
- Party runs:
- [x] run-01: `protoadventures/party_runs/side-library-misfiled-wing/run-01.md`
- [x] run-02: `protoadventures/party_runs/side-library-misfiled-wing/run-02.md`
- [x] run-03: `protoadventures/party_runs/side-library-misfiled-wing/run-03.md`
- [x] run-04: `protoadventures/party_runs/side-library-misfiled-wing/run-04.md`
- [x] run-05: `protoadventures/party_runs/side-library-misfiled-wing/run-05.md`
- [x] run-06: `protoadventures/party_runs/side-library-misfiled-wing/run-06.md`
- [x] run-07: `protoadventures/party_runs/side-library-misfiled-wing/run-07.md`
- [x] run-08: `protoadventures/party_runs/side-library-misfiled-wing/run-08.md`
- [x] run-09: `protoadventures/party_runs/side-library-misfiled-wing/run-09.md`
- [x] run-10: `protoadventures/party_runs/side-library-misfiled-wing/run-10.md`
