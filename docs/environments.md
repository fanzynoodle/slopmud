# Environments

This repo uses the term **track** for deployment environments:

- `dev`
- `stg` (staging)
- `prd`

## Naming Conventions (Canonical)

- Env files: `env/<track>.env` (example: `env/stg.env`)
- Systemd units: `slopmud-<track>` (example: `slopmud-stg`)
- Binaries: `/opt/slopmud/bin/slopmud-<track>`
- SSM Parameter Store prefix: `/slopmud/<track>/...` (example: `/slopmud/stg/openai_api_key`)

## Gaia Vanity Web Envs

Gaia vanity names follow:

- `dev-gaia.slopmud.com`
- `stg-gaia.slopmud.com`
- `prd-gaia.slopmud.com`

Env files:

- Static web: `env/<track>-gaia.env`
- OAuth web: `env/<track>-gaia-oauth.env` (sources the static env file and overrides only the web bind ports/binary)

Port layout is documented in `docs/gaia_ports.md`.
