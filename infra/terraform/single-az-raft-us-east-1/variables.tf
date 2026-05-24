variable "region" {
  type        = string
  description = "AWS region."
  default     = "us-east-1"
}

variable "name_prefix" {
  type        = string
  description = "Name/tag prefix."
  default     = "slopmud-az1"
}

variable "vpc_id" {
  type        = string
  description = "VPC to deploy into. Empty uses the default VPC."
  default     = ""
}

variable "availability_zone" {
  type        = string
  description = "Gateway AZ, and the default Raft AZ when raft_availability_zones/raft_subnet_ids are not set."
  default     = "us-east-1a"
}

variable "subnet_id" {
  type        = string
  description = "Gateway subnet in availability_zone. Empty selects the first subnet in the AZ."
  default     = ""
}

variable "raft_availability_zones" {
  type        = list(string)
  description = "Optional explicit AZ per Raft slot n0,n1,n2. Use three distinct AZs for cross-AZ quorum placement. Ignored when raft_subnet_ids is set."
  default     = []

  validation {
    condition = length(var.raft_availability_zones) == 0 || (
      length(var.raft_availability_zones) == 3 &&
      length(distinct(var.raft_availability_zones)) == 3
    )
    error_message = "raft_availability_zones must be empty or contain three distinct AZ names."
  }
}

variable "raft_subnet_ids" {
  type        = list(string)
  description = "Optional explicit subnet per Raft slot n0,n1,n2. Each slot data volume is created in that subnet's AZ."
  default     = []

  validation {
    condition     = length(var.raft_subnet_ids) == 0 || length(var.raft_subnet_ids) == 3
    error_message = "raft_subnet_ids must be empty or contain exactly three subnet IDs."
  }
}

variable "ssh_allowed_cidr" {
  type        = string
  description = "CIDR allowed to SSH to the public gateway."
}

variable "ssh_public_key_path" {
  type        = string
  description = "Path to the SSH public key installed on gateway and Raft nodes. Required for deploy scripts."
  default     = ""
}

variable "os" {
  type        = string
  description = "OS image. Supported: debian12."
  default     = "debian12"

  validation {
    condition     = var.os == "debian12"
    error_message = "Only debian12 is currently supported by the deploy scripts."
  }
}

variable "gateway_instance_type" {
  type        = string
  description = "Tiny non-Spot public gateway for telnet, web, websocket, and broker."
  default     = "t3a.nano"
}

variable "raft_instance_type" {
  type        = string
  description = "Tiny Spot shard/Raft node type. Keep x86_64 unless the release build supports arm64."
  default     = "t3a.nano"
}

variable "gateway_root_volume_gib" {
  type        = number
  description = "Gateway root EBS size. Needs room for the tiny self-hosted runner toolchain and warm Rust build cache."
  default     = 34
}

variable "raft_root_volume_gib" {
  type        = number
  description = "Raft node root EBS size."
  default     = 8
}

variable "data_volume_enabled" {
  type        = bool
  description = "Attach one persistent data EBS volume to the gateway and one to each Raft node."
  default     = true
}

variable "data_volume_type" {
  type        = string
  description = "EBS volume type for persistent gateway/Raft state."
  default     = "gp3"
}

variable "gateway_data_volume_gib" {
  type        = number
  description = "Gateway persistent data EBS size."
  default     = 1
}

variable "raft_data_volume_gib" {
  type        = number
  description = "Per-Raft-node persistent data EBS size."
  default     = 1
}

variable "raft_spot_max_price" {
  type        = string
  description = "Optional max Spot price per Raft node-hour. Empty means no cap."
  default     = ""
}

variable "raft_health_check_grace_period_seconds" {
  type        = number
  description = "ASG EC2 health check grace period for one-slot Raft groups."
  default     = 60
}

variable "raft_wait_for_capacity_timeout" {
  type        = string
  description = "How long Terraform waits for each one-slot Raft ASG to report capacity."
  default     = "5m"
}

variable "raft_private_ips" {
  type        = list(string)
  description = "Reserved for non-ASG topologies. Must be empty for the one-slot Raft Auto Scaling groups because EC2 Auto Scaling does not support pinned private IPs in launch templates."
  default     = []

  validation {
    condition     = length(var.raft_private_ips) == 0 || length(var.raft_private_ips) == 3
    error_message = "raft_private_ips must be empty or contain exactly three IPs."
  }
}

variable "gateway_public_tcp_ports" {
  type        = list(number)
  description = "Public TCP ports on the gateway. Defaults cover HTTP, HTTPS, telnet broker, playable web HTTPS, and internal OIDC HTTPS."
  default     = [80, 443, 4000, 4200, 4242, 9000]
}

variable "gateway_bind_port" {
  type        = number
  description = "Public broker port rendered into SLOPMUD_BIND for the split deploy env."
  default     = 4200
}

variable "shard_port" {
  type        = number
  description = "Private shard app port on each Raft node."
  default     = 5000
}

variable "raft_rpc_port" {
  type        = number
  description = "Private Raft RPC port on each Raft node."
  default     = 5100
}

variable "assets_bucket_name" {
  type        = string
  description = "Existing CI/CD assets bucket. Empty derives slopmud-assets-<account>-<region>."
  default     = ""
}

variable "wal_backup_bucket_name" {
  type        = string
  description = "Existing S3 bucket for streaming WAL backups. Empty reuses assets_bucket_name."
  default     = ""
}

variable "s3_gateway_endpoint_enabled" {
  type        = bool
  description = "Create a no-hourly-cost S3 Gateway VPC endpoint so private Raft nodes can fetch release artifacts without NAT."
  default     = true
}

variable "ec2_interface_endpoint_enabled" {
  type        = bool
  description = "Create an EC2 Interface VPC endpoint so private Raft replacement nodes can attach their EBS data volume without NAT."
  default     = true
}

variable "ssm_read_parameter_names" {
  type        = list(string)
  description = "SSM Parameter names the gateway may read for OAuth, TLS cache, OpenAI, etc."
  default     = []
}

variable "route53_zone_id" {
  type        = string
  description = "Optional hosted zone ID for DNS upserts from gateway user-data."
  default     = ""
}

variable "route53_zone_name" {
  type        = string
  description = "Zone apex used only for documentation/output when route53_zone_id is set."
  default     = "slopmud.com"
}

variable "raft_dns_record_prefix" {
  type        = string
  description = "Stable DNS record prefix for Raft slots. The runtime env uses <prefix>-n0/n1/n2.<route53_zone_name> instead of Terraform-rendered private IPs."
  default     = "slopmud-raft"

  validation {
    condition     = can(regex("^[A-Za-z0-9-]+$", var.raft_dns_record_prefix))
    error_message = "raft_dns_record_prefix may contain only letters, numbers, and hyphens."
  }
}

variable "raft_dns_reconcile_enabled" {
  type        = bool
  description = "Grant the gateway enough AWS API access to reconcile stable Raft DNS records from current ASG membership."
  default     = true
}

variable "dns_upsert_enabled" {
  type        = bool
  description = "If true, gateway user-data upserts dns_record_names to its current public IP/DNS. Keep false until cutover."
  default     = false
}

variable "dns_record_names" {
  type        = list(string)
  description = "FQDNs to upsert when dns_upsert_enabled=true. First record is an A record; the rest are CNAMEs to gateway public DNS."
  default     = []
}
