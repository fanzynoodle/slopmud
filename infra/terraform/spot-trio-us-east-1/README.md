# Spot Trio (us-east-1)

Terraform stack to run 3 ultra-cheap EC2 **Spot** instances in `us-east-1` with minimal defaults.

## Options (Cheap Instance Types)

These are current **Spot compute** prices observed in `us-east-1` over the last ~6 hours (Linux/UNIX), and an estimated monthly cost for **3 instances** at the median price (730 hrs/mo).

| Instance type | Arch | vCPU | Mem | Median spot $/hr | 3x monthly (compute only) |
|---|---:|---:|---:|---:|---:|
| `t4g.nano` | arm64 | 2 | 0.5 GiB | ~$0.0012 | ~$2.63/mo |
| `t3.nano` | x86_64 | 2 | 0.5 GiB | ~$0.0016 | ~$3.50/mo |
| `t3a.nano` | x86_64 | 2 | 0.5 GiB | ~$0.00185 | ~$4.05/mo |
| `t4g.micro` | arm64 | 2 | 1.0 GiB | ~$0.0035 | ~$7.67/mo |

Notes:
- `t4g.*` is typically the cheapest, but it is **ARM** (make sure your workload supports arm64).
- These numbers exclude **EBS**, **public IPv4**, and any other service costs.

## Important Cost Gotchas (Bigger Than Spot Compute)

- **Public IPv4**: your account is currently being charged for `PublicIPv4:InUseAddress`. If you give each instance a public IPv4, that can cost more than the Spot compute. Use `associate_public_ip_address = false` only if you already have a NAT/VPC endpoints strategy (NAT Gateway is not cheap).
- **EBS volumes**: root disks cost money even if instances are stopped (unless deleted). This stack deletes root disks on termination.

## Quick Start

1. (Optional) Ensure you are authenticated:
   - If you are using console OAuth: `aws login`

2. Initialize / apply:

```bash
cd terraform/spot-trio-us-east-1
terraform init
terraform apply
```

## Defaults

- Region: `us-east-1`
- Instance count: `3`
- Default instance type: `t4g.nano` (arm64)
- AMI: Amazon Linux 2023 via SSM Parameter (arm64 by default)
- No inbound security-group rules (egress allowed)
- Spot market instances (`instance_market_options`)
- Root volume: gp3, size configurable, deleted on termination

## Customization Examples

Use x86_64:

```bash
terraform apply -var='instance_type=t3.nano' -var='ami_ssm_parameter=/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64'
```

Increase memory:

```bash
terraform apply -var='instance_type=t4g.micro'
```

Set a Spot max price cap (per-instance $/hr):

```bash
terraform apply -var='spot_max_price=0.002'
```

