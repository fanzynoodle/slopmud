---
adventure_id: event-rail-runaway-cargo-bot
area_id: A01
zone: Rail Spur (Event Node)
clusters:
  - CL_RAIL_TERMINAL
  - CL_RAIL_YARDS
  - CL_RAIL_LINE
hubs: []
setpieces:
  - setpiece.e17.chase_start
  - setpiece.e17.lever_1
  - setpiece.e17.lever_2
  - setpiece.e17.brake_node_final
level_band: [5, 8]
party_size: [1, 5]
expected_runtime_min: 35
---

# Event: Rail Runaway Cargo Bot (Rail Spur) (L5-L8, Party 1-5)

## Hook

A cargo bot breaks loose on the spur. It is not "a mob" you kill. It is a moving disaster on a timer. Stop it before it hits the terminal buffer and turns people into a cleanup job.

This event is a tempo puzzle: levers, brake nodes, safe lanes, and a clear moral choice (rescue bystanders vs fastest stop).

Inputs (design assumptions):

- Chase UI must be loud and always visible (segments remaining, ETA, objective counter).
- Lever/brake node channels and counts must scale for solo/small parties.
- Track lane danger rules must be explicit and consistent; safe lanes must be clearly marked.
- If body-block exists, it must be an explicit interaction with clear duration and damage rules.
- Provide explicit non-combat rescue solutions with clear time cost so the moral choice is deliberate.
- Optional bargain systems (brake overcharge) are fun only if explicit, capped, and have paydown.

## Quest Line

Success conditions:

- stop the bot before it reaches the terminal

Optional outcomes:

- rescue trapped bystanders (time cost, better reward/rep)
- stop the bot early (clean run bonus)
- stop the bot cleanly (no terminal damage)

## Rules That Must Be Legible

### Chase UI (required)

The chase needs a loud UI banner:

- distance-to-terminal (or "segments remaining")
- bot speed state (slow / normal / fast)
- time remaining (or impact ETA)
- levers/brake nodes completed (e.g., 1/3)

Players should always know if they are winning or losing tempo.

### Track Lane Rule (required)

Standing on the track lane should be an explicit, consistent rule:

- warning text first, then knockback/damage if ignored (or immediate danger if we want it strict)
- safe lanes are clearly marked (side-walks, service planks, rooftop line)

### Body-Block (optional but must be explicit)

If body-blocking is a mechanic:

- it is a deliberate interaction (brace/hold), not physics jank
- it has clear duration and damage rules
- it works best at the final choke

### Optional Debt: Brake Overcharge (`brake_debt`, capped)

If a party opts in (warlock/console beat):

- `brake_debt` tokens: start at 0; cap at 2
- overcharge gives immediate tempo (slow bot for one segment, skip one hazard wave)
- debt consequence increases hazards later (more carts, tighter windows)
- paydown option at terminal staging (time cost or forfeit bonus)

Make it explicit and never a surprise.

----

## Room Flow

This event is linear with short alternative routes (rooftops/service lanes) that reward movement builds.

### R_RAIL_EVENT_01 (setpiece.e17.chase_start)

- beat: horn blares. Bot launches. UI banner appears.
- teach: "stop it with objectives, not kill count"
- exits: north -> `R_RAIL_SEG_01`

### R_RAIL_SEG_01 (trackside run)

- beat: first moving cart hazard. Establish safe lanes and "don’t stand on tracks."
- exits: north -> `R_RAIL_LEVER_01`

### R_RAIL_LEVER_01 (setpiece.e17.lever_1)

- beat: emergency lever. Short channel while adds pressure.
- teach: one solver channels, others hold; channel safe square is obvious
- quest: lever 1/2
- exits: north -> `R_RAIL_SEG_02`

### R_RAIL_SEG_02 (route split)

- beat: bystander crossing ahead; optional rescue visible.
- exits:
  - north -> `R_RAIL_RESCUE_01` (optional rescue)
  - east -> `R_RAIL_SERVICE_01` (service shortcut)
  - west -> `R_RAIL_ROOFS_01` (rooftop shortcut)

### R_RAIL_RESCUE_01 (optional)

- beat: trapped bystanders behind a stuck gate.
- objective: rescue quickly (interaction or social solve) for bonus.
- cost: consumes time; should be explicit in UI.
- exits: north -> `R_RAIL_SEG_03`

### R_RAIL_SEG_03 (rejoin after rescue)

- beat: you rejoin the chase lane. Hazard density is higher and the bot is audibly closer.
- teach: time pressure stays readable; "late" has a visible terminal damage state, not a surprise wipe
- exits: north -> `R_RAIL_LEVER_02`, back -> `R_RAIL_SEG_02`

### R_RAIL_SERVICE_01 (shortcut)

- beat: low-combat service planks; good for rogues/monks.
- exits: north -> `R_RAIL_LEVER_02`

### R_RAIL_ROOFS_01 (shortcut)

- beat: ladder chain + one risky gap. Failure drops you into patrol line (time/heat cost).
- exits: north -> `R_RAIL_LEVER_02`

### R_RAIL_LEVER_02 (setpiece.e17.lever_2)

- beat: second lever. Hazards increase.
- quest: lever 2/2
- exits: north -> `R_RAIL_TERMINAL_01`

### R_RAIL_TERMINAL_01 (terminal approach staging)

- beat: last short staging pocket. Optional: pay down `brake_debt` here.
- exits: north -> `R_RAIL_TERMINAL_02`

### R_RAIL_TERMINAL_02 (setpiece.e17.brake_node_final)

- beat: final brake node + choke. Bot arrives fast if you’re behind.
- objective: stop the bot (brake node + weak point window + optional body-block).
- teach: coordination under maximum time pressure.
- note: ram lane and weak point highlights must be readable at full sprint; communicate terminal damage state if late.
- exits: end -> `R_RAIL_REWARD_01`

### R_RAIL_REWARD_01 (event cash-out)

- beat: the terminal staff slaps an emergency stamp on your badge and hands over a bruised payout. The bot is chained down.
- rewards: shift payout and any rescue bonuses.
- exits: back -> `R_RAIL_EVENT_01`

## NPCs

- Terminal Staff: shouts timer updates; reinforces "objective, not kill count."
- Bystanders: make the rescue choice feel real.

## Rewards

- event reward cache
- optional bonus for rescues / early stop / low debt

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/event-rail-runaway-cargo-bot/`):

- Chase UI must be loud and always visible.
- Provide explicit non-combat rescue solutions and clear time cost.
- Solo/small parties need lever counts and channels scaled by party size.
- If body-block exists, it must be an explicit interaction with clear rules.
- Optional bargain systems (brake overcharge) are fun only if explicit, capped, and have paydown.
- Weak points must stay visible through lighting changes (outline/shimmer); speed lanes depend on it.
- Lever route signage must appear before the first hazard segment; don't teach it only after failure.
- Lever ambush needs deterministic spawns (tells + pattern) and scaling so low-DPS/support parties aren't trapped.
- Track hazards need consistent lane telegraphs; sprinting under timer should feel fair.
- Add an explicit rescue objective tracker (who/where/how many) and make the time cost obvious.
- Communicate terminal damage state immediately (success but degraded) and what it changes.
