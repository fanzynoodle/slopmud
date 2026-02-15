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

## Fast Deploys (Code Only)

For quick "hot" deploys that reuse the same asset tarball + install logic as CI, use:

```bash
just hot-deploy-slopmud dev
just hot-deploy-slopmud stg
just hot-deploy-slopmud prd
```

This relies on `scripts/cicd/slopmud-shuttle-assets`, which installs the new broker binary and restarts the
systemd unit without overwriting an existing unit file by default.

## How to verify a `dev` push reaches mud.slopmud.com

For this repo, a push to `dev` should trigger `.github/workflows/enterprise-cicd.yml` and run the deploy job with `DEPLOY_ENV=dev`.

To check live host state:

1. SSH key source (your org standard, one of):
   - AWS SSM parameter (example path: `/slopmud/dev/ops_ssh_key_pem`)
   - AWS Key Vault secret (equivalent secret name/path used by your organization)

2. Write the private key to disk, lock it down, and connect using `root` or `admin`:

```bash
KEY_PATH=~/.ssh/mud-dev-host-key.pem
chmod 0600 "$KEY_PATH"
ssh -i "$KEY_PATH" root@mud.slopmud.com "systemctl status slopmud-dev"
```

3. Confirm services are running and on the expected port:

```bash
ssh -i "$KEY_PATH" root@mud.slopmud.com \
  'sudo systemctl status slopmud-dev slopmud-shuttle-assets --no-pager; \
   sudo ss -ltnp | rg "(4000|4023|4200|443|4242|4042|4043)"'
```

If SSH is unreachable, validate DNS/instance and SGs (`mud.slopmud.com` points to the current instance and SSH is allowed from your egress IP).
