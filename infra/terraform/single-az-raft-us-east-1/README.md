# Single-AZ Gateway + Raft Trio

Economical production topology for one AWS region and one AZ:

- One tiny on-demand gateway instance.
  - Public telnet/web/websocket endpoint.
  - Runs the session broker and web services.
  - Has the only public IPv4 address in the game path.
- Three tiny Spot Raft/shard instances.
  - Same subnet/AZ as the gateway.
  - No public IPv4 addresses.
  - Broker-to-shard and Raft replication traffic use private VPC IPs only.

The current one-box mud host can remain the build/deploy runner. Deploy to the private Raft nodes through the gateway with SSH ProxyJump.

## Why This Shape

- Keeps the player-facing endpoint stable on non-Spot EC2.
- Keeps the expensive public IPv4 count to one.
- Avoids cross-AZ data transfer by pinning all four instances to one subnet/AZ.
- Lets private Raft nodes fetch release artifacts from S3 through a Gateway VPC endpoint instead of a NAT gateway.
- Lets Spot interruptions hit shard capacity while the gateway keeps player TCP/websocket sessions alive.

## Defaults

- Region: `us-east-1`
- AZ: `us-east-1a`
- Gateway: `t3a.nano`, on-demand, public IPv4
- Raft nodes: 3x `t3a.nano`, Spot, private IPv4 only
- Public dev telnet: `dev-mud.slopmud.com:4000` on the gateway
- Public prod telnet: `mud.slopmud.com:4200` on the gateway
- Persistent state: 1 GiB encrypted EBS data volume on the gateway and each Raft node
- OS: Debian 12 x86_64
- S3 Gateway endpoint: enabled, no hourly endpoint charge, used for private release artifact fetches

`t4g.nano` is usually cheaper, but the current release build path emits x86_64 Debian binaries. Switch to Arm only after the build/deploy path produces arm64 artifacts.

## Apply

Use a tfvars file rather than committing account-specific values:

```hcl
ssh_allowed_cidr   = "203.0.113.10/32"
ssh_public_key_path = "~/.ssh/slopmud.pub"
availability_zone  = "us-east-1a"

ssm_read_parameter_names = [
  "/slopmud/prd/openai_api_key",
  "/slopmud/prd/google_oauth_client_id",
  "/slopmud/prd/google_oauth_client_secret",
]
```

Then:

```bash
cd infra/terraform/single-az-raft-us-east-1
terraform init
terraform apply -var-file=prod.tfvars
terraform output recommended_env
```

Keep `dns_upsert_enabled = false` until the new stack is deployed and smoke-tested. At cutover, enable it with the desired record names, or update Route53 separately.

## Deploy Flow

1. Build/publish the normal artifact from the current build machine.
2. Render the split env from Terraform outputs and mount data volumes:
   `just render-single-az-raft-env /tmp/slopmud-prd-split-az1.env /home/rob/slopmud/env/prd.env`
   then `just ensure-data-volumes /tmp/slopmud-prd-split-az1.env all`.
3. Deploy the Raft trio through the gateway using private IPs from `terraform output recommended_env`.
4. Deploy the gateway broker with `SHARD_ADDRS` set to the three private shard addresses.
5. Restore cached TLS if needed and deploy the web/websocket services to the gateway.
6. Smoke test telnet, websocket resume, `version`, and `raft status`.

For normal code-only shard upgrades after the split stack is already running,
use the zero-downtime live path:

```bash
just live-upgrade-split-raft-trio /tmp/slopmud-prd-split-az1.env
```

That target does not restart the public gateway, web, or websocket process. It
stages the new shard binary on all three private Raft nodes, atomically swaps the
node-local binary path, transfers Raft leadership away from any node about to be
restarted, and restarts nodes one at a time. If leadership transfer is
unavailable, or if the script cannot see a healthy Raft leader, the live path
refuses to restart the active leader.

When `target/release/shard_01` already contains the desired binary, use:

```bash
just live-upgrade-split-raft-trio-fast /tmp/slopmud-prd-split-az1.env
```

For the faster fan-out path, publish the shard binary once to S3 and let all
three private Raft nodes pull it concurrently before activation:

```bash
just live-upgrade-split-raft-trio-s3 /tmp/slopmud-prd-split-az1.env
```

When the local release binary is already built:

```bash
just live-upgrade-split-raft-trio-s3-fast /tmp/slopmud-prd-split-az1.env
```

The S3 path uploads `target/release/shard_01` to
`s3://$ASSETS_BUCKET/split-raft/$ENV_NAME/$SLOPMUD_RELEASE_ID/shard_01` by
default, writes a sibling `.sha256`, and has every Raft node verify the checksum
before atomically swapping its local binary symlink. S3 fetch is parallel and
does not affect quorum. Process activation remains rolling: before each restart
the script acquires a leader-owned Raft restart lease, verifies that the other
two voters are reachable, verifies that the node about to restart is not still
the leader, and releases the lease after the restarted node rejoins. This lease
is the cluster-side guard that keeps racing deploy controllers or k8s-style
hooks from restarting two voters at once. During the first rollout from an older
binary, `SLOPMUD_RAFT_RESTART_LEASE=auto` falls back to the older local guard;
after all voters run a lease-aware build, use `SLOPMUD_RAFT_RESTART_LEASE=required`
for strict k8s-style activations.

The important runtime values are:

- `HOST` / `GATEWAY_HOST`: public DNS for the gateway.
- `dev-mud.slopmud.com:4000`: outside-in dev telnet smoke target. CI must prove this path after a dev deploy.
- `ASSETS_BUCKET`: S3 bucket used for release artifacts.
- `SLOPMUD_WAL_BACKUP_S3_BUCKET` / `SLOPMUD_WAL_BACKUP_S3_PREFIX`: S3 target for per-node streaming WAL backups. By default this reuses the assets bucket under a separate prefix.
- `SLOPMUD_WAL_RESTORE_ENABLED=auto`: replaced nodes attempt restore from WAL backups when their local Raft log is missing or empty, and skip cleanly if no manifest exists yet.
- `SHARD_ADDRS`: private shard addresses for the broker.
- `SHARD_NODE_HOSTS`: private IPs used by deploy scripts through ProxyJump.
- `SHARD_TRIO_RAFT_PEERS`: literal Raft peer addresses.

## Replacement Behavior

Raft peer config currently uses literal `SocketAddr` values, not DNS names. If Spot nodes are replaced and private IPs change, regenerate deploy env from Terraform outputs and redeploy the shard units plus gateway broker env. To avoid that operational step, set `raft_private_ips` to three known-free private IPs in the selected subnet.

The data EBS volumes survive instance replacement and are reattached by Terraform. After a gateway rebuild or Spot replacement, run the data-volume mount helper before service deploy; the generated env stores gateway accounts, OAuth handoffs, nearline state, and Raft logs under `/opt/slopmud/state`. If the data volume is unavailable or empty, the shard service can restore its node-specific Raft WAL from S3 before joining the cluster.
