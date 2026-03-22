# Environments

This repo uses the term **track** for deployment environments:

- `dev`
- `sandbox` (pre-dev validation target)
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

## One Stack, Split Web Lifecycles

Production keeps a single infrastructure stack/host (`mudbox`) but separates web service lifecycles:

- Landing site (`slopmud.com`/`www`) runs as `slopmud-landing` from `env/prd_landing.env`.
- Web portal/service (`mud.slopmud.com` path and auth endpoints) runs as `slopmud-web` from `env/prd.env`.
- Game broker/shard lifecycle remains independent from landing deploys.

Shared deploy entrypoint (local + CI):

```bash
./scripts/deploy_web_target.sh landing prd
./scripts/deploy_web_target.sh webportal prd
./scripts/deploy_web_target.sh both prd
```

## Fast Deploys (Code Only)

For quick "hot" deploys that reuse the same asset tarball + install logic as CI (broker + shard), use:

```bash
just hot-deploy-slopmud dev
just hot-deploy-slopmud stg
just hot-deploy-slopmud prd
```

This relies on `scripts/cicd/slopmud-shuttle-assets`, which installs the new broker binary and restarts the
systemd unit without overwriting an existing unit file by default.

## Asset Build And Deploy Flow

The important split is:

- cloud-init user data re-registers DNS after spot instance replacement
- CI and local deploy flows both build the same artifact shape
- on-host install/redeploy is handled by `slopmud-shuttle-assets`

```mermaid
flowchart TD
  A[Feature worktree<br/>/tmp/slopmud-wt-...] --> B[Local build or just hot-deploy-slopmud]
  A --> C[Push branch]
  C --> D[GitHub Actions enterprise-cicd.yml]
  D --> E[Build artifact.tgz]
  E --> F[S3 assets bucket]

  G[Terraform mudbox stack] --> H[Launch template + ASG]
  H --> I[EC2 instance boot]
  I --> J[cloud-init per-boot DNS script]
  J --> K[Route53 updates apex A and mud/www CNAMEs]

  L[scripts/cicd/bootstrap_runner.sh] --> M[/usr/local/bin/slopmud-shuttle-assets]
  F --> M
  B --> N[scripts/cicd/hot_deploy_slopmud.sh]
  N --> M
  M --> O[/opt/slopmud/assets/<env>/<sha>/]
  M --> P[/opt/slopmud/bin/slopmud-<env> and shard-01-<env>]
  P --> Q[systemd restart]
  Q --> R[Broker + shard + web serve new assets]
```

For prod recovery after instance replacement, Terraform/cloud-init handles DNS reachability and the instance role handles S3/SSM reads. The actual app bits still need either CI deploy promotion or a local `just hot-deploy-slopmud prd` style redeploy to install the artifact on the replacement host.

## How to verify a `dev` push reaches mud.slopmud.com

For this repo, a push to `dev` should trigger `.github/workflows/enterprise-cicd.yml` and run:

1. `build` (artifact generation)
2. `deploy_sandbox` (deploy artifact to sandbox on port `4500`)
3. smoke test against `127.0.0.1:4500`
4. `deploy` (promote the same artifact to `dev` on `4000`)

If any sandbox step fails, the `dev` deploy is blocked and `deploy` does not run.

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
  'sudo ss -ltnp | rg "(4000|4023|4200|4500|443|4242|4042|4043)"'
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
  # GitHub CLI step logs for in-progress jobs are sparse; use runner blocks for live feedback.
  ssh -o StrictHostKeyChecking=accept-new admin@mud.slopmud.com '
    bdir=/opt/actions-runner/_diag/blocks
    latest="$(ls -1t "$bdir" | grep -m 1 -E "\\.1$")"
    tail -n 200 -f "$bdir/$latest"
  '
  ```

- Live watch without opening the UI:

  ```bash
  gh run watch --workflow enterprise-cicd.yml --interval 10
  ``` 

- SSH to deployment host from `env/<track>.env`:

  ```bash
  source env/dev.env
  ssh -o StrictHostKeyChecking=accept-new -p "$SSH_PORT" "$SSH_USER@$HOST"
  ```

- If SSH fails from your machine, refresh the instance by updating DNS with Terraform (`terragrunt apply`) and confirm `HOST` in env files points at the active instance.
