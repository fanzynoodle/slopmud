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
- Lets Spot interruptions hit shard capacity while the gateway keeps player TCP/websocket sessions alive.

## Defaults

- Region: `us-east-1`
- AZ: `us-east-1a`
- Gateway: `t3a.nano`, on-demand, public IPv4
- Raft nodes: 3x `t3a.nano`, Spot, private IPv4 only
- OS: Debian 12 x86_64

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
2. Deploy the Raft trio through the gateway using private IPs from `terraform output recommended_env`.
3. Deploy the gateway broker with `SHARD_ADDRS` set to the three private shard addresses.
4. Deploy the web/websocket services to the gateway.
5. Smoke test telnet, websocket resume, `version`, and `raft status`.

The important runtime values are:

- `HOST` / `GATEWAY_HOST`: public DNS for the gateway.
- `SHARD_ADDRS`: private shard addresses for the broker.
- `SHARD_NODE_HOSTS`: private IPs used by deploy scripts through ProxyJump.
- `SHARD_TRIO_RAFT_PEERS`: literal Raft peer addresses.

## Replacement Behavior

Raft peer config currently uses literal `SocketAddr` values, not DNS names. If Spot nodes are replaced and private IPs change, regenerate deploy env from Terraform outputs and redeploy the shard units plus gateway broker env. To avoid that operational step, set `raft_private_ips` to three known-free private IPs in the selected subnet.
