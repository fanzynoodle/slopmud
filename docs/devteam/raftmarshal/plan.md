# RAFT-MARSHAL-14 Plan

## Next

- Write the "QoS and backpressure" runbook:
  - per-IP / per-account limits
  - async work queues with bounded memory
  - load shedding strategy
- Define Raft invariants and tests (recovery, leader changes, snapshot restore).

## Later

- Add chaos tests for partitions and slow disks.
- Standardize an internal event envelope format to reduce ad-hoc glue code.

