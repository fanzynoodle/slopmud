# Parties And Bots (Draft)

This doc defines the party model for Gaia and how we use bots to ensure group-first questing stays playable even when few humans are online.

## Party Size

- Party size (hard): 1-5.
- Group content target: 3-5 ("trio to quintet").
- Default expectation:
  - tutorials and casual contracts can be solo/duo friendly,
  - mainline quest setpieces (mechanical objectives and bosses) expect 3-5.

## Auto-Fill Policy (Bots)

If fewer than 3 humans are present when a party crosses a "setpiece boundary" (boss arena, valve run, beacon run, etc.):

- Spawn Gaia assist bots to bring the party to 3 total actors.
- Bots are temporary and leave at the end of the setpiece (or when humans replace them).

Replacement rules:

- If a human joins mid-run, a bot should step out cleanly at the next safe edge (doorway/checkpoint).
- If a human disconnects, a bot can step in to keep the party at 3.

Loot/credit rules (to avoid bots feeling like "real players"):

- Bots do not roll or claim loot.
- Bots do not consume limited quest rewards.
- Bots should not permanently advance reputation/quest branches; only humans do.

## Bot Roles (Simple)

Keep bots boring and predictable. We only need enough to complete "3-player math".

Suggested role archetypes:

- Vanguard: high durability, taunt/threat tools, simple positioning.
- Medic: heals/cleanses, low damage, prioritizes keeping humans alive.
- Striker: consistent damage, avoids pulling extra packs.

Default fill pattern:

- 1 human: add Vanguard + Medic.
- 2 humans: add Vanguard (or Medic if humans are fragile).
- 3+ humans: no bots unless requested.

## Encounter Scaling (Draft)

Encounters should scale by total actors (humans + bots), but with a "bot discount" so 1 human + 2 bots is not equivalent to 3 humans.

Initial tuning:

- Treat each bot as ~0.6 of a human for scaling calculations.
- Boss mechanics should still be solvable with 1-2 humans (bots can execute basic "do the obvious thing" logic).

## Setpiece Boundaries

A setpiece boundary is any room cluster tagged as:

- `setpiece.*` (see `docs/quest_state_model.md`)

When we implement area files, we should be able to mark an exit or room with:

- `requires.party_min = 3`
- `requires.party_max = 5`
- `autofill.bots = true`

