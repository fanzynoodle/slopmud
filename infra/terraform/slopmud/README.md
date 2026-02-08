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

- If you want the instance role to be able to read SSM Parameter Store values (for app secrets like Google OAuth):
  - Add `ssm_read_parameter_names` in `terraform/slopmud/us-east-1/mudbox/terragrunt.hcl`
  - Create the parameters ad hoc:
```bash
aws ssm put-parameter --name "/slopmud/prd/google_oauth_client_id" --type SecureString --value "..." --overwrite
aws ssm put-parameter --name "/slopmud/prd/google_oauth_client_secret" --type SecureString --value "..." --overwrite
```

Or use the helper script:
```bash
./scripts/google_oauth_ssm_put.sh prd
```

- Compliance portal config (domain allowlist) can be stored in SSM via Terraform variables:
  - Set `compliance_portal_config_json_ssm_name` and pass `compliance_portal_config_json_ssm_value` via `TF_VAR_compliance_portal_config_json_ssm_value` (keep it out of git).
