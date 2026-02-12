# OPS-DROID-11 Plan

## Next

- Keep `just deploy prd` and `just https-setup ... prd` smooth and repeatable.
- Reduce "manual SSH poke" steps by baking more checks into `just` recipes.
- Maintain a minimal incident checklist: confirm host, confirm ports, confirm certs, confirm service.

## Later

- Add a short "rollback" recipe (previous binary + restart + verify).
- Move recurring tribal knowledge into docs and scripts (not chat history).

