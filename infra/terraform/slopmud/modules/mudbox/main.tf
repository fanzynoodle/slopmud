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
  assets_bucket_name = var.assets_bucket_name != "" ? var.assets_bucket_name : "slopmud-assets-${data.aws_caller_identity.current.account_id}-${var.region}"
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
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "ssm_core" {
  count      = var.enable_compute ? 1 : 0
  role       = aws_iam_role.ssm[0].name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

locals {
  # Zone ID to grant admin privileges over (for DNS-01 challenges, etc.).
  dns_admin_zone_id = var.create_hosted_zone ? aws_route53_zone.this[0].zone_id : var.dns_admin_zone_id
}

data "aws_iam_policy_document" "dns_admin" {
  count = var.enable_compute && var.dns_admin_enabled && (var.create_hosted_zone || var.dns_admin_zone_id != "") ? 1 : 0

  statement {
    sid    = "Route53ZoneAdmin"
    effect = "Allow"
    actions = [
      "route53:ChangeResourceRecordSets",
      "route53:GetHostedZone",
      "route53:ListResourceRecordSets",
      "route53:ListTagsForResource",
    ]
    resources = ["arn:aws:route53:::hostedzone/${local.dns_admin_zone_id}"]
  }

  statement {
    sid       = "Route53ChangeRead"
    effect    = "Allow"
    actions   = ["route53:GetChange"]
    resources = ["arn:aws:route53:::change/*"]
  }

  # List* APIs generally require '*' resources.
  statement {
    sid    = "Route53List"
    effect = "Allow"
    actions = [
      "route53:ListHostedZones",
      "route53:ListHostedZonesByName",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "dns_admin" {
  count = var.enable_compute && var.dns_admin_enabled && (var.create_hosted_zone || var.dns_admin_zone_id != "") ? 1 : 0

  name_prefix = "${var.name_prefix}-dnsadmin-"
  description = "Allow administering Route53 records in the hosted zone (for certbot DNS-01, etc.)."
  policy      = data.aws_iam_policy_document.dns_admin[0].json

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "dns_admin" {
  count      = var.enable_compute && var.dns_admin_enabled && (var.create_hosted_zone || var.dns_admin_zone_id != "") ? 1 : 0
  role       = aws_iam_role.ssm[0].name
  policy_arn = aws_iam_policy.dns_admin[0].arn
}

resource "aws_s3_bucket" "assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  bucket        = local.assets_bucket_name
  force_destroy = var.assets_bucket_force_destroy

  tags = merge(local.tags, { Name = local.assets_bucket_name })
}

resource "aws_s3_bucket_public_access_block" "assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  bucket = aws_s3_bucket.assets[0].id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_ownership_controls" "assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  bucket = aws_s3_bucket.assets[0].id

  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  bucket = aws_s3_bucket.assets[0].id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_versioning" "assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  bucket = aws_s3_bucket.assets[0].id

  versioning_configuration {
    status = "Enabled"
  }
}

data "aws_iam_policy_document" "cicd_assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  statement {
    sid    = "ListAssetsBucket"
    effect = "Allow"
    actions = [
      "s3:ListBucket",
      "s3:ListBucketMultipartUploads",
    ]
    resources = [aws_s3_bucket.assets[0].arn]
  }

  statement {
    sid    = "ReadWriteAssetsObjects"
    effect = "Allow"
    actions = [
      "s3:GetObject",
      "s3:PutObject",
      "s3:DeleteObject",
      "s3:AbortMultipartUpload",
      "s3:ListMultipartUploadParts",
      "s3:CreateMultipartUpload",
      "s3:UploadPart",
      "s3:CompleteMultipartUpload",
    ]
    resources = ["${aws_s3_bucket.assets[0].arn}/*"]
  }
}

resource "aws_iam_policy" "cicd_assets" {
  count = var.enable_compute && var.assets_bucket_enabled ? 1 : 0

  name_prefix = "${var.name_prefix}-cicd-assets-"
  description = "Allow CI/CD on the instance to read/write build assets in the S3 bucket."
  policy      = data.aws_iam_policy_document.cicd_assets[0].json

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "cicd_assets" {
  count      = var.enable_compute && var.assets_bucket_enabled ? 1 : 0
  role       = aws_iam_role.ssm[0].name
  policy_arn = aws_iam_policy.cicd_assets[0].arn
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

resource "aws_route53_record" "www_cname" {
  # Count cannot depend on resource attributes. If compute is enabled, we create the record
  # and let the target resolve to the instance's public DNS after apply.
  count = var.create_hosted_zone && (var.enable_compute || var.cname_target != "" || var.www_cname_target != "") ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = "www.${trim(var.zone_name, ".")}"
  type    = "CNAME"
  ttl     = 60
  records = [
    var.www_cname_target != "" ? var.www_cname_target : (var.cname_target != "" ? var.cname_target : aws_instance.this[0].public_dns),
  ]
}

resource "aws_route53_record" "apex_a" {
  count = var.create_hosted_zone && var.enable_compute ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = trim(var.zone_name, ".")
  type    = "A"
  ttl     = 60
  records = [aws_instance.this[0].public_ip]
}
