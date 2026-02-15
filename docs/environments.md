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

1. SSH key source (your org standard, one of):
   - AWS SSM parameter (example path: `/slopmud/dev/ops_ssh_key_pem`)
   - AWS Key Vault secret (equivalent secret name/path used by your organization)

2. Connect using `admin` or `root` and run host checks:

```bash
ssh -o StrictHostKeyChecking=accept-new admin@mud.slopmud.com
sudo systemctl status slopmud-dev --no-pager
```

3. Confirm services are running and on the expected ports:

```bash
ssh -o StrictHostKeyChecking=accept-new admin@mud.slopmud.com \
  'sudo ss -ltnp | rg "(4000|4023|4200|443|4242|4042|4043)"'
```

If SSH is unreachable, validate DNS/instance and SGs (`mud.slopmud.com` points to the current instance and SSH is allowed from your egress IP).

## CI/CD troubleshooting

- Quick tail of latest `dev` workflow run:

  ```bash
  run_id="$(gh run list --workflow enterprise-cicd.yml --branch dev --limit 1 --json databaseId --jq '.[0].databaseId')"
    gh run view "$run_id" --log --job \
      "$(gh run view "$run_id" --json jobs --jq '.jobs[] | select(.name=="Build + Store Asset") | .id')"
  ```

- Live job progress and runner-tail while waiting:

  ```bash
  run_id="$(gh run list --workflow enterprise-cicd.yml --branch dev --limit 1 --json databaseId --jq '.[0].databaseId')"
  gh run watch --interval 10 --workflow enterprise-cicd.yml
  ```

  For self-hosted runner live stdout while a step is running:

  ```bash
  ssh -o StrictHostKeyChecking=accept-new admin@mud.slopmud.com '
    log="$(ls -1t /opt/actions-runner/_diag/Worker_*.log | head -n 1)"
    tail -f "/opt/actions-runner/_diag/$log"
  '
  ```

- Live watch without opening the UI:

  ```bash
  gh run watch --workflow enterprise-cicd.yml --interval 10 --json status,databaseId,conclusion
  ``` 

- SSH to deployment host from `env/<track>.env`:

  ```bash
  source env/dev.env
  ssh -o StrictHostKeyChecking=accept-new -p "$SSH_PORT" "$SSH_USER@$HOST"
  ```

- If SSH fails from your machine, refresh the instance by updating DNS with Terraform (`terragrunt apply`) and confirm `HOST` in env files points at the active instance.
