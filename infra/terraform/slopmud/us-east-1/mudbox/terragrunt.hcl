include "root" {
  path = find_in_parent_folders("root.hcl")
}

terraform {
  source = "../../modules/mudbox"
}

inputs = {
  name_prefix    = "mudbox"
  enable_compute = true
  os             = "debian12"
  instance_type  = "t3a.small"
  root_volume_gib = 30
  spot_max_price = ""

  zone_name        = "slopmud.com"
  record_name      = "mud"
  create_hosted_zone = true

  # Set this when you know what you want mud.slopmud.com to point at.
  # cname_target = "example.com"

  # Optional: override www.slopmud.com target. Defaults to the same as mud (or the instance public DNS).
  # www_cname_target = "example.com"

  # Allow the instance role to read specific SSM parameters (app secrets).
  # (Names only; values live in SSM and should not be committed.)
  ssm_read_parameter_names = [
    "/slopmud/dev/openai_api_key",
    "/slopmud/stg/openai_api_key",
    "/slopmud/prd/openai_api_key",

    # "/slopmud/prd/google_oauth_client_id",
    # "/slopmud/prd/google_oauth_client_secret",
  ]

  # Env-specific vanity CNAMES -> this instance (helpful when running multiple envs on one host with different ports).
  extra_cname_record_names = [
    "prd-gaia",
    "stg-gaia",
    "dev-gaia",
  ]

  # Optional: create + allow-read a compliance portal config JSON parameter (value should be passed via TF_VAR_...).
  # compliance_portal_config_json_ssm_name  = "/slopmud/prd/compliance_portal_config_json"
  # compliance_portal_config_json_ssm_value = "<json>"

  # SBC enforcement enable switch.
  # The record is created when name is set; `enabled` toggles the A-record value to avoid NXDOMAIN negative caching.
  # Keep disabled until verified in production.
  sbc_enable_dns_record_name    = "sbc-anti-lockout-prd"
  sbc_enable_dns_record_enabled = false
  # sbc_enable_dns_record_ttl   = 60
  # sbc_enable_dns_record_ip    = "192.0.2.1"
  # sbc_enable_dns_record_disabled_ip = "192.0.2.2"
}
