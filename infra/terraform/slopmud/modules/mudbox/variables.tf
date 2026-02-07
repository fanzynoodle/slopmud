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
