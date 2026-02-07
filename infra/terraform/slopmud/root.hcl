locals {
  region     = "us-east-1"
  account_id = run_cmd("bash", "-lc", "aws sts get-caller-identity --query Account --output text")

  state_bucket = "tfstate-${local.account_id}-${local.region}-slopmud"
  lock_table   = "tf-locks-${local.account_id}-${local.region}-slopmud"

  # Default to current egress IP; override with SSH_ALLOWED_CIDR if needed.
  ssh_allowed_cidr = get_env("SSH_ALLOWED_CIDR", "") != "" ? get_env("SSH_ALLOWED_CIDR") : "${trimspace(run_cmd("bash", "-lc", "curl -fsSL https://checkip.amazonaws.com"))}/32"

  # Try to auto-detect a reasonable local SSH public key; override with SSH_PUBKEY_PATH.
  ssh_public_key_path = get_env("SSH_PUBKEY_PATH", "") != "" ? get_env("SSH_PUBKEY_PATH") : trimspace(run_cmd("bash", "-lc", "test -f ~/.ssh/id_ed25519.pub && echo ~/.ssh/id_ed25519.pub || (test -f ~/.ssh/id_rsa.pub && echo ~/.ssh/id_rsa.pub || echo '')"))
}

remote_state {
  backend = "s3"
  config = {
    bucket         = local.state_bucket
    key            = "${path_relative_to_include()}/terraform.tfstate"
    region         = local.region
    encrypt        = true
    dynamodb_table = local.lock_table
  }
}

terraform {
  extra_arguments "aws_auth" {
    commands = ["init", "plan", "apply", "destroy", "import", "refresh", "output", "state", "providers"]
    env_vars = {
      AWS_PROFILE         = "tf"
      AWS_SDK_LOAD_CONFIG = "1"
    }
  }
}

generate "provider" {
  path      = "provider.generated.tf"
  if_exists = "overwrite_terragrunt"
  contents  = <<EOF
provider "aws" {
  region = "${local.region}"

  # Use AWS CLI's credential resolution (including `aws login`) via credential_process.
  profile = "tf"
}
EOF
}

inputs = {
  region              = local.region
  ssh_allowed_cidr    = local.ssh_allowed_cidr
  ssh_public_key_path = local.ssh_public_key_path
}

