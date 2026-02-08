---
adventure_id: q6-hillfort-signal
area_id: A01
zone: Hillfort Ruins -> Rail Spur
clusters:
  - CL_HILLFORT_APPROACH
  - CL_HILLFORT_COMMAND_BUNKER
  - CL_HILLFORT_COURTYARDS
  - CL_HILLFORT_BANNER_HALL
  - CL_RAIL_TERMINAL
hubs:
  - HUB_HILLFORT_COMMAND
  - HUB_RAIL_TERMINAL
setpieces:
  - setpiece.q6.pylon_1
  - setpiece.q6.pylon_2
  - setpiece.q6.pylon_3
  - setpiece.q6.banner_hall
level_band: [5, 7]
party_size: [3, 5]
expected_runtime_min: 45
---

# Q6: Hillfort Signal (L5-L7, Party 3-5)

## Hook

The hillfort is an old defense system with a new population problem. The command bunker wants the pylons relit to stabilize patrol routes. The banner hall is where the problem nests.

Inputs (design assumptions):

- Parties should be able to fail the banner hall and try again without redoing pylons.
- Pylons should visibly change the world (courtyard signage, audio cues).
- The boss interrupt lesson should be teachable even if damage is high.
- The post-boss flow should clearly route: report in, then rail terminal.
- Assist bots should fill parties to 3 total at setpiece boundaries, but should be intentionally dumb by default (no interrupts unless explicitly commanded later).

## Quest Line

Target quest keys (from `docs/quest_state_model.md`):

- `q.q6_hillfort_signal.state`: `enter` -> `pylons` -> `boss` -> `resolved` -> `complete`
- `q.q6_hillfort_signal.pylons_lit`: `0..3`
- `gate.hillfort.safehouse`: `0/1`
- `gate.rail_spur.pass`: `0/1`
- `q.q6_hillfort_signal.add_pressure`: `0/1` (optional reducer; see "Banner Dampers")
- `q.q6_hillfort_signal.pylon_overcharge`: `0..3` (optional risk/reward)

Success conditions:

- light 3 pylons
- clear the banner hall boss setpiece
- report in, unlock safehouse
- claim rail pass at the terminal

## Room Flow

Linear spine with three short pylon spokes, then a boss, then a handoff to Rail Spur.

### R_HILL_APPROACH_01 (CL_HILLFORT_APPROACH)

- beat: broken road up to the hillfort. Patrol graffiti marks "SAFE" and "NOT SAFE" in big letters.
- teach: scouting and reading travel hints; this approach room should make it obvious you can retreat
- exits: east -> `R_HILL_CMD_01`

### R_HILL_CMD_01 (CL_HILLFORT_COMMAND_BUNKER, HUB_HILLFORT_COMMAND)

- beat: bunker entrance; tired patrol leader; a map with three pylon locations circled.
- teach: patrol timing intro; the idea of "safe travel nodes"
- quest: set `q.q6_hillfort_signal.state=enter`
- prop: a plain-text board listing pylon directions, rough route length (short/medium/long), and a checkbox-like "lit/unlit" status
- exits: west -> `R_HILL_APPROACH_01`, east -> `R_HILL_COURT_01`, overworld -> `R_RAIL_TERM_01` (handoff after boss)

### R_HILL_COURT_01 (CL_HILLFORT_COURTYARDS)

- beat: courtyard fork with three broken pylon lines. A cracked sign reads: "BANNER HALL SEALED UNTIL 3 SIGNALS RESTORED".
- quest: set `q.q6_hillfort_signal.state=pylons`
- feedback: after each pylon, the courtyard visibly changes (1 light lit, then 2, then 3; hum grows; sign updates)
- feedback: patrol timing cue exists (audible or text) before a patrol intersects the courtyard
- exits:
  - north -> `R_HILL_PYLON_01`
  - northwest -> `R_HILL_PYLON_02`
  - northeast -> `R_HILL_PYLON_03`
  - south -> `R_HILL_HALL_01` (sealed until 3 pylons; leads to `R_HILL_BANNER_02`)
  - down -> `R_HILL_CACHE_01` (optional secret; TODO: implement in area file)

### R_HILL_CACHE_01 (optional secret cache, CL_HILLFORT_COURTYARDS)

- beat: a narrow maintenance void under the courtyard. You can hear patrols overhead.
- teach: curiosity reward; low stakes resource planning
- reward: small consumable cache; one lore note that hints the boss has a "call"
- clue: one obvious but non-spoiler hint exists in the courtyard (draft breeze, scuff marks, or a loose grate that rattles)
- exits: up -> `R_HILL_COURT_01`

## Pylon 1

### R_HILL_PYLON_01 (setpiece.q6.pylon_1)

- beat: relight sequence: hold the point for 30s while patrols arrive in waves.
- teach: defend-a-point; positioning
- quest:
  - `pylons_lit += 1` (persist across wipes)
  - optional: if party chooses to "overcharge" after relight, set `pylon_overcharge += 1`
- feedback: explicit "signal restored" confirmation and a courtyard-visible change
- feedback: readable cadence (waves remaining, seconds remaining)
- exits: south -> `R_HILL_COURT_01`

## Pylon 2

### R_HILL_PYLON_02 (setpiece.q6.pylon_2)

- beat: pylon in a narrow approach; archers force you to advance.
- teach: push pressure; don’t stall
- quest:
  - `pylons_lit += 1` (persist across wipes)
  - optional: if party chooses to "overcharge" after relight, set `pylon_overcharge += 1`
- feedback: explicit "signal restored" confirmation and a courtyard-visible change
- feedback: readable cadence (waves remaining, seconds remaining)
- exits: east -> `R_HILL_COURT_01`

## Pylon 3

### R_HILL_PYLON_03 (setpiece.q6.pylon_3)

- beat: pylon in a collapsed section; falling debris hazards telegraph where not to stand.
- teach: read the room; react
- quest:
  - `pylons_lit += 1` (persist across wipes)
  - optional: if party chooses to "overcharge" after relight, set `pylon_overcharge += 1`
- feedback: explicit "signal restored" confirmation and a courtyard-visible change
- feedback: readable cadence (waves remaining, seconds remaining)
- feedback: biggest debris spike includes one extra warning line
- exits: west -> `R_HILL_COURT_01`

## Boss Gate

When `pylons_lit == 3`, unseal `R_HILL_HALL_01`.

### R_HILL_HALL_01 (banner hall approach)

- beat: long hall with hanging banners; audio cue: "breathing machine".
- exits: south -> `R_HILL_HALL_02`, north -> `R_HILL_COURT_01`

### R_HILL_HALL_02 (forced-interrupt tutorial, banner hall approach)

- beat: a small antechamber with a "signal horn" that emits a call. If left uninterrupted it spawns a single add.
- teach: interrupts are a team skill; do it fast, not perfectly
- telegraph: the call has a distinct cue (audible + one-line text) unlike normal attacks
- teach: assign an interrupt captain and a backup, even if the party is overgeared
- note: this should be low-stakes and repeatable (no wipe, no shame)
- exits: south -> `R_HILL_BANNER_02`, north -> `R_HILL_HALL_01`

### R_HILL_BANNER_02 (setpiece.q6.banner_hall)

- beat: boss + patrol adds. Boss has a "banner call" that summons adds unless interrupted.
- teach: interrupts as team skill; add control
- quest: set `q.q6_hillfort_signal.state=boss` then `resolved`
- terrain: add one or two simple room features (pillars, broken cover, hanging banner lines) to give positioning meaning
- hint: on the first call only, add one explicit hint line that it can be broken by fast disruption
- optional: "Banner Dampers" objective reduces add pressure: if the party destroyed/disabled 3 dampers in pylon rooms, set `add_pressure=1` and reduce add spawns
- optional: add minimal nonlethal framing for patrol units (disable/power down/drive off), without changing the core mechanics
- exits: north -> `R_HILL_HALL_02`

### Report In (back at HUB_HILLFORT_COMMAND)

- beat: report in; patrol leader marks a niche as a safehouse.
- beat: one line explains why the niche is safe (signal coverage, patrol pattern, or hardened door)
- quest:
  - set `gate.hillfort.safehouse=1`
  - set `q.q6_hillfort_signal.state=complete` (handoff)
- next: travel to Rail Spur via overworld portals:
  - `P_HILLFORT_CHECKPOINT` -> `P_CHECKPOINT_HILLFORT` -> `P_CHECKPOINT_RAIL` -> `P_RAIL_CHECKPOINT` -> `R_RAIL_TERM_01`

### R_RAIL_TERM_01 (CL_RAIL_TERMINAL, HUB_RAIL_TERMINAL)

- beat: terminal shed with a battered machine that prints a pass.
- quest: set `gate.rail_spur.pass=1`
- exits:
  - north -> `R_RAIL_TERMINAL_02` (teaser; requires pass)

### R_RAIL_TERMINAL_02 (rail teaser event)

- beat: one platform room with a locked timetable board and a short "world event" teaser that can only be watched, not solved yet.
- teach: passes unlock future routes and event nodes
- hint: timetable includes one sentence that implies future branching (which line, which district) even if still locked
- exits: south -> `R_RAIL_TERM_01`

## NPCs

- Patrol Leader: gives short, practical briefings.
- Terminal Attendant: jokes about "paper in a world of metal" and hands you a pass anyway.

## Rewards

- hillfort safehouse unlocked
- rail pass unlocked; world events can start showing up on rail nodes
- optional: pylon overcharge yields a modest bonus (currency, consumable, or cosmetic token)

## Implementation Notes

- This is the first "patrol timing" content. Keep patrols readable, not random.
- Pylon rooms are where we can later attach bot autofill boundaries (`setpiece.*`).
- Persist pylon progress (`pylons_lit`) even if the party wipes in the banner hall.
- Sealed exit message should be explicit and consistent (do not move the party).
- Timer content needs readable feedback (waves remaining, seconds remaining).

Learnings from party runs (`protoadventures/party_runs/q6-hillfort-signal/`):

- Pylon lit confirmation should be loud (audio + text + visible courtyard change) so progress feels real.
- Courtyard patrol timing needs a readable intersection cue (sound/text) so "wait, then move" becomes learnable.
- Banner hall gate messaging ("sealed until 3 signals") prevents sequence breaks; keep it explicit.
- Interrupt lesson needs one explicit first-time hint on the first summon/call, plus a UI concept of interrupt captain + backup.
- Pylon progress must persist across banner hall wipes; parties should be able to retry boss without relighting pylons.
- Secrets in hub clusters (cache void) work best when reconnection points are obvious and payoff is modest (consumables/lore).
