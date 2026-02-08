---
adventure_id: q1-first-day-on-gaia
area_id: A01
zone: "Newbie School -> Town: Gaia Gate"
clusters:
  - CL_NS_ORIENTATION
  - CL_NS_DORMS
  - CL_NS_LABS
  - CL_NS_SIM_YARD
  - CL_TOWN_GATE_PLAZA
hubs:
  - HUB_NS_ORIENTATION
  - HUB_NS_LABS
  - HUB_TOWN_GATE
setpieces: []
level_band: [1, 2]
party_size: [1, 1]
expected_runtime_min: 20
---

# Q1: First Day On Gaia (L1-L2, Solo)

## Hook

You wake up in a clean facility that is trying very hard to feel normal. A badge is your right to exist here. A pass is your right to leave.

Inputs (design assumptions):

- New players need explicit reassurance that this is safe and that failure is expected.
- Tutorial steps should have explicit success acknowledgements ("drill complete") to reduce confusion and help automation.
- If we expect any secret content to be found, `search` must be taught explicitly.
- Movement needs a fast path for veterans (either `n/e/s/w` aliases or very clear `go <dir>` teaching).
- Kit choice should be framed as low-stakes (flavor now) to reduce decision paralysis.

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q1_first_day.state`: `orientation` -> `badge` -> `verbs` -> `combat` -> `town_pass` -> `complete`
- `q.q1_first_day.kit_choice`: `human` | `robot`

Success conditions:

- Badge issued
- Verbs drilled (3 tiny tasks)
- One combat scenario cleared
- Town pass accepted + starter kit chosen

## Room Flow

This is a mostly-linear chain with tiny detours for drills.

### R_NS_ORIENT_01 (CL_NS_ORIENTATION, HUB_NS_ORIENTATION)

- beat: sterile foyer, calm signage, soft ambient audio; NPCs can see you.
- teach: `look`, `say`, `help`
- note: one explicit reassurance line exists here (safe place to learn; failure is expected)
- note: a visible sign suggests `rules` for rules-first players
- exits: east -> `R_NS_ORIENT_02`

### R_NS_ORIENT_02 (badge desk)

- beat: Instructor Kline issues a badge after you type a name; Tutor-Unit R0 watches silently.
- teach: character naming UX, basic disclosure prompt (human/bot)
- quest: set `q.q1_first_day.state=badge`
- exits: north -> `R_NS_ORIENT_03`

### R_NS_ORIENT_03 (drill: hands)

- beat: a table with a training kit in a sealed bin.
- teach: `get`, `drop`
- quest: contributes to `q.q1_first_day.state=verbs` (drill 1/3)
- feedback: explicit "drill complete" acknowledgement after the player succeeds
- exits: west -> `R_NS_ORIENT_04`

### R_NS_ORIENT_04 (drill: wearables)

- beat: a mirror wall and an equipment rack.
- teach: `equip`, `remove`
- quest: drill 2/3
- feedback: explicit "drill complete" acknowledgement after the player succeeds
- exits: south -> `R_NS_DORMS_01`

### R_NS_DORMS_01 (CL_NS_DORMS)

- beat: dorm hall with other new arrivals; low-stakes social room.
- teach: `who`, `tell` (later), basic etiquette
- note: a small hint teaches `search` (for the optional supply closet)
- exits: east -> `R_NS_DORMS_02`

### R_NS_DORMS_02 (drill: doors)

- beat: three identical doors; one is "sticky" and makes you re-try movement.
- teach: `go <dir>`, re-orientation after failure
- quest: drill 3/3; set `q.q1_first_day.state=verbs`
- feedback: explicit "drill complete" acknowledgement after the player succeeds
- exits: north -> `R_NS_LABS_01`

### R_NS_LABS_01 (CL_NS_LABS, HUB_NS_LABS)

- beat: lab antechamber. R0 explains: "This is a simulation. Pain is a message."
- teach: basic combat loop briefing; how to retreat
- quest: set `q.q1_first_day.state=combat`
- note: a short death/recovery promise appears here or in med bay (no permanent loss in tutorial)
- exits: east -> `R_NS_LABS_02`

### R_NS_LABS_02 (prep bay)

- beat: lockers; a dispenser hands you a disposable heal item.
- teach: inventory, `use`
- exits: east -> `R_NS_LABS_03`

### R_NS_LABS_03 (scenario: training drone)

- beat: one weak drone; clear, readable telegraph; drops a "proof token".
- teach: attack, hit feedback, simple cooldown messaging
- feedback: provide simple, readable combat outcome cues (hit/miss; enemy looks hurt; "drone defeated")
- exits: east -> `R_NS_LABS_04`

### R_NS_LABS_04 (scenario: two pests)

- beat: two low-damage targets; teaches target switching.
- teach: managing multiple enemies; "don’t panic"
- exits: east -> `R_NS_LABS_05`

### R_NS_LABS_05 (scenario: hazard strip)

- beat: floor hazard that punishes standing still.
- teach: reposition; why movement matters
- telegraph: hazard cycle has readable messaging (so failure doesn't feel random)
- failure: failure messaging is explicit and suggests the next action (move, wait, try again)
- exits: north -> `R_NS_LABS_06`

### R_NS_LABS_06 (med bay)

- beat: tutorial "death" or "near-death" explanation; cheap reset.
- teach: recovery loop, revive, consequences without punishment
- quest: mark labs clear; set `q.q1_first_day.state=town_pass`
- exits: north -> `R_NS_SIMYARD_01`

### R_NS_SIMYARD_01 (CL_NS_SIM_YARD)

- beat: a small yard with a marked drill ring. Two NPC trainees are waiting. You run a short "pull-and-peel" drill.
- teach: aggro basics (what draws attention), how to disengage, and why spacing matters even in text.
- feedback: the instructor calls out what happened in plain language ("you pulled too much"; "you broke line"; "nice peel").
- exits: south -> `R_NS_LABS_06`, north -> `R_NS_EXIT_01`

### R_NS_EXIT_01 (corridor to town)

- beat: a long white corridor with a window: you can see the town gate plaza.
- teach: world feels bigger; one-way transition moment
- note: visible landmarking points at the job board square
- exits: north -> `R_TOWN_GATE_01`

### R_TOWN_GATE_01 (CL_TOWN_GATE_PLAZA, HUB_TOWN_GATE)

- beat: arrivals plaza; a kit kiosk; guards that look bored.
- teach: accept rules, pick kit
- quest: choose `q.q1_first_day.kit_choice`; set `q.q1_first_day.state=complete`
- note: kit choice explicitly frames itself as low-stakes (flavor now; can be revised later in early builds)
- exits: east -> `R_TOWN_GATE_02`

### R_TOWN_GATE_02 (starter kit kiosk)

- beat: choose "human-flavored" or "robot-flavored" kit; gives one cosmetic + one starter tool.
- teach: choices are flavor now; later affect contacts and quest text
- exits: south -> `R_TOWN_JOBBOARD_TEASER`

### R_TOWN_JOBBOARD_TEASER

- beat: you can see the job board square ahead; NPC barks "work available".
- teach: points to Q2 without forcing it
- note: this pointer should be impossible to miss
- exits: back -> `R_TOWN_GATE_02`, south -> (future) `HUB_TOWN_JOB_BOARD`

## NPCs

- Instructor Kline: calm, clipped. Makes the rules feel like safety rails.
- Tutor-Unit R0 ("Rho"): laconic. Explains systems, not lore.

## Rewards

- `gate.town_entry=1` (soft, after badge)
- `gate.town_services=1` (after pass)
- Starter kit choice stored.

## Implementation Notes

- This run should be safe and deterministic: no random deaths; no bot autofill.
- The only "secret" is a supply closet in Dorms (optional) that teaches `search`.

Learnings from party runs (`protoadventures/party_runs/q1-first-day-on-gaia/`):

- The sticky door must be explicitly messaged once, or it reads as broken movement.
- Hazard strip cadence should be forgiving for slower typists but still reward constant movement.
- Kit choice should have at least one immediate feedback line at the gate plaza (so it isn’t cosmetic-only).
- Unknown verbs should fail helpfully (suggest `help` or closest match), not silently.
- Consider adding one tiny optional environment vignette for druid/ranger curiosity (sim yard or "living system" beat).
- Spammy inputs should not flood output; keep invalid command feedback short and consistent.
- If we add direction aliases (`n/e/s/w`) in the engine, ensure the tutorial still teaches `go <dir>` explicitly.
- Over-leveled runs get bored in the drone room; allow a future skip (admin or explicit veteran flow).
