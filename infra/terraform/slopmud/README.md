# slopmud infra

Terragrunt stack that creates:
- Public Route53 hosted zone `slopmud.com` and a `CNAME` record `mud.slopmud.com` -> instance public DNS
- `CNAME` record `www.slopmud.com` -> same target as `mud.slopmud.com` (overrideable via `www_cname_target`)

If you want DNS set up first, set `enable_compute=false` to create **Route53 only**.

## One-time setup

1. Ensure AWS auth works (console OAuth):
```bash
aws login
aws sts get-caller-identity
```

2. Create backend bucket + lock table:
```bash
chmod +x terraform/slopmud/bootstrap_state.sh
terraform/slopmud/bootstrap_state.sh
```

3. Make Terraform able to use `aws login` credentials:

This stack expects an AWS config profile called `tf` that uses `credential_process` to pull creds from your `default` CLI session.

```bash
cat >> ~/.aws/config <<'EOF'

[profile tf]
region = us-east-1
credential_process = aws configure export-credentials --profile default
EOF
```

## Deploy

```bash
cd terraform/slopmud/us-east-1/mudbox
terragrunt init
terragrunt apply
```

## Overrides

- If your SSH IP isn’t what `checkip.amazonaws.com` reports:
```bash
SSH_ALLOWED_CIDR="1.2.3.4/32" terragrunt apply
```

- If you want a specific SSH keypair:
```bash
SSH_PUBKEY_PATH="$HOME/.ssh/id_ed25519.pub" terragrunt apply
```
