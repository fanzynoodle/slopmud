---
adventure_id: q4-quarry-rights
area_id: A01
zone: Quarry + Old Road Checkpoint
clusters:
  - CL_QUARRY_FOOTHILLS
  - CL_QUARRY_WORKS
  - CL_QUARRY_PITS
  - CL_QUARRY_ILLEGAL_DIG
  - CL_CHECKPOINT_GATEHOUSE
  - CL_CHECKPOINT_ROAD
  - CL_CHECKPOINT_AMBUSH_SPUR
hubs:
  - HUB_QUARRY_FOREMAN
  - HUB_CHECKPOINT_GATEHOUSE
setpieces:
  - setpiece.q4.illegal_dig_site
  - setpiece.q4.breaker7_arena
level_band: [4, 6]
party_size: [3, 5]
expected_runtime_min: 40
---

# Q4: Quarry Rights (L4-L6, Party 3-5)

## Hook

Someone is stealing from the quarry and blaming everyone else. The foreman wants the illegal dig cleared. A union rep wants proof it was "unauthorized". You can be loyal to a person, or to the work.

Inputs (design assumptions):

- Worm ambush needs a strong first-time telegraph, then can rely on subtle tells.
- Net-throwers must have explicit, teachable counterplay messaging.
- The side choice needs a short summary plus confirm/vote, and should be acknowledged later (gatehouse reacts).
- Trio / low-sustain parties need a clear recovery loop (restock node and/or bot fill to trio minimum).
- Over-leveled parties should have an optional hard mode knob with modest reward.
- Assist bots can fill parties to 3 total, but should not solve choice prompts or complex mechanics by default.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q4_quarry_rights.state`: `clear_dig` -> `choice` -> `elite` -> `complete`
- `q.q4_quarry_rights.side`: `union` | `foreman`
- `gate.checkpoint.access`: `0/1`
- `q.q4_quarry_rights.hard_mode`: `0/1` (optional)

Success conditions:

- clear the illegal dig setpiece
- make a side choice (union vs foreman)
- defeat Breaker-7 (named elite)
- unlock checkpoint access

## Room Flow

Linear with a single choice moment after the illegal dig.

### R_QUARRY_FOOTHILLS_01 (CL_QUARRY_FOOTHILLS)

- beat: rocky foothills with warning flags and a big painted arrow: "WORKS" one way, "SEWERS" the other.
- teach: entrances should be obvious; this is the connective tissue between zones
- exits: east -> `R_QUARRY_WORKS_01`

### R_QUARRY_WORKS_01 (CL_QUARRY_WORKS, HUB_QUARRY_FOREMAN)

- beat: foreman shack, clipboard tyranny, a vending crate of "ore masks".
- teach: interrupts are coming; buy one "stun break" consumable (future)
- note: this is an explicit restock node (cheap consumables, plus one hint about nets and worm tells)
- note: include one short line that retreating to restock is valid and does not erase illegal dig progress
- gate: entry via `gate.quarry.entry=1` (from Q2) OR via sewer shortcut
- exits: east -> `R_QUARRY_PITS_01`, west -> `R_QUARRY_FOOTHILLS_01`

### R_QUARRY_PITS_01 (CL_QUARRY_PITS)

- beat: open pits, heavy hits, sparse cover.
- teach: pull carefully, watch telegraphs
- exits: east -> `R_QUARRY_PITS_02`

### R_QUARRY_PITS_02 (CL_QUARRY_PITS)

- beat: worm-ambush tutorial. The ground has "tells" before the hit.
- teach: react to environment cues
- telegraph: the first ambush includes one explicit warning line before the hit; later occurrences can be subtle
- exits: east -> `R_QUARRY_DIG_01`

### R_QUARRY_DIG_01 (setpiece.q4.illegal_dig_site)

- beat: illegal dig with three crews. Clear in waves. Final wave drops a stamped permit shard.
- teach: fight in a wide room with flanks; prioritize the net-throwers
- teach: nets have explicit counterplay (break free, focus thrower, or ally assist), and the room messaging teaches it once
- feedback: readable wave boundaries ("wave 1/3", "wave 2/3", "final wave")
- quest: set `q.q4_quarry_rights.state=clear_dig`
- exits: north -> `R_QUARRY_CHOICE_01`

### R_QUARRY_CHOICE_01 (choice room)

- beat: union rep and foreman arrive at the same time and start talking past each other.
- teach: choose a side; choice affects rep flavor and reward text
- note: present a short "what changes" summary, then require explicit confirmation (or party vote)
- note: include a "neutral framing" dialogue option that still resolves to union/foreman (no third major branch)
- quest: set `q.q4_quarry_rights.side=union|foreman`
- quest: set `q.q4_quarry_rights.state=choice`
- exits: east -> `R_QUARRY_ELITE_01`

### R_QUARRY_ELITE_01 (Breaker-7 approach)

- beat: the tunnel narrows; you can hear a rhythmic impact.
- exits: east -> `R_QUARRY_ELITE_02`

### R_QUARRY_ELITE_02 (Breaker-7 arena)

- beat: Breaker-7 does a slow, huge slam (avoidable), and a faster shove (positioning check).
- teach: interrupts (or movement) as a core skill; heal under pressure
- optional: offer a "hard mode" start (set `hard_mode=1`) for a modest bonus reward
- optional: hard mode text states delta and reward in one sentence each (not just "hard mode enabled")
- telegraph: the shove has a clear cue and at least two counterplay options (reposition plus brace/disrupt), not a hidden check
- terrain: arena includes a clear "danger edge" hint so displacement is legible (fall hazard, crush zone, or similar)
- quest: set `q.q4_quarry_rights.state=elite`
- exits: north -> `R_CHECK_GATE_01`

### R_CHECK_GATE_01 (CL_CHECKPOINT_GATEHOUSE, HUB_CHECKPOINT_GATEHOUSE)

- beat: gatehouse interior. You hand over your permit shard and get a stamped pass.
- beat: gatehouse NPC acknowledges the chosen side (one line), so the choice feels real
- exits: include a sealed "midgame road teaser" northward (present but blocked) so the unlock feels immediate even before shipping the next zone
- quest: set `gate.checkpoint.access=1`, set `q.q4_quarry_rights.state=complete`
- exits:
  - south -> `R_QUARRY_WORKS_01`
  - east -> `R_CHECK_ROAD_01`
  - north -> (future) midgame road

### R_CHECK_ROAD_01 (CL_CHECKPOINT_ROAD)

- beat: the old road begins: cracked lines, distance markers, and a toll sign that now reads "PASS REQUIRED".
- teach: travel gates should be loud; routes should be named and consistent
- exits: west -> `R_CHECK_GATE_01`, north -> `R_CHECK_AMBUSH_01`

### R_CHECK_AMBUSH_01 (CL_CHECKPOINT_AMBUSH_SPUR)

- beat: a short spur with overturned signage. A small ambush pack tests whether you understand pulls and focus.
- teach: travel tutoring spike; "don’t fight on the road if you can reset into the gatehouse"
- exits: south -> `R_CHECK_ROAD_01`

## NPCs

- Quarry Foreman: wants order, hates surprises.
- Union Rep: wants dignity, hates exploitation.
- Breaker-7: says nothing; teaches by hitting.

## Rewards

- `gate.checkpoint.access=1`
- union side: `rep.civic += 1` (future)
- foreman side: `rep.industrial += 1` (future)

## Implementation Notes

- The choice should not lock players out permanently; it should mainly change rewards and next call-ins.
- Breaker-7 should be "learnable": deaths should feel like "I ignored the tell", not RNG.

Learnings from party runs (`protoadventures/party_runs/q4-quarry-rights/`):

- The restock node is the recovery loop: explicitly message that retreating is valid and that illegal dig progress persists.
- Worm ambush needs one extra-loud first-time telegraph, then can rely on subtler tells.
- Net-throwers need explicit counterplay messaging (break free, focus thrower, ally assist) so the dig setpiece teaches instead of panics.
- Illegal dig wave boundaries should be readable (1/3, 2/3, final) so parties time resources.
- Choice needs a short "what changes" summary and a confirm/vote step to prevent misclicks and griefing.
- Breaker-7 shove/displacement needs clear cue + clear arena edge language so it feels like geometry, not hidden checks.
