# WAL Backups

`shard_01` can stream raw Raft WAL byte extents to a local LSMT-style backup
catalog and optionally upload the same immutable extents plus manifests to S3.

The hot path does not deserialize game events. Once per interval, the backup
worker checks the WAL length, copies only newly appended bytes into immutable
extent files, writes a tiny manifest, and uploads only new extent objects plus
the manifest objects when S3 is configured.

## Runtime

Enable with either a local backup directory or an S3 bucket:

```sh
SLOPMUD_WAL_BACKUP_ENABLED=1
SLOPMUD_WAL_BACKUP_DIR=/var/lib/slopmud/walbackup
SLOPMUD_WAL_BACKUP_INTERVAL_S=60
SLOPMUD_WAL_BACKUP_MAX_SEGMENT_BYTES=536870912
SLOPMUD_WAL_BACKUP_S3_BUCKET=slopmud-backups
SLOPMUD_WAL_BACKUP_S3_PREFIX=prd/wal
```

The on-disk layout is:

```text
v1/nodes/<node>/lsmt/segments/g<generation>/<start>-<end>.wal
v1/nodes/<node>/lsmt/manifests/<created>-g<generation>-<source_len>.json
v1/nodes/<node>/lsmt/manifests/latest.json
```

The segment payload is raw WAL bytes. The manifest is small JSON metadata that
lists contiguous byte extents and best-effort Raft line metadata for completed
lines inside each extent. New manifests include a SHA-256 for each extent;
older manifests without checksums still restore, but checksum-backed manifests
detect same-length extent corruption before recovery replaces the target WAL.

## Startup Restore

`shard_01` can restore `SHARD_RAFT_LOG` before it opens the listener. Restore is
idempotent by default: if the target WAL already exists and is non-empty, restore
is skipped.

```sh
SLOPMUD_WAL_RESTORE_ENABLED=auto
SLOPMUD_WAL_RESTORE_S3_BUCKET=slopmud-backups
SLOPMUD_WAL_RESTORE_S3_PREFIX=prd/wal
SLOPMUD_WAL_RESTORE_CACHE_DIR=/var/lib/slopmud/walrestore-cache
SLOPMUD_WAL_RESTORE_NODE_ID=shard-a
SLOPMUD_WAL_RESTORE_MISSING_OK=1
```

`auto` skips restore when no source is configured and treats a missing manifest
as non-fatal. Use `SLOPMUD_WAL_RESTORE_OVERWRITE=1` only for deliberate
replacement of an existing WAL. Restore removes the adjacent Raft state file
after writing a WAL so term and commit metadata are recomputed from the restored
log.

## Recovery CLI

List backup manifests and latest extents:

```sh
slopmud_adminctl wal-backup list --dir /var/lib/slopmud/walbackup --extents
slopmud_adminctl wal-backup list --s3 s3://slopmud-backups/prd/wal --node-id shard-a --json
```

Recover locally:

```sh
slopmud_adminctl wal-backup recover \
  --dir /var/lib/slopmud/walbackup \
  --node-id shard-a \
  --out /var/lib/slopmud/shard_01_raft.recovered.jsonl \
  --until-index 1000
```

Recover from S3 through a local cache:

```sh
slopmud_adminctl wal-backup recover \
  --s3 s3://slopmud-backups/prd/wal \
  --cache-dir /var/lib/slopmud/walbackup-cache \
  --node-id shard-a \
  --out /var/lib/slopmud/shard_01_raft.recovered.jsonl \
  --manifest-unix-at-or-before 1779163138
```

Verify local extent continuity and file sizes:

```sh
slopmud_adminctl wal-backup verify --dir /var/lib/slopmud/walbackup
```

## Cost And Performance

The default segment cap is 512 MiB so large WAL growth is broken into S3-safe
PUT objects without buffering whole objects in memory. S3 uploads use
`ByteStream::from_path`; S3 downloads stream into files before recovery.

Cost controls:

- no S3 writes happen when the WAL has not advanced;
- one immutable manifest plus `latest.json` are uploaded per changed interval;
- local old immutable manifests are pruned by count and unreferenced old
  generation extents are removed from disk;
- segment size can be increased to reduce PUT count or lowered for faster retry
  granularity.
