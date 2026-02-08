---
adventure_id: event-quarry-night-shift
area_id: A01
zone: Quarry (Timed Event)
clusters:
  - CL_QUARRY_PITS
  - CL_QUARRY_WORKS
  - CL_QUARRY_ILLEGAL_DIG
hubs: []
setpieces:
  - setpiece.e19.shift_start
  - setpiece.e19.worker_evac
  - setpiece.e19.rare_cache
  - setpiece.e19.night_elite
level_band: [4, 8]
party_size: [1, 5]
expected_runtime_min: 50
---

# Event: Quarry Night Shift (Quarry) (L4-L8, Party 1-5)

## Hook

When the night bell rings, the quarry becomes a different place. Visibility drops. Hits get heavier. Rare ore caches open briefly. Event elites prowl and try to escape with the good stuff.

This event is about choices under pressure: evacuate workers vs push deeper, take one cache vs risk a wipe, chase the elite vs secure the objective.

Inputs (design assumptions):

- Objective panel must be loud; players should always know what matters right now.
- Low visibility must remain readable (audio telegraphs, outlines, lamplit lanes, clear hazard silhouettes).
- Small/under-leveled parties need a guaranteed lamplit safe pocket.
- Time pressure must stay relevant even for high sustain parties (escalation and escape windows must be real).
- Optional bargain systems (cursed ore) are fun only if explicit, capped, and have paydown.

## Quest Line

Success conditions:

- survive until night shift ends
- complete at least 2 shift objectives (any two: evacuate workers, secure cache, defeat elite)

Optional outcomes:

- complete all 3 shift objectives (big bonus)
- keep patrol heat low (stealth/avoidance bonus)
- take cursed ore (explicit risk) and pay down debt (high-risk bonus)

## Rules That Must Be Legible

### Night Shift Panel (required)

Show a loud event panel:

- time remaining in shift
- objectives completed (0/2, 1/2, 2/2) and current optional bonuses
- current danger tier (spawns escalate as shift progresses)
- current patrol heat (spawns escalate when you are loud)

### Low Visibility (must be fair)

Night should not remove readability:

- elite attacks must have strong audio telegraphs
- key hazards have silhouettes/outlines
- safe lanes are clearly marked (lamps, reflective paint)
- lantern light must not hide telegraphs (contrast rule: "lit" always means "readable")

### Optional Debt: Cursed Ore (`cursed_ore_debt`, capped)

If a party opts in (warlock vein beat):

- `cursed_ore_debt` tokens: start at 0; cap at 2
- taking cursed ore grants immediate tempo or power
- debt consequence spawns extra elite pressure later
- paydown at shift end by forfeiting bonus or spending time

Make it explicit and never a surprise.

----

## Room Flow

This is a timed loop with three major beats:

1. shift start + objective selection
2. worker evacuations and cache windows
3. night elite chase and cash-out

### R_QUARRY_NIGHT_01 (setpiece.e19.shift_start)

- beat: bell rings; panel appears; spawns swap.
- teach: pick objectives; you cannot do everything
- exits:
  - north -> `R_QUARRY_WORKER_01`
  - east -> `R_QUARRY_CACHE_01`
  - west -> `R_QUARRY_CATWALK_01`
  - south -> `R_QUARRY_SHORTCUT_01`

### R_QUARRY_WORKER_01 (setpiece.e19.worker_evac)

- beat: workers move to lamplit shelters; patrols pressure them.
- teach: escort/hold under low visibility; safe lane markers matter
- exits: back -> `R_QUARRY_NIGHT_01`, east -> `R_QUARRY_SHELTER_01`

### R_QUARRY_CACHE_01 (setpiece.e19.rare_cache)

- beat: cache opens for a short window. Optional stealth path exists.
- teach: risk/reward; disengage is valid
- note: stealth path needs consistent markers and a clear reconnection point (no random maze)
- exits: back -> `R_QUARRY_NIGHT_01`, deeper -> `R_QUARRY_ELITE_01`

### R_QUARRY_SHORTCUT_01 (collapsed tunnel shortcut)

- beat: a collapsed tunnel connects objectives. Opening it is loud and fast.
- teach: loud shortcuts trade patrol heat for time; the cost must be explicit before you commit.
- exits: west -> `R_QUARRY_WORKER_01`, east -> `R_QUARRY_CACHE_01`, north -> `R_QUARRY_ELITE_01`

### R_QUARRY_CATWALK_01 (maintenance connector)

- beat: a maintenance catwalk/ladder chain that bypasses one patrol choke.
- teach: non-brute parties still get a chaining route (movement mastery, not just smash)
- exits: south -> `R_QUARRY_NIGHT_01`, west -> `R_QUARRY_WORKER_01`, east -> `R_QUARRY_CACHE_01`

### R_QUARRY_ELITE_01 (setpiece.e19.night_elite)

- beat: night elite spawns, tries to escape with ore.
- teach: burst before escape; do not chase into darkness blindly
- note: flee tell must be loud (audio + UI ping), and pursuit lanes need readable markers
- exits: back -> `R_QUARRY_NIGHT_01`, south -> `R_QUARRY_SHELTER_01`

### R_QUARRY_SHELTER_01 (lamplit safe pocket)

- beat: a lamplit shelter where waiting is explicitly safe (no escalation).
- teach: regroup and re-plan; small/under-leveled parties need this.
- exits: back -> `R_QUARRY_NIGHT_01`

## NPCs

- Night Foreman: shouts objective updates and time remaining.
- Workers: make evacuate vs push choice feel real.

## Rewards

- ore cache reward
- optional bonuses for worker safety and elite kill

## Implementation Notes

Learnings from party runs (`protoadventures/party_runs/event-quarry-night-shift/`):

- Low visibility must remain readable (audio + outlines + lamplit lanes).
- Objective panel must be loud; players should always know time left and objectives completed (0/2, 1/2, 2/2).
- Small/under-leveled parties need a guaranteed lamplit safe pocket.
- Patrol heat needs a visible meter and deterministic escalation; loud shortcuts should not feel random.
- Patrol boundaries need markers that read in low light (no "got clipped by nothing").
- Lantern/telegraph contrast rules matter: lighting must never hide the important silhouettes.
- Shadow/stealth routes need consistent markers and reconnection points (skill, not guessing).
- Non-brute parties need at least one chaining connector route (catwalk/ladder), not just smash shortcuts.
- Elite flee needs a loud tell and a pursuit lane marker so melee can follow fairly.
- Worker escort needs a formation command and a "stay in the light lane" instruction; babysitting AI is not fun.
- Under-leveled runs need explicit recommended objectives and a post-wipe status readout (time left, objectives remaining).
- Optional bargain systems (cursed ore) are fun only if explicit, capped, and have paydown.
- Time pressure must remain relevant even for high sustain parties.
