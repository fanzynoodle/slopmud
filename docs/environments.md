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

For quick "hot" deploys that reuse the same asset tarball + install logic as CI
on legacy one-box targets, use:

```bash
just hot-deploy-slopmud stg
just hot-deploy-slopmud prd
```

This relies on `scripts/cicd/slopmud-shuttle-assets`, which installs the new broker binary and restarts the
systemd unit without overwriting an existing unit file by default.
`dev` is intentionally excluded from that helper: CI deploys dev through
`scripts/cicd/deploy_split_raft_trio_from_asset.sh` so the public gateway always
uses the three Raft voters via `SHARD_ADDRS`.

## Low-Cost Single-AZ Raft Topology

The economical split-prod shape lives in `infra/terraform/single-az-raft-us-east-1`:

- one tiny on-demand public gateway for telnet, web, websocket, and the broker
- three tiny private Spot shard/Raft nodes in the same subnet/AZ
- no public IPv4 on Raft nodes
- broker-to-shard and Raft replication traffic stays on private VPC addresses

After applying Terraform, reconcile the stable Raft DNS records from current
ASG membership, then combine `terraform output recommended_env` with the usual
secret/env values and deploy the private Raft nodes through the gateway:

```bash
just reconcile-raft-dns infra/terraform/single-az-raft-us-east-1
just render-single-az-raft-env /tmp/slopmud-prd-split-az1.env /home/rob/slopmud/env/prd.env
just ensure-data-volumes /tmp/slopmud-prd-split-az1.env all
just deploy-split-raft-trio prd-split
just deploy-slopmud prd-split
just deploy-web-sso prd-split-oauth
```

The current one-box host can remain the build/deploy runner. The split deploy script reaches private Raft nodes through SSH ProxyJump via the gateway, so the Raft nodes do not need public IPs or a NAT gateway.

The Terraform stack attaches tiny encrypted data EBS volumes and the render helper points gateway accounts, nearline state, OAuth handoff state, and Raft logs at `/opt/slopmud/state`. Re-run `just ensure-data-volumes /tmp/slopmud-prd-split-az1.env all` after a rebuild or Spot replacement before deploying services.

For the smallest gateway shape, set `SLOPMUD_SBC_ENABLED=0` unless the SBC sidecar services are also deployed. The render helper sets that by default, which keeps the broker from repeatedly trying to subscribe to a local SBC event socket that intentionally does not exist on the tiny gateway.

For a local deployment that follows the same artifact path as CI, use
`just deploy-split-raft-trio-asset /tmp/slopmud-prd-split-az1.env`. Passing a
second argument deploys that exact local tarball or S3 artifact instead of
building a fresh asset.

Fresh gateways can restore cached cert material with `scripts/tls_cache_restore.sh /path/to/env.env` when the env defines `TLS_CACHE_FULLCHAIN_SSM` and `TLS_CACHE_PRIVKEY_SSM`. That gives the rebuilt gateway valid TLS before DNS is moved; verify with `curl --resolve mud.slopmud.com:4242:<gateway-ip> https://mud.slopmud.com:4242/healthz`.

## How to verify a `dev` push reaches mud.slopmud.com

For this repo, a push to `dev` should trigger `.github/workflows/enterprise-cicd.yml` and run:

1. `build` (artifact generation)
2. `deploy_sandbox` (deploy artifact to sandbox on port `4500`)
3. smoke test against `127.0.0.1:4500`
4. `deploy` (promote the same artifact to the dev Raft trio, then update the gateway on `4000`)
5. public smoke test against `dev-mud.slopmud.com:4000`

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

If SSH is unreachable, validate DNS/instance and SGs. The public dev smoke target is `dev-mud.slopmud.com:4000`; `mud.slopmud.com:4200` is the split-gateway prod telnet endpoint.

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
