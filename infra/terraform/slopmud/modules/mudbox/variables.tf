variable "region" {
  type        = string
  description = "AWS region."
}

variable "name_prefix" {
  type        = string
  description = "Name/tag prefix."
  default     = "mudbox"
}

variable "enable_compute" {
  type        = bool
  description = "Whether to create EC2/IAM/SG resources. When false, only Route53 resources are managed."
  default     = false
}

variable "os" {
  type        = string
  description = "OS image to use when enable_compute=true. Supported: debian12, al2023."
  default     = "debian12"
}

variable "instance_type" {
  type        = string
  description = "EC2 instance type."
  default     = "t3a.small"
}

variable "spot_max_price" {
  type        = string
  description = "Optional spot max price per hour. Empty means no cap."
  default     = ""
}

variable "associate_public_ip_address" {
  type        = bool
  description = "Assign a public IPv4 address."
  default     = true
}

variable "ssh_allowed_cidr" {
  type        = string
  description = "CIDR allowed to SSH (22/tcp)."
}

variable "ssh_public_key_path" {
  type        = string
  description = "Path to a local SSH public key to register as an EC2 key pair. If empty, no key pair is configured."
  default     = ""
}

variable "root_volume_gib" {
  type        = number
  description = "Root EBS volume size (GiB)."
  default     = 8
}

variable "zone_name" {
  type        = string
  description = "Route53 hosted zone name (e.g. slopmud.com)."
  default     = "slopmud.com"
}

variable "record_name" {
  type        = string
  description = "Record name relative to the zone (e.g. mud)."
  default     = "mud"
}

variable "create_hosted_zone" {
  type        = bool
  description = "Whether to create the hosted zone."
  default     = true
}

variable "cname_target" {
  type        = string
  description = "Target for mud.<zone> CNAME. If empty, the record is not created (unless enable_compute=true, in which case it points at the instance public DNS)."
  default     = ""
}

variable "www_cname_target" {
  type        = string
  description = "Target for www.<zone> CNAME. If empty, it falls back to cname_target, and then (if enable_compute=true) to the instance public DNS."
  default     = ""
}

variable "dns_admin_enabled" {
  type        = bool
  description = "Whether to attach a Route53 hosted-zone admin policy to the instance IAM role (useful for certbot DNS-01)."
  default     = true
}

variable "dns_admin_zone_id" {
  type        = string
  description = "Route53 hosted zone ID to administer when create_hosted_zone=false (e.g. Z123...). If create_hosted_zone=true, this is ignored."
  default     = ""
}

variable "assets_bucket_enabled" {
  type        = bool
  description = "Whether to create an S3 bucket for CI/CD build assets and grant the instance IAM role access."
  default     = true
}

variable "assets_bucket_name" {
  type        = string
  description = "Optional explicit S3 bucket name for CI/CD assets. If empty, a name is derived from account_id+region."
  default     = ""
}

variable "assets_bucket_force_destroy" {
  type        = bool
  description = "Whether to allow destroying the assets bucket even if it contains objects."
  default     = false
}

variable "ssm_read_parameter_names" {
  type        = list(string)
  description = "Optional list of SSM Parameter Store names that the instance role may read (useful for app secrets like OAuth client secrets). Example: /slopmud/prd/google_oauth_client_secret"
  default     = []
}

variable "extra_cname_record_names" {
  type        = list(string)
  description = "Optional additional CNAME record names (relative to zone_name) pointing at cname_target (or the instance public DNS if enable_compute=true). Example: [\"prd-gaia\", \"stg-gaia\", \"dev-gaia\"]."
  default     = []
}

variable "compliance_portal_config_json_ssm_name" {
  type        = string
  description = "Optional: create an SSM SecureString parameter with the compliance portal config JSON at this name (e.g. /slopmud/prd/compliance_portal_config_json). Value is supplied via compliance_portal_config_json_ssm_value."
  default     = ""
}

variable "compliance_portal_config_json_ssm_value" {
  type        = string
  description = "Value for compliance_portal_config_json_ssm_name. Keep this out of git; pass via TF_VAR_... at apply time."
  default     = ""
  sensitive   = true
}

variable "ses_send_enabled" {
  type        = bool
  description = "Whether to allow the instance IAM role to send email via Amazon SES (used by the compliance portal access-key email sender)."
  default     = true
}

variable "sbc_enable_dns_record_name" {
  type        = string
  description = "Optional DNS record name (relative to zone) used as the SBC enforcement enable switch (e.g. sbc-anti-lockout-prd)."
  default     = ""
}

variable "sbc_enable_dns_record_enabled" {
  type        = bool
  description = "Whether the SBC enforcement enable DNS record should be in the enabled state. The record is created whenever sbc_enable_dns_record_name is non-empty; this flag toggles the A-record value to avoid NXDOMAIN negative-caching delays."
  default     = false
}

variable "sbc_enable_dns_record_ttl" {
  type        = number
  description = "TTL for the SBC enforcement enable DNS record."
  default     = 60
}

variable "sbc_enable_dns_record_ip" {
  type        = string
  description = "Enabled-state IP value for the SBC enable A record (use a documentation/reserved IP by default)."
  default     = "192.0.2.1"
}

variable "sbc_enable_dns_record_disabled_ip" {
  type        = string
  description = "Disabled-state IP value for the SBC enable A record (use a documentation/reserved IP by default)."
  default     = "192.0.2.2"
}
