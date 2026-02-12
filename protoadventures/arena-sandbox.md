---
adventure_id: arena-sandbox
area_id: A01
zone: "Arena"
clusters: [CL_ARENA_PIT]
hubs: []
setpieces: []
level_band: [1, 20]
party_size: [1, 5]
expected_runtime_min: 5
---

# Arena Sandbox

## Hook

You came here to settle something cleanly: no patrols, no quests, just a ring and a witnessless crowd.

Inputs (design assumptions):

- The arena is isolated and should not require overworld travel.
- Rooms are safe to respawn/teleport into for testing combat balance.
- No exits beyond the arena; leaving is out-of-band (admin warp / proto exit).

## Quest Line

- Success: defeat your opponent (or agree to stop).

## Room Flow

### R_ARENA_PIT (entry)

- beat: step into the sand; take stock
- teach: movement, look, inventory, readiness
- exits: north -> `R_ARENA_RING`

### R_ARENA_RING (ring)

- beat: the ring where grudges get paid
- teach: combat loop (attack, skills, disengage)
- exits: south -> `R_ARENA_PIT`

## NPCs

- none (bring your own opponent)

## Rewards

- none (this is a sandbox)

## Implementation Notes

- Add a toggleable PvP-on room tag later.
- Add a reset verb that restores HP/mana/stamina for fast iteration.
- If we add audience/NPCs, keep them non-blocking and low noise.
