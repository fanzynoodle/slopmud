---
adventure_id: q5-sunken-index
area_id: A01
zone: Rustwood -> Sunken Library
clusters:
  - CL_RUSTWOOD_RANGER_CAMP
  - CL_RUSTWOOD_PYLONS
  - CL_LIB_ANTECHAMBER
  - CL_LIB_STACKS
  - CL_LIB_DECODER_CHAMBER
hubs:
  - HUB_RUSTWOOD_RANGER
  - HUB_LIB_ANTECHAMBER
setpieces:
  - setpiece.q5.plate_shrine_1
  - setpiece.q5.plate_shrine_2
  - setpiece.q5.plate_shrine_3
  - setpiece.q5.plate_shrine_4
  - setpiece.q5.decoder_chamber
level_band: [6, 8]
party_size: [3, 5]
expected_runtime_min: 45
---

# Q5: The Sunken Index (L6-L8, Party 3-5)

## Hook

The Library is a drowned machine for remembering. The ranger camp wants an "index key" that can open routes you cannot see yet. To get it, you collect four plates and decode a map.

Inputs (design assumptions):

- "Do not linger in water" must be a defined, readable mechanic with a safe reset at the dry threshold.
- Each shrine gimmick must have an explicit telegraph (no surprise pulses or pins).
- The choice room must communicate immediate meaning and later catch-up.
- Over-leveled parties should have an optional "overcharge" knob (risk/reward).
- Assist bots can fill parties to 3 total, but should not solve shrine objectives or make choices by default.
- The choice should have a confirm/vote step to prevent misclicks and griefing.
- Decoder overcharge should be explicitly optional with a modest reward.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q5_sunken_index.state`: `plates` -> `decode` -> `choose_unlock` -> `complete`
- `q.q5_sunken_index.plates`: `0..4`
- `q.q5_sunken_index.first_unlock`: `factory` | `reservoir`
- `gate.factory.entry`: `0/1`
- `gate.reservoir.entry`: `0/1`
- `q.q5_sunken_index.decoder_overcharge`: `0/1` (optional)

Success conditions:

- collect 4 plates from the stacks
- decode them in the decoder chamber
- choose first unlock (factory or reservoir)

## Room Flow

Linear spine: ranger camp -> library antechamber -> four plate shrines (branching but short) -> decoder -> choice -> return.

### R_RUST_CAMP_01 (CL_RUSTWOOD_RANGER_CAMP, HUB_RUSTWOOD_RANGER)

- beat: ranger camp with a chalk map and a warning: "Do not linger in water."
- teach: multi-zone runs; party check
- note: camp offers a short "what water does here" explanation (one paragraph, no rules dump)
- exits: east -> `R_RUST_PYLON_01`

### R_RUST_PYLON_01 (CL_RUSTWOOD_PYLONS)

- beat: a pylon clearing with long sight lines. A low hum makes it hard to track footsteps. Packs can see you early.
- teach: geometry matters; use landmarks; avoid chaining pulls in open space
- exits: west -> `R_RUST_CAMP_01`, east -> `R_RUST_PYLON_02`

### R_RUST_PYLON_02 (pylon overlook, CL_RUSTWOOD_PYLONS)

- beat: the pylon line points toward a stone stair that drops into the Library’s dry threshold.
- teach: route confirmation; make the transition between zones feel deliberate
- exits: west -> `R_RUST_PYLON_01`, east -> `R_LIB_ANTE_01`

### R_LIB_ANTE_01 (CL_LIB_ANTECHAMBER, HUB_LIB_ANTECHAMBER)

- beat: dry stone threshold; quest intake; a plate socket with four empty slots.
- quest: set `q.q5_sunken_index.state=plates`
- mechanic: "dry stone threshold" clears water exposure stacks and ends flood pulses; it does not reset shrine state
- feedback: after each plate, the socket visibly updates (1/4, 2/4, 3/4, 4/4)
- exits: east -> `R_LIB_STACKS_01`

### R_LIB_STACKS_01 (CL_LIB_STACKS)

- beat: stacked aisles; muffled movement; first "mold sprite" pack.
- teach: debuff awareness; don’t over-pull in tight rooms
- note: stacks room is the natural regroup node between shrine spokes
- exits:
  - north -> `R_LIB_PLATE_01`
  - east -> `R_LIB_PLATE_02`
  - south -> `R_LIB_PLATE_03`
  - west -> `R_LIB_PLATE_04`
  - decode -> `R_LIB_DECODER_01` (sealed until `plates==4`)
  - down -> `R_LIB_LORE_01` (optional secret)
  - up -> `R_LIB_LORE_02` (optional secret)

### R_LIB_LORE_01 (optional lore-only secret, CL_LIB_STACKS)

- beat: a half-collapsed reading nook above the waterline. A page of "index folklore" is pinned to stone.
- teach: curiosity reward without power creep
- reward: lore-only note that foreshadows the decoder and hints that the second gate will be unlocked later
- exits: up -> `R_LIB_STACKS_01`

### R_LIB_LORE_02 (optional lore-only secret, CL_LIB_STACKS)

- beat: a dry balcony with a broken index placard. The placard mentions "routes you cannot see yet".
- teach: lore reward without power creep
- reward: lore-only note that ties the index key to future travel tech
- exits: down -> `R_LIB_STACKS_01`

----

## Plate Shrine 1

### R_LIB_PLATE_01 (setpiece.q5.plate_shrine_1)

- beat: a shrine with a plate in clear view, but a flood pulse triggers when you grab it.
- teach: timed movement; "take it and go"
- quest: `plates += 1`
- telegraph: one explicit pre-warning before the grab triggers the first pulse
- exits: south -> `R_LIB_STACKS_01`

## Plate Shrine 2

### R_LIB_PLATE_02 (setpiece.q5.plate_shrine_2)

- beat: a plate behind a glass case; case opens after you "quiet" three sprites (mini objective).
- teach: focus targets; simple sub-objective
- quest: `plates += 1`
- telegraph: shrine shows three obvious "calm marks" that fill in as sprites are quieted
- note: provide one melee-friendly way to contribute to the objective (interaction or positioning), so "all-melee" comps are not punished
- exits: west -> `R_LIB_STACKS_01`

## Plate Shrine 3

### R_LIB_PLATE_03 (setpiece.q5.plate_shrine_3)

- beat: plate on a pedestal; a sentry appears and pins one player (force a rescue).
- teach: peel for teammates; priority targeting
- quest: `plates += 1`
- telegraph: sentry windup is obvious; first pin includes one short hint that it can be broken by teamwork
- note: allow minimal nonlethal framing in text (disable/power down), without changing mechanics
- exits: north -> `R_LIB_STACKS_01`

## Plate Shrine 4

### R_LIB_PLATE_04 (setpiece.q5.plate_shrine_4)

- beat: plate is real, but there are three decoys; grabbing a decoy spawns an extra pack.
- teach: risk management; reward careful play
- quest: `plates += 1`
- telegraph: decoys have readable tells; punishment scales with party size so it is survivable but sharp
- exits: east -> `R_LIB_STACKS_01`

----

## Decoder

When `plates == 4`, open an exit from `R_LIB_STACKS_01` to the decoder.

### R_LIB_DECODER_01 (setpiece.q5.decoder_chamber)

- beat: insert plates; decoder hums; the room "prints" a route map.
- teach: "content unlocks change the overworld"
- quest: set `q.q5_sunken_index.state=decode`
- optional: party can overcharge the decoder for a modest bonus by accepting extra pressure (set `decoder_overcharge=1`)
- optional: overcharge explicitly states reward and cost in one sentence each
- lore: one short line foreshadows future shard travel ("routes you cannot see yet")
- exits: west -> `R_LIB_CHOICE_01`

### R_LIB_CHOICE_01 (choice room)

- beat: two sealed doors: FACTORY and RESERVOIR. Choose one to unlock first.
- quest:
  - set `q.q5_sunken_index.first_unlock=factory|reservoir`
  - set `gate.factory.entry=1` OR `gate.reservoir.entry=1`
  - set `q.q5_sunken_index.state=complete`
- note: each door includes one sentence about immediate meaning (what it points to next), plus one sentence that catch-up will unlock the other later
- note: each door includes one optional moral flavor line (workers, water, stewardship) that does not change the mechanical choice
- note: selection requires explicit confirmation and should echo/log the decision as a story beat
- exits: west -> `R_LIB_ANTE_01`

## NPCs

- Ranger: practical; gives navigation hints ("count pylons", "follow the dry stone").
- Archive Sentry: communicates via debuffs and positioning pressure.

## Rewards

- first midgame district gate opened (factory or reservoir)
- "index key" as a persistent unlock token (future)

## Implementation Notes

- The plate shrines should be close to each other so the run is brisk.
- The choice should eventually allow catch-up (later quest makes both gates true).
- Make the "water exposure" mechanic consistent across shrines and stacks: readable feedback, safe reset at dry stone, and no hidden stacking.

Learnings from party runs (`protoadventures/party_runs/q5-sunken-index/`):

- Water exposure ("do not linger in water") must be a named, readable mechanic with a safe reset at the dry stone threshold.
- Each plate shrine needs an explicit telegraph for its gimmick (pulse, mini objective, pin/rescue, decoys) so it teaches rather than surprises.
- Tight aisles + choke rooms work well as natural recovery for overpull groups; make the regroup node explicit.
- Shrine state should persist across retreats and wipes; the dry threshold should be the clear reset anchor.
- Decoder overcharge should be explicitly optional with modest reward; safe to skip for stressed parties.
- Factory vs Reservoir choice must have an explicit short-term meaning and a confirm/vote step to prevent griefing.
