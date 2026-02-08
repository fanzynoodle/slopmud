---
adventure_id: hunt-rustwood-stump-stalker
area_id: A01
zone: Rustwood (Hunt Loop)
clusters:
  - CL_RUSTWOOD_TRAILS
  - CL_RUSTWOOD_GROVES
hubs: []
setpieces:
  - setpiece.h18.trail_clues
  - setpiece.h18.first_ambush
  - setpiece.h18.stump_ring_finish
level_band: [5, 9]
party_size: [1, 5]
expected_runtime_min: 60
---

# Hunt: Rustwood Stump Stalker (Rustwood) (L5-L9, Party 1-5)

## Hook

Something in Rustwood is taking travelers and vanishing between the trees. It is not a straight brawl. The stump-stalker relocates when hurt. You win by tracking, patience, and a clean finish.

This is the hunt loop we want everywhere: clues are learnable, relocation is explicit, and the finish is a trap-and-corner setpiece.

Inputs (design assumptions):

- Clues must be consistent and learnable; avoid random trail RNG.
- Tracking UI must be loud (`trail_intensity`) so players always know if they are on the trail.
- Relocation must be loud and explicitly explained so it never reads like a despawn bug.
- Solo/small parties need safe wait pockets and deterministic trap/lure rules.
- The hunt must remain about tracking and positioning; healing should not trivialize it.
- Optional bargain systems are fun only if explicit, capped, and have paydown.

## Quest Line

Success conditions:

- locate the stump-stalker
- survive at least one ambush/reveal
- corner and defeat it at the stump ring

Optional outcomes:

- protect travelers during the hunt (escort beat)
- complete the hunt with low "trail loss" (clean tracking bonus)
- set a trap/lure for a clean finish (bonus)

## Rules That Must Be Legible

### Trail Intensity (tracking UI)

Define one simple UI meter (placeholder):

- `trail_intensity`: cold -> warm -> hot
- increases when you find the right clue chain
- decreases when you choose wrong branches or sprint blindly

Players should always know if they are "on the trail" or wasting time.

### Relocation Threshold (explicit)

When the stalker takes enough damage in a reveal window:

- it relocates to the next lair node
- a loud message fires ("It relocates. Follow the fresh sap trail.")
- the clue type updates (new direction, new tell)

Do not make relocation feel like a despawn bug.

### Safe Wait (camp nooks)

Provide explicit safe nooks:

- waiting is safe there (no escalation)
- used for solos and small parties to reset and set traps

### Optional Debt: Risky Mark (`hunt_mark_debt`, capped)

If a party opts in (warlock/console beat):

- `hunt_mark_debt` tokens: start at 0; cap at 2
- taking a mark grants reliable reveals for a short time
- debt consequence makes later ambush bursts harsher
- paydown at camp nooks (time or forfeit bonus)

Make it explicit and never a surprise.

----

## Room Flow

This loop is linear in logic, even if the physical map is a small graph:

1. pick up the trail
2. first reveal + relocation lesson
3. track to stump ring
4. trap-and-corner finish

### R_RUST_HUNT_START_01 (trailhead)

- beat: a posted notice + fresh drag marks into the woods.
- teach: "do not chase blindly"; tracking UI starts.
- exits: north -> `R_RUST_TRAIL_01`

### R_RUST_TRAIL_01 (setpiece.h18.trail_clues)

- beat: first clue node (scratches, sap smear, broken fern line).
- teach: clue grammar and `trail_intensity`
- exits: north -> `R_RUST_CLEARING_01`

### R_RUST_CLEARING_01 (setpiece.h18.first_ambush)

- beat: first reveal/ambush. Stalker bursts, then withdraws.
- teach: peel and stabilize; don’t tunnel; expect relocation.
- exits: north -> `R_RUST_TRAIL_02`

### R_RUST_TRAIL_02 (relocation follow-up)

- beat: fresh clue update points at the next lair direction.
- teach: relocation is explicit and trackable.
- exits: north -> `R_RUST_CAMP_01`

### R_RUST_CAMP_01 (safe nook)

- beat: camp nook. Safe wait. Trap placement tutorial.
- teach: waiting is safe here; set traps and plan.
- exits: north -> `R_RUST_STUMP_RING_01`

### R_RUST_STUMP_RING_01 (setpiece.h18.stump_ring_finish)

- beat: stump ring arena with clear choke points and trap squares.
- objective: corner the stalker. Prevent relocation by:
  - landing a mark, or
  - triggering a trap, or
  - completing an "anchor" interaction that locks the ring briefly.
- teach: finish window discipline; do not let it flee.
- exits: end -> `R_RUST_STUMP_REWARD_01`, back -> `R_RUST_CAMP_01`

### R_RUST_STUMP_REWARD_01 (hunt cash-out)

- beat: a hidden cache in the stump ring cracks open. The woods feel quieter.
- rewards: hunt cache reward and any clean-tracking bonuses.
- exits: south -> `R_RUST_CAMP_01`

## NPCs

- Hunt Clerk (optional): frames the rules and rewards patience.
- Travelers (optional): make the escort beat feel real.

## Rewards

- hunt cache reward
- optional bonus for low trail loss / traveler safety

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/hunt-rustwood-stump-stalker/`):

- Clues must be consistent and learnable; avoid random trail RNG.
- Provide a baseline clue marker visible to all parties; tracker classes can get extra detail.
- Relocation must be loud, explicitly explained, and tracked with progress (0/2, 1/2, 2/2).
- Add a strong pre-burst tell (audio/shimmer) and a readable "reveal window" so counterplay is fair.
- Add "bad chase" warnings before extra-pack routes; make punishments deterministic and signposted.
- Bound the relocation pool and communicate it; randomness should feel like variance, not roulette.
- Solo/small parties need safe wait pockets and deterministic trap/lure rules.
- Fast trail shortcuts should be teachable via consistent markers, not memorization-only secrets.
- Make clean kill/low trail loss criteria explicit and reward it loudly.
- The hunt must remain about tracking and positioning; healing should not trivialize it.
- Mark bargains must be explicit, capped, and bounded (room pools, duration) with a clear paydown/reset option.
