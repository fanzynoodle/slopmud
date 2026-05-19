# Mixed-Version Migration

This is the intended Raft/world-log upgrade story. It follows the same shape as
etcd-style rolling upgrades: binaries move first, persisted semantics move only
after the cluster is no longer mixed-version.

## Compatibility Contract

- A mixed-version cluster must emit only entries classified as rolling-safe for
  the old cluster version.
- Adding optional fields to existing log events is rolling-safe when old readers
  can ignore the field and new readers provide a default when it is absent.
- Adding a new event variant, changing the meaning of an existing field, or
  changing ordering/idempotency semantics is not rolling-safe. It requires a
  cluster feature gate.
- Downgrade is supported until a new cluster feature is activated. After
  activation, downgrade requires a restore/migration plan that rewrites state
  into the older format.

## Rollout Procedure

1. Deploy the new binary to followers first.
2. Transfer Raft leadership away from the old leader where possible.
3. Deploy the old leader last.
4. Keep `WORLD_LOG_FORMAT_VERSION` behavior at the previous cluster feature
   level while any member is still old.
5. Verify all members report the new binary and log compatibility level.
6. Activate the new feature/log level.
7. Only after activation may leaders emit gated event variants or new
   non-defaulted fields.

## Regression Coverage

- `worldlog` unit tests parse old character snapshots with missing newer fields.
- `worldlog` unit tests parse existing event variants with future optional
  fields.
- `worldlog` unit tests reject unknown event variants, documenting why those
  must be feature-gated.
- `worldlog` unit tests require every voter, not just a quorum, to advertise a
  target format before activation. The covered rollout shape is `AAB` rejected,
  `ABB` rejected, then `BBB` allowed.
- `raftlog` unit tests parse legacy envelopes without `term` and future
  envelopes with extra metadata.
- `raftlog` status RPCs advertise the application max format and default old
  peers to format `1` when that field is absent.
- `shard_01` unit tests replay the Raft-backed `ClusterFeatureSet` event so the
  active world-log format is restored after restart.
- `shard_01` unit tests verify feature-gated events cannot use the normal world
  append path or `project_world` local fallback path.
- `shard_01` unit tests block format-2 snapshot/LSMT persistence until both the
  active snapshot format and minimum reader format are advanced.
- `e2e_shard_raft_trio` covers `AAA -> AAB -> ABB -> BBB -> activate`,
  unreachable voter rejection, restart/replay of the active format, format-2
  metadata writes, and live Raft leader loss/rejoin behavior.

## Activation Command

Admins can run `raft feature worldlog <n> check` to dry-run activation. Running
`raft feature worldlog <n>` on the leader polls Raft status from every configured
voter, rejects unreachable or old-format voters, then appends a Raft-backed
`ClusterFeatureSet` record when the whole cluster can read the target format.
`raft status` shows the active world-log format, active snapshot format, minimum
reader format, and per-voter max format, role, leader, term, commit/last index,
quorum freshness, and status latency.

## Writer Gate

The world append path checks every `WorldEvent` against `world_event_write_gate`
before writing or applying it locally. Rolling-safe events can still be emitted
during binary rollout. Future incompatible events must be classified as requiring
an active world-log format; otherwise the exhaustive match will fail at compile
time when the event is added. `ClusterFeatureSet` is the only bootstrap event and
is accepted only through the activation command path.

`ClusterMetadataSet` is the first real format-2 event. It is intentionally small
metadata, but it uses the normal world append path and is rejected until
world-log format 2 is active.

The `neck` equipment slot follows the same rule even though it is represented
inside `CharacterSnapshot` rather than as a new event variant. Older binaries
can parse the snapshot but cannot preserve an unknown equipment key when they
reduce it back into local state, so equipping neck-slot items is blocked until
world-log format 2 is active.

## Lazy Migration

Snapshot dictionaries that can grow over time should use `VersionedDict`: known
entries are exposed to gameplay, while unknown entries are preserved verbatim
for write-back. `Equipment` uses this shape for future slots. A shard that reads
`equip: {"ring":"copper ring"}` before it knows what a ring slot is must not let
commands act on that slot, but it must keep the key/value pair when an unrelated
character snapshot is written.

This makes incremental migrations boring: replay can normalize old fields into
the current in-memory model, preserve future fields it cannot understand yet,
and lazily write the canonical shape when that object naturally changes. New
semantic writes still need an active world-log format gate until every live
binary has the preservation code.

## Persistence Gate

Snapshot and LSMT writers should call `evaluate_persistence_format_write` before
emitting a new artifact. Format-2 persistence is allowed only after both
`snapshot_format` and `min_reader_world_log_format` have advanced to 2, so a
mixed reader set cannot accidentally receive an unreadable snapshot or compacted
run.
