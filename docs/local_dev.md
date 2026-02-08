# Local Dev

## Ports

Default local dev stack (from `env/dev.env`):

- Session broker (TCP): `0.0.0.0:4940`
- Shard (TCP, broker-to-shard): `127.0.0.1:4941`

Other local ports:

- `just web-run-local` (static homepage dev server): `127.0.0.1:4943`
- `just web-sso-run-local` (OAuth callback + auth endpoints): `127.0.0.1:4942`
- `just ws-run-local` (websocket gateway): `127.0.0.1:4100`

E2E harness ports (do not collide with the dev stack):

- `just e2e-local`: broker `127.0.0.1:54010`, shard `127.0.0.1:55011`
- `just e2e-party`: broker `127.0.0.1:54100`, shard `127.0.0.1:55021`

## Start The Stack

Run shard + broker together (shard in the background, broker in the foreground):

```bash
just dev-run
```

Run the local web UI (served by `apps/static_web`):

```bash
just web-run-local
```

Then open:

- `http://127.0.0.1:4943/connect.html`
- `http://127.0.0.1:4943/play.html`

Or run them in two terminals:

```bash
just shard-run
just slopmud-run
```

## Connect And Complete Character Creation

Connect to the broker:

```bash
nc 127.0.0.1 4940
```

Creation flow (current as of 2026-02-08):

1. `name:` pick a name (letters/numbers/`_`/`-`, max 20)
2. `set password ...` pick a password (min 8 chars)
3. `type: human | bot` choose one
4. Two times `type: agree` (public licensing + code of conduct)
5. Race/class/sex prompts:
   - `race human`
   - `class fighter`
   - `none` (or `male` / `female` / `other`)

After that you should be in-world on the shard.

Useful smoke commands:

- `buildinfo`
- `uptime` (prints broker uptime + forwards to shard uptime/world time)
- `skills` / `skills compendium`
- `train list` then `train power_strike`, then `use power_strike`

## Local End-To-End Verification

This is the fastest way to prove local end-to-end works (broker <-> shard <-> gameplay):

```bash
just e2e-local
just e2e-party
just e2e-web-local  # selenium: web client creates account + logs in again
```

## Multi-Agent Local Dev (Dedicated Working Trees)

If you have multiple agents working in parallel, each agent should run from a dedicated working tree
and use a dedicated port block.

Rules:

1. Each agent uses its own working tree directory (example: `/tmp/a`, `/tmp/b`).
2. Each agent picks a `base_port` that is `100` away from other agents' base ports (example: `4940`, `5040`, `5140`).
3. Each agent generates an env file (`env/agent.env`) from that base port.

Automation:

```bash
# From the main repo:
just agent-worktree /tmp/a a 5040
just agent-worktree /tmp/b b 5140
```

In each agent working tree:

```bash
cd /tmp/a
just local-all env_file=env/agent.env
```

Then open:

- `http://127.0.0.1:5043/play.html`

Or run the selenium check (starts its own stack on a free 49xx port block):

```bash
just e2e-web-local
```

## Secrets

All secrets must live in files under `$SECRET_STORE` and be referenced by name (never committed).

Recommended pattern for local shells:

```bash
export SECRET_STORE="$HOME/.secrets/slopmud"
export SLOPMUD_OIDC_CLIENT_SECRET="$(cat "$SECRET_STORE/oidc_client_secret")"
```

If you are adding a new secret, add a `*_NAME` (or similar) knob rather than embedding secret material in repo files.
