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

data "aws_ami" "debian12" {
  count = var.os == "debian12" ? 1 : 0

  most_recent = true
  owners      = ["136693071363"] # Debian

  filter {
    name   = "name"
    values = ["debian-12-amd64-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }

  filter {
    name   = "root-device-type"
    values = ["ebs"]
  }
}

data "aws_ssm_parameter" "al2023_ami" {
  count = var.os == "al2023" ? 1 : 0

  name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64"
}

locals {
  subnet_ids = sort(data.aws_subnets.default.ids)
  ami_id     = var.os == "al2023" ? data.aws_ssm_parameter.al2023_ami[0].value : data.aws_ami.debian12[0].id
  tags = {
    ManagedBy = "terraform"
    Stack     = var.name_prefix
  }
}

resource "aws_security_group" "this" {
  count = var.enable_compute ? 1 : 0

  name_prefix = "${var.name_prefix}-"
  description = "mudbox: SSH limited; other ports wide open per request."
  vpc_id      = data.aws_vpc.default.id

  ingress {
    description = "SSH from your IP"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.ssh_allowed_cidr]
  }

  ingress {
    description = "Telnet (world)"
    from_port   = 23
    to_port     = 23
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "HTTP (world)"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "HTTPS (world)"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "4000-9999 (world)"
    from_port   = 4000
    to_port     = 9999
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    description = "Allow all egress"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(local.tags, { Name = "${var.name_prefix}-sg" })
}

resource "aws_iam_role" "ssm" {
  count = var.enable_compute ? 1 : 0

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
  count      = var.enable_compute ? 1 : 0
  role       = aws_iam_role.ssm[0].name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "ssm" {
  count = var.enable_compute ? 1 : 0

  name_prefix = "${var.name_prefix}-"
  role        = aws_iam_role.ssm[0].name
  tags        = local.tags
}

resource "aws_key_pair" "this" {
  count = var.enable_compute && var.ssh_public_key_path != "" ? 1 : 0

  key_name_prefix = "${var.name_prefix}-"
  public_key      = file(var.ssh_public_key_path)

  tags = local.tags
}

resource "aws_instance" "this" {
  count = var.enable_compute ? 1 : 0

  ami                         = local.ami_id
  instance_type               = var.instance_type
  subnet_id                   = element(local.subnet_ids, 0)
  vpc_security_group_ids      = [aws_security_group.this[0].id]
  iam_instance_profile        = aws_iam_instance_profile.ssm[0].name
  associate_public_ip_address = var.associate_public_ip_address
  key_name                    = var.ssh_public_key_path != "" ? aws_key_pair.this[0].key_name : null

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

  tags = merge(local.tags, { Name = var.name_prefix })
}

resource "aws_route53_zone" "this" {
  count = var.create_hosted_zone ? 1 : 0

  name = var.zone_name
  tags = local.tags
}

locals {
  hosted_zone_id = var.create_hosted_zone ? aws_route53_zone.this[0].zone_id : null
}

resource "aws_route53_record" "mud_cname" {
  # Count cannot depend on resource attributes. If compute is enabled, we create the record
  # and let the target resolve to the instance's public DNS after apply.
  count = var.create_hosted_zone && (var.enable_compute || var.cname_target != "") ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = "${var.record_name}.${trim(var.zone_name, ".")}"
  type    = "CNAME"
  ttl     = 60
  records = [var.cname_target != "" ? var.cname_target : aws_instance.this[0].public_dns]
}
