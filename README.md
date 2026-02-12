# slopmud

Monorepo.

## Links

- Website: https://slopmud.com
- GitHub: https://github.com/fanzynoodle/slopmud

## Layout

- `crates/` Rust crates
- `apps/` Executables/services (future)
- `packages/` Shared libraries (future)
- `infra/` Infrastructure (Terraform/Terragrunt)
- `web_homepage/` Static homepage (served by `apps/static_web` or `apps/slopmud_web`)

## Ops (for now)

This repo is intentionally optimized for fast hand-driven ops:
- Base infrastructure is managed via Terraform/Terragrunt under `infra/`.
- Everything else (deploys, service admin, etc.) is driven via `just` recipes.
- CI/CD exists for `dev`/`stg` tracks via the self-hosted runner (see `.github/workflows/enterprise-cicd.yml`).
- Canonical environment naming (`dev`/`stg`/`prd`) is documented in `docs/environments.md`.

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

Or run the full local sanity suite:

```bash
just check
```

## Local Dev (Broker + Shard)

Ports for local dev and local E2E are documented in `docs/local_dev.md`.

Secrets must be stored under `$SECRET_STORE` (never committed) and referenced by name; see `docs/local_dev.md`.

Run in two terminals:

```bash
just shard-run
just slopmud-run
```

Connect:

```bash
nc 127.0.0.1 4940
```

Or run both (shard in background, broker in foreground):

```bash
just dev-run
```

Local end-to-end verification (starts its own stack on dedicated ports and completes character creation):

```bash
just e2e-local
just e2e-party
```
