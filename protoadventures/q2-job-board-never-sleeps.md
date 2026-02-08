---
adventure_id: q2-job-board-never-sleeps
area_id: A01
zone: "Town: Gaia Gate (+ short hunting loops)"
clusters:
  - CL_TOWN_JOB_BOARD
  - CL_TOWN_MARKET_ROW
  - CL_TOWN_BANK_CLINIC
  - CL_TOWN_EDGE
  - CL_MEADOW_TRAILS
  - CL_MEADOW_PONDS
  - CL_MEADOW_PEST_FIELDS
  - CL_MEADOW_SEWER_GRATE
  - CL_ORCHARD_GROVE
  - CL_ORCHARD_ROOTWORKS
  - CL_ORCHARD_DRONE_NESTS
  - CL_ORCHARD_PIPEWAY
hubs:
  - HUB_TOWN_JOB_BOARD
setpieces: []
level_band: [1, 4]
party_size: [1, 3]
expected_runtime_min: 25
---

# Q2: The Job Board Never Sleeps (L1-L4, Party 1-3)

## Hook

The board is how the town metabolizes problems. You are a new metabolizer. Do three small things. Pick a contact. Get repeatables.

Inputs (design assumptions):

- The hub UI must make contract selection and progress obvious in text-only form.
- Turn-ins must provide consistent acknowledgement and a visible counter (`contracts_done=2/3`).
- Market Row must be clearly positioned as the sustain/prep loop (especially for no-healer parties).
- Orchard loop must teach cover/line-of-sight explicitly (it is the first ranged-pressure content).
- After the faction/contact choice, the game must explicitly point at what to do next (maintenance, quarry, rustwood).
- Repeatables unlock must include a clear "how to access" prompt (command or location).

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q2_job_board.state`: `contract_1` -> `contract_2` -> `contract_3` -> `choice` -> `repeatables` -> `complete`
- `q.q2_job_board.contracts_done`: `0..3`
- `q.q2_job_board.faction`: `civic` | `industrial` | `green`
- `q.q2_job_board.repeatables_unlocked`: `0/1`

Success conditions:

- complete 3 micro-contracts (each is a short, linear loop)
- choose a faction contact at the board
- unlock repeatables

## Room Flow

This protoadventure is written as three "mini-runs" that all start/end at the same hub.

### R_TOWN_JOB_01 (CL_TOWN_JOB_BOARD, HUB_TOWN_JOB_BOARD)

- beat: loud square, too many papers; NPCs announce work; a public ledger tracks completion.
- teach: quest intake, turn-in, the idea of repeatables
- feedback: contract selection uses stable labels (A/B/C) and progress is shown (`contracts_done=1/3`)
- exits:
  - east -> `R_TOWN_MARKET_01` (prep)
  - north -> `R_MEADOW_TRAIL_01` (contract A)
  - west -> `R_ORCHARD_TRAIL_01` (contract B)
  - south -> `R_TOWN_ALLEY_01` (contract C, town-only)

### R_TOWN_MARKET_01 (CL_TOWN_MARKET_ROW)

- beat: vendor row; buy bandages, sell junk; repair hook.
- teach: money sink loop; consumables matter
- note: the job board explicitly points parties here if they are low sustain (no healer)
- exits: west -> `R_TOWN_JOB_01`

----

## Contract A (Meadowline): "Pest Sweep"

Goal: remove a pest cluster that is blocking a marked trail segment.

### R_MEADOW_TRAIL_01 (signpost, CL_MEADOW_TRAILS)

- beat: a painted signpost marks "Pest Sweep" and points back to the square.
- teach: reading landmarks; returning to the hub
- exits: east -> `R_MEADOW_TRAIL_02`, south -> `R_TOWN_JOB_01`

### R_MEADOW_TRAIL_02 (open grass, CL_MEADOW_TRAILS)

- beat: open grass with clear sight lines; pests move in packs.
- teach: scanning the room; picking pulls
- exits: east -> `R_MEADOW_POND_01`, west -> `R_MEADOW_TRAIL_01`

### R_MEADOW_POND_01 (shallow pond, CL_MEADOW_PONDS)

- beat: shallow water and reeds; visibility is worse; pests can "skitter" from cover pockets.
- teach: hazards and terrain in text; slow zones should be clearly marked
- telegraph: always describe the waterline and footing ("ankle-deep", "slick stones")
- exits: east -> `R_MEADOW_PEST_01`, west -> `R_MEADOW_TRAIL_02`

### R_MEADOW_PEST_01 (pest field edge, CL_MEADOW_PEST_FIELDS)

- beat: the field edge is churned and loud; easy to overpull.
- teach: pack boundaries; "back up to reset"
- exits: east -> `R_MEADOW_PEST_02`, west -> `R_MEADOW_TRAIL_02`

### R_MEADOW_PEST_02 (pest knot)

- beat: 6-10 weak pests; clear them to spawn a "nest marker"
- teach: pulls, pacing, not over-aggroing
- quest: increment `contracts_done`; set `state=contract_1` (if first)
- exits: west -> `R_MEADOW_PEST_01`, north -> `R_MEADOW_GRATE_01`

### R_MEADOW_GRATE_01 (CL_MEADOW_SEWER_GRATE)

- beat: a maintenance grate rattles. You can hear deeper flow. A painted arrow points "SEWERS" with a warning mark.
- teach: world has layered routes; teaser gates without forcing
- reward: optional shortcut tease (later becomes a soft entry to Sewers once `gate.sewers.entry=1`)
- exits: south -> `R_MEADOW_PEST_02`, west -> `R_MEADOW_POND_01`

Turn-in: talk to the board clerk at `R_TOWN_JOB_01` to get a small reward (food, bandage, a token).

----

## Contract B (Scrap Orchard): "Drone Tag"

Goal: retrieve one stamped drone casing (teaches "loot one item from a pack").

### R_ORCHARD_TRAIL_01 (tree roots + half-buried metal, CL_ORCHARD_GROVE)

- beat: tree roots and half-buried metal; the path is narrow and readable.
- teach: short hunting loop; "go out, do task, return"
- exits: west -> `R_TOWN_JOB_01`, east -> `R_ORCHARD_TRAIL_02`

### R_ORCHARD_TRAIL_02 (tight roots, CL_ORCHARD_ROOTWORKS)

- beat: tight roots; cover pockets teach the idea of breaking line-of-sight.
- teach: cover/LOS basics (first ranged-pressure content)
- exits: east -> `R_ORCHARD_NEST_01`, west -> `R_ORCHARD_TRAIL_01`

### R_ORCHARD_NEST_01 (CL_ORCHARD_DRONE_NESTS)

- beat: ranged drone pack; drops 1 guaranteed "stamped casing"
- teach: ranged pressure; cover and line-of-sight; "break aggro" by rounding corners (later)
- quest: increment `contracts_done`; set `state=contract_2` (if second)
- exits: west -> `R_ORCHARD_TRAIL_02`, east -> `R_ORCHARD_PIPE_01`

### R_ORCHARD_PIPE_01 (CL_ORCHARD_PIPEWAY)

- beat: a pipeway hums under the roots; air smells like coolant. The path tilts down toward the town’s underworks.
- teach: "pipeway" is a distinct landmark; teaches that Orchard has a sewer-adjacent route
- note: later this becomes a soft route into Sewers (alternate to Town maintenance)
- exits: west -> `R_ORCHARD_NEST_01`

Turn-in: board clerk stamps your casing and adds you to the ledger.

----

## Contract C (Town): "Clinic + Gate Check"

Goal: deliver a sealed sample to the clinic, then carry a stamped clearance note to the edge gate.

### R_TOWN_ALLEY_01 (quiet lane, CL_TOWN_ALLEYS)

- beat: quiet lane; the delivery is marked with chalk arrows and "no soliciting" tags.
- teach: navigation beats in town; reading obvious signage
- exits: north -> `R_TOWN_ALLEY_02`, east -> `R_TOWN_JOB_01`

### R_TOWN_ALLEY_02 (service door, CL_TOWN_ALLEYS)

- beat: a locked-looking door that is actually just "push"
- teach: the world lies; try verbs anyway
- exits: north -> `R_TOWN_CLINIC_01`, south -> `R_TOWN_ALLEY_01`

### R_TOWN_CLINIC_01 (CL_TOWN_BANK_CLINIC)

- beat: clinic intake window; a medic scans the sealed sample and stamps a slip for the gate guards.
- teach: "services live in town"; cures/revive framing without lore dump
- exits: north -> `R_TOWN_EDGE_01`, south -> `R_TOWN_ALLEY_02`

### R_TOWN_EDGE_01 (CL_TOWN_EDGE)

- beat: outward gates and warning placards. A bored guard takes your stamped slip and points at the roads beyond.
- teach: the edge exists and is readable; blocked exits should be loud and explicit
- quest: increment `contracts_done`; set `state=contract_3` (if third)
- exits: south -> `R_TOWN_CLINIC_01`, west -> `R_TOWN_JOB_01`

----

## Choice: Pick A Contact

At `R_TOWN_JOB_01`, once `contracts_done=3`, present a choice:

- civic: maintenance office, order, valves
- industrial: quarry, ore, machines
- green: rustwood, library, navigation

Quest effects:

- set `q.q2_job_board.faction`
- set `q.q2_job_board.repeatables_unlocked=1`
- unlock gates:
  - civic/industrial: `gate.sewers.entry=1`
  - industrial/green: `gate.quarry.entry=1`

Next pointers (explicit, text-only):

- civic: points at the maintenance office (Q3 hook)
- industrial: points at the quarry foreman (Q4 hook)
- green: points at rustwood/library routes (Q5 hook)

## NPCs

- Board Clerk: neutral, efficient; refuses lore dumps; points to concrete places.
- Receiver Courier: "I don’t want your story, I want the package."

## Rewards

- repeatable contracts unlocked (daily caps later)
- first faction contact (flavor rewards)

## Implementation Notes

- Keep contracts short and clearly marked; no dead-end labyrinths.
- Respawns should be fast in the hunting rooms.

Learnings from party runs (`protoadventures/party_runs/q2-job-board-never-sleeps/`):

- Over-aggro should be readable and resettable (clear pack boundaries + "back up to reset" hint).
- Orchard drone rooms need an obvious LoS break and at least one melee-friendly cover pocket.
- Add an optional "first-timer suggested order" at the board (A -> B -> C) and a clearer repeatables explanation.
- Faction choice should grant one immediate tangible perk (small discount/token/hint), not only future gates.
- Optional contract clauses (fast/hard mode) are good replay knobs if explicit and tracked.
- Decide whether multiple contracts can be accepted simultaneously; if not, explicitly deny with a clear reason.
- Add consistent contract completion feedback for all three contracts (including delivery).
- Ensure orchard teaches cover/LOS explicitly (especially for all-melee and solo).
- Add a clear post-completion prompt that teaches how to access repeatables (command or location).
- Add a small courier rumor line that points to side content (alley fence) without spoilers.
