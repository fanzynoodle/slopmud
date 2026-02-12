# RUST-ARCHON-0 Plan

## Next

- Document the core architecture pillars:
  - event-based design + backpressure
  - QoS via rate limiting
  - async boundaries and cancellation
  - consensus/replication via Raft
- Define "build vs buy" criteria (default: build tiny in-house tools first).
- Establish performance budgets (memory, latency) per service.

## Later

- Create a scaling playbook: from a few MB RAM to "oh no" traffic without rewrites.
- Standardize internal crates for common patterns (limits, tracing, retries, streaming IO).

