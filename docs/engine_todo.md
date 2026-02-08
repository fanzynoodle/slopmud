# Engine TODO (SmaugFUSS-Informed) (Draft)

Goal: build a "zero-copy" Rust MUD engine that can evolve into a RAFT-backed, FlatBuffers-wired, sharded system (single shard first), and that can run the Gaia world/quests we already outlined.

This doc is a pragmatic todo list, with a few SmaugFUSS takeaways that influence the plan.

Related world docs:

- Quest states and gates: `docs/quest_state_model.md`
- Quest graph: `docs/quest_state_graph.mermaid`
- Zones and clusters: `docs/zone_beats.md`
- Room-by-room protoadventures (playtest runs): `protoadventures/README.md`
- Overworld coords + exit lengths: `docs/overworld_cartesian_layout.md`
- Party + bot autofill policy: `docs/party_and_bots.md`
- Area summaries: `docs/area_summary.md`

## SmaugFUSS Takeaways (For Reference Only)

We should not copy SmaugFUSS code (Diku/Merc lineage). But it is useful to see which problems a "real" MUD solved.

Notable patterns worth recreating (in our own way):

- Single-threaded "game loop" driving pulses for systems like mobs/violence/areas (`reference/mud-codebases/smaugfuss/src/comm.c`, `reference/mud-codebases/smaugfuss/src/update.c`).
- World content is data-driven and hot-loadable: areas, commands, skills, socials (`reference/mud-codebases/smaugfuss/src/db.c`, `reference/mud-codebases/smaugfuss/src/tables.c`).
- Exits have an explicit `distance` (movement cost) and can be represented as virtual corridor rooms (`reference/mud-codebases/smaugfuss/src/mud.h`, `reference/mud-codebases/smaugfuss/src/act_move.c`).
- Hotboot/copyover keeps TCP sockets alive across an exec by saving descriptor state (`reference/mud-codebases/smaugfuss/src/hotboot.c`).
- Script triggers (mudprogs) attach to mobs/rooms/objects and change behavior based on named triggers (`reference/mud-codebases/smaugfuss/src/mud_prog.c`).

We should deliberately *not* replicate:

- Global mutable linked lists for everything.
- Ad-hoc parsing everywhere with shared global state.
- Tight coupling of networking, simulation, persistence, and content editing into one process.

## Target Architecture (Single Shard First)

Processes/services (start as separate binaries; can co-locate later):

- `session_broker`: owns telnet sockets; does login/auth; forwards player input to a shard; buffers output; keeps sessions alive through shard restarts.
- `shard_01`: authoritative world simulation (rooms, exits, combat, quests, spawns). For now there is exactly one shard.
- `chat`: global chat bus (optional initially; can be in-shard until we split).

State model:

- Static content: area/zone data (rooms, exits, spawn templates, quest hub tags).
- Dynamic state: characters, parties, quest state keys (`q.*`), gate keys (`gate.*`), timers, spawned NPCs, loot rolls.

Hot reload intent:

- Shard process can restart (deploy/hotreload) without dropping telnet sockets.
- Session broker reconnects the session stream to the new shard leader.

## Protocol Strategy (FlatBuffers + Zero-Copy)

We want all service-to-service traffic to be FlatBuffers messages carried in length-delimited frames.

Zero-copy guideline:

- Network read into `BytesMut`, parse frame boundaries without copying.
- Treat FlatBuffers buffers as immutable `Bytes` and route them as-is.
- Avoid `String` allocations on the hot path; only allocate when rendering text to telnet.

## RAFT Strategy (NIH, But Staged)

We want RAFT mainly for:

- continuous service during shard restarts,
- eventual multi-node redundancy,
- deterministic replay of world state transitions.

Staged approach:

- Phase A: implement a RAFT-shaped interface but run single-node (no network), still writing an append-only log + snapshots. This gives us restart recovery and "hot reload" under one machine.
- Phase B: implement real leader election + replication to N nodes.
- Phase C: allow multiple shards and cross-shard travel.

## TODO (Prioritized)

P0: Make A Single-Shard Game Playable

- [ ] Define a minimal command set and UX: `look`, `go <dir>`, `say`, `tell`, `who`, `help`, `party`, `quest`, `stats`.
- [ ] Implement a real "world model" in Rust: rooms, exits, occupants, items, NPCs (no combat yet).
- [ ] Implement movement with exit `len` (movement points later): newbie areas must be `len=1` everywhere.
- [ ] Implement "sealed exits" (`exit.state=sealed`) that never move the player and show the standard message from `docs/room_scale_plan.md`.
- [ ] Implement account + character persistence (SQLite or RocksDB): name, password hash, last room, quest state KV map.
- [ ] Implement `gate.*` evaluation for travel locks (zone gates and future A02+ blocked exits).
- [ ] Add basic admin commands: spawn item, teleport, open/close gate, view/set quest keys.

P0: Build The Service Split (Broker + Shard)

- [ ] Turn `crates/slopmud` into `session_broker` (or add `apps/session_broker`) that owns telnet sockets.
- [ ] Add `apps/shard_01` that the broker connects to over a local TCP/UDS link.
- [ ] Implement a session protocol: `Attach(session_id)`, `Input(line)`, `Output(text)`, `Detach`.
- [ ] Guarantee reconnect semantics: broker retries shard connection; shard can accept re-attach and resume stream ordering.

P0: Restart Recovery (Before Real RAFT)

- [ ] Implement append-only event log for shard state transitions (single-node) and periodic snapshots.
- [ ] On shard restart: load latest snapshot, replay log tail, accept session re-attaches.

P1: Party-First + Bot Auto-Fill

- [ ] Implement parties (size 1-5), party chat, invite/leave/kick.
- [ ] Implement bot autofill to reach 3 total actors at `setpiece.*` boundaries (`docs/party_and_bots.md`).
- [ ] Implement 3 bot roles (Vanguard/Medic/Striker) with intentionally dumb, predictable behavior.
- [ ] Ensure bots never claim loot, never consume limited rewards, and never permanently advance quest branches.

P1: Quests As Named State Machines

- [ ] Implement quest state storage exactly as key/value maps (`q.*`, `rep.*`, `gate.*`) with atomic updates.
- [ ] Implement hub/setpiece tags as stable IDs (HUB_* and `setpiece.*`) that quests can reference.
- [ ] Implement Q1-Q14 progression gates from `docs/quest_state_model.md` with server-side evaluation.
- [ ] Add quest debug tooling: list states, show blockers, show which gate is preventing progress.

P1: Combat + Spawns + Resets

- [ ] Implement the baseline combat loop (attack rounds, hit/avoid, damage, death, loot).
- [ ] Implement NPC spawns with simple reset rules (area repop) inspired by Smaug resets, but in our own format.
- [ ] Implement encounter scaling by (humans + bots) with a bot discount.
- [ ] Implement status effects needed for early content (poison-ish, slow, stun-lite).

P1: Content Pipeline (Area Files)

- [ ] Define our own area file format (recommend: one file per zone; JSON/TOML; stable IDs).
- [ ] Write a generator that converts `docs/zone_beats.md` + `docs/overworld_cartesian_layout.md` into stub zone files.
- [ ] Generator output: rooms with placeholder descriptions.
- [ ] Generator output: portal rooms for each overworld portal (`P_*`).
- [ ] Generator output: sealed exits to future areas (A02+ placeholders).
- [ ] Generator output: `len` on exits (movement cost).
- [ ] Add a validator: all portals are bidirectional (unless explicitly one-way).
- [ ] Add a validator: newbie region exit lengths are 1.
- [ ] Add a validator: no dangling exits.
- [ ] Add a validator: quest hubs exist and are reachable.

Authoring scaffolding that already exists (not the final area format yet):

- `just overworld-export` -> `world/overworld.yaml` + `world/overworld_pairs.tsv`
- `just zones-stubgen` -> `world/zones/*.yaml` (stub zone shapes)
- `just world-validate` -> runs `scripts/overworld_validate.py` + `scripts/areas_validate.py`

P2: FlatBuffers Everywhere

- [ ] Create FlatBuffers schema: session broker <-> shard.
- [ ] Create FlatBuffers schema: shard <-> chat (if split).
- [ ] Create FlatBuffers schema: raft replication entries and snapshots.
- [ ] Implement framing and zero-copy routing for FlatBuffers messages over TCP/UDS.
- [ ] Add golden tests for schema evolution (backward/forward compatibility).

P2: Real RAFT (Multi-Node)

- [ ] Implement leader election + log replication + membership (initially static 3 nodes).
- [ ] Define the replicated state machine boundary (event-log replication vs snapshot+delta replication).
- [ ] Implement snapshot install + log compaction.
- [ ] Implement client request routing to leader (broker discovers leader).

P3: Sharding (More Than One)

- [ ] Shard registry and routing (broker knows where to send a session).
- [ ] Implement cross-shard travel: save+freeze character state on source shard.
- [ ] Implement cross-shard travel: transfer character payload to destination shard.
- [ ] Implement cross-shard travel: reattach session stream without disconnecting telnet.
- [ ] Shared chat and shared account/identity across shards.

P3: Scripting (Mudprog-Like, But Safer)

- [ ] Decide scripting model (limited DSL for quest triggers vs embedded Lua with strict sandboxing).
- [ ] Attach scripts to rooms/NPCs/items via stable tags.
- [ ] Provide an offline test runner for scripts (no live debugging required).
