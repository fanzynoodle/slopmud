# slopmud infra

Terragrunt stack that creates:
- Public Route53 hosted zone `slopmud.com`
- Configurable DNS records for `mud.slopmud.com`, `www.slopmud.com`, the zone apex, and optional vanity CNAMEs
- Optional EC2 compute when you want a mudbox instance to own some or all of those records

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

- If landing, mud portal, and vanity names have different lifecycles, set the DNS targets explicitly instead of letting every name follow the mudbox instance:
```hcl
inputs = {
  mud_cname_target  = "game-host.example.com"
  www_cname_target  = "landing-host.example.com"
  apex_a_records    = ["203.0.113.10"]
  extra_cname_targets = {
    prd-gaia = "game-host.example.com"
  }
}
```

- Compliance portal config (domain allowlist) can be stored in SSM via Terraform variables:
  - Set `compliance_portal_config_json_ssm_name` and pass `compliance_portal_config_json_ssm_value` via `TF_VAR_compliance_portal_config_json_ssm_value` (keep it out of git).
