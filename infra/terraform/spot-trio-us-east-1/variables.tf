variable "region" {
  type        = string
  description = "AWS region to deploy into."
  default     = "us-east-1"
}

variable "name_prefix" {
  type        = string
  description = "Prefix used for resource names/tags."
  default     = "spot-trio"
}

variable "instance_count" {
  type        = number
  description = "Number of spot instances to run."
  default     = 3
}

variable "instance_type" {
  type        = string
  description = "EC2 instance type (e.g. t4g.nano, t3.nano)."
  default     = "t4g.nano"
}

variable "ami_ssm_parameter" {
  type        = string
  description = "SSM parameter name for the AMI ID."
  default     = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64"
}

variable "associate_public_ip_address" {
  type        = bool
  description = "Whether to assign a public IPv4 address (can add meaningful cost)."
  default     = true
}

variable "root_volume_gib" {
  type        = number
  description = "Root EBS volume size in GiB."
  default     = 8
}

variable "spot_max_price" {
  type        = string
  description = "Optional max spot price per hour (e.g. 0.002). Leave empty for default behavior."
  default     = ""
}

