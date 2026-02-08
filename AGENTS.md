# Agent Rules (slopmud)

## HTTPS (Certbot + Route53 DNS-01, No Nginx)

When the user asks to "get certbot working" or "enable HTTPS" for `slopmud.com`/`www.slopmud.com`, use the EC2 instance role + `certbot-dns-route53` (DNS-01) and the repo's `static_web` server (it serves TLS directly on `:443`).

Do this sequence:

1. Ensure the EC2 instance exists and DNS points at it (the instance is Spot and may be terminated):
   - `cd infra/terraform/slopmud/us-east-1/mudbox && terragrunt apply -auto-approve`
   - Then update `env/prd.env` `HOST` to the new instance `public_ip` (Terraform output).

2. Install certbot + Route53 plugin on the instance:
   - `just certbot-install prd`

3. Install the renewal deploy hook (copies certs to `/etc/slopmud/tls` and restarts `slopmud-web`):
   - `just certbot-hook-install prd`

4. Issue the cert for apex + `www` using DNS-01:
   - `just certbot-issue-web <email> prd`

5. Sync cert material to the path the service reads:
   - `just certbot-tls-sync prd`

6. Deploy + verify:
   - `just deploy prd`
   - `just https-smoke prd`

If SSH fails (timeout), it usually means the Spot instance was replaced and/or the security group no longer allows the current egress IP; step 1 fixes both.

