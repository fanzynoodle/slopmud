provider "aws" {
  region = var.region
}

data "aws_caller_identity" "current" {}

data "aws_vpc" "default" {
  default = true
}

data "aws_subnets" "default" {
  filter {
    name   = "vpc-id"
    values = [data.aws_vpc.default.id]
  }
}

data "aws_ssm_parameter" "ami" {
  name = var.ami_ssm_parameter
}

locals {
  subnet_ids = sort(data.aws_subnets.default.ids)
  tags = {
    ManagedBy = "terraform"
    Stack     = "${var.name_prefix}-us-east-1"
  }
}

resource "aws_security_group" "this" {
  name_prefix = "${var.name_prefix}-"
  description = "Spot trio: default deny inbound; allow all egress."
  vpc_id      = data.aws_vpc.default.id

  egress {
    description      = "Allow all egress"
    from_port        = 0
    to_port          = 0
    protocol         = "-1"
    cidr_blocks      = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  tags = merge(local.tags, { Name = "${var.name_prefix}-sg" })
}

resource "aws_iam_role" "ssm" {
  name_prefix = "${var.name_prefix}-ssm-"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action = "sts:AssumeRole"
    }]
  })

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "ssm_core" {
  role       = aws_iam_role.ssm.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "ssm" {
  name_prefix = "${var.name_prefix}-"
  role        = aws_iam_role.ssm.name
  tags        = local.tags
}

resource "aws_instance" "spot" {
  count = var.instance_count

  ami                         = data.aws_ssm_parameter.ami.value
  instance_type               = var.instance_type
  subnet_id                   = element(local.subnet_ids, count.index % length(local.subnet_ids))
  vpc_security_group_ids      = [aws_security_group.this.id]
  iam_instance_profile        = aws_iam_instance_profile.ssm.name
  associate_public_ip_address = var.associate_public_ip_address

  instance_market_options {
    market_type = "spot"

    spot_options {
      instance_interruption_behavior = "terminate"
      spot_instance_type             = "one-time"
      max_price                      = var.spot_max_price != "" ? var.spot_max_price : null
    }
  }

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.root_volume_gib
    delete_on_termination = true
    encrypted             = true
  }

  tags = merge(local.tags, {
    Name = format("%s-%02d", var.name_prefix, count.index + 1)
  })
}
