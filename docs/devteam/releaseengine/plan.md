# RELEASE-ENGINE-10 Plan

## Next

- Standardize artifact naming and version metadata for all services.
- Add a minimal "deploy checklist" that matches `just deploy ...` and the real failure modes.

## Later

- Reduce deploy time with parallel build/copy where safe.
- Add staged rollouts for changes that touch auth/session or raft membership.

