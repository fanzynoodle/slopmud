# slopmud

Monorepo.

## Layout

- `crates/` Rust crates
- `apps/` Executables/services (future)
- `packages/` Shared libraries (future)
- `infra/` Infrastructure (Terraform/Terragrunt)
- `web_homepage/` Static homepage (served by `apps/static_web`)

## Ops (for now)

This repo is intentionally optimized for fast hand-driven ops:
- Base infrastructure is managed via Terraform/Terragrunt under `infra/`.
- Everything else (deploys, service admin, etc.) is driven via `just` recipes (no CI/CD yet).

## Engineering Stance

Other than security, zero-copy, async, and write-your-reads (for some stuff), we embrace the slop.

## NIH Corner

- `crates/slopio`: bespoke "zero-copy-ish" framing + line reading primitives (no tokio-util codecs).
- `crates/mudproto`: tiny binary protocol types for slopmud services (session/chat/shard).

## Rust

```bash
cargo build
cargo test
```

## Local Dev (Broker + Shard)

Run in two terminals:

```bash
just shard-run
just slopmud-run
```

Connect:

```bash
nc 127.0.0.1 4000
```

Or run both (shard in background, broker in foreground):

```bash
just dev-run
```
