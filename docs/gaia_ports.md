# Gaia Port Assignment

This repo currently supports running multiple environments on one host by:

- giving each env its own web ports (HTTP + HTTPS),
- giving each env its own systemd service name (`WEB_SERVICE_NAME`),
- giving each env its own TLS destination directory (`TLS_DST_DIR`),
- issuing one cert per env (`CERTBOT_CERT_NAME` + `CERTBOT_DOMAINS`) so cert lineages and SANs are not commingled.

Services involved:

- `apps/static_web`: static assets + `/ws` only (no OAuth callback).
- `apps/slopmud_web`: static assets + `/ws` + Google OAuth endpoints (`/auth/google`, `/auth/google/callback`).
- `crates/slopmud` (broker): telnet entrypoint. Prints the auth URL based on `SLOPMUD_GOOGLE_AUTH_BASE_URL`.

## DNS Names

Terraform creates env vanity CNAMEs (to the same instance target):

- `dev-gaia.slopmud.com`
- `stg-gaia.slopmud.com`
- `prd-gaia.slopmud.com`

## Web Port Layout (Gaia)

Each env runs two web services on its own ports:

- Static web (`apps/static_web`) on `xx43` (HTTPS): serves static assets + `/ws`.
- OAuth web (`apps/slopmud_web`) on `xx42` (HTTPS): serves `/auth/google` and `/auth/google/callback` (and may also serve static, but it doesn't need to be the thing users browse).

The broker prints the auth URL using `SLOPMUD_GOOGLE_AUTH_BASE_URL`, which should point at the OAuth web origin (the `xx42` port).

### dev-gaia

- Static web HTTPS: `4043` (`https://dev-gaia.slopmud.com:4043/`)
- OAuth web HTTPS: `4042` (`https://dev-gaia.slopmud.com:4042/`)
- OAuth callback (register in Google): `https://dev-gaia.slopmud.com:4042/auth/google/callback`

### stg-gaia

- Static web HTTPS: `4143` (`https://stg-gaia.slopmud.com:4143/`)
- OAuth web HTTPS: `4142` (`https://stg-gaia.slopmud.com:4142/`)
- OAuth callback (register in Google): `https://stg-gaia.slopmud.com:4142/auth/google/callback`

### prd-gaia

- Static web HTTPS: `443` (`https://prd-gaia.slopmud.com/`) (vanity port)
- OAuth web HTTPS: `4242` (`https://prd-gaia.slopmud.com:4242/`)
- OAuth callback (register in Google): `https://prd-gaia.slopmud.com:4242/auth/google/callback`

## Legacy Names

This env was previously referred to as `uat-gaia` / `test-gaia`.
Use `stg-gaia` consistently.

## Local Dev OAuth Port

For local OAuth testing, `just web-sso-run-local` binds:

- `http://localhost:4900/`
- Redirect URI: `http://localhost:4900/auth/google/callback`

Register that exact redirect URI in the Google OAuth client used for local dev.

## TLS Separation (One Cert Per Env)

Recommended env vars (per env file) to keep certs/services separated:

- `WEB_SERVICE_NAME=slopmud-web-<env>` (separate systemd units)
- `TLS_DST_DIR=/etc/slopmud/tls/<env>` (separate cert material)
- `TLS_CERT=${TLS_DST_DIR}/fullchain.pem`
- `TLS_KEY=${TLS_DST_DIR}/privkey.pem`
- `CERTBOT_CERT_NAME=<fqdn>` (separate lineage in `/etc/letsencrypt/live/<cert_name>/`)
- `CERTBOT_DOMAINS="<fqdn>"` (only domains for that env)

## Optional: Static `:443` + OAuth on Alternate Port

If you want `:443` to remain “static-only” (served by `apps/static_web`) but still use Google OAuth, you can register a redirect URI that points at a different port served by `apps/slopmud_web`, for example:

- static: `https://slopmud.com/` on `:443` (`apps/static_web`)
- OAuth callback: `https://slopmud.com:4242/auth/google/callback` (`apps/slopmud_web`)

In that model:

- `GOOGLE_OAUTH_REDIRECT_URI` must include the port.
- `SLOPMUD_GOOGLE_AUTH_BASE_URL` should point at the `slopmud_web` origin so the broker prints the correct URL.
