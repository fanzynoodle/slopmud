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
  subnet_ids                 = sort(data.aws_subnets.default.ids)
  ami_id                     = var.os == "al2023" ? data.aws_ssm_parameter.al2023_ami[0].value : data.aws_ami.debian12[0].id
  assets_bucket_name         = var.assets_bucket_name != "" ? var.assets_bucket_name : "slopmud-assets-${data.aws_caller_identity.current.account_id}-${var.region}"
  zone_apex                  = trim(var.zone_name, ".")
  compute_public_dns         = try(data.aws_instance.active[0].public_dns, "")
  compute_public_ip          = try(data.aws_instance.active[0].public_ip, "")
  default_cname_target       = var.cname_target != "" ? var.cname_target : local.compute_public_dns
  mud_cname_target_effective = var.mud_cname_target != "" ? var.mud_cname_target : local.default_cname_target
  www_cname_target_effective = var.www_cname_target != "" ? var.www_cname_target : local.mud_cname_target_effective
  extra_cname_names          = distinct(concat(var.extra_cname_record_names, keys(var.extra_cname_targets)))
  extra_cname_target_map = {
    for name in local.extra_cname_names :
    name => lookup(var.extra_cname_targets, name, local.default_cname_target)
    if lookup(var.extra_cname_targets, name, local.default_cname_target) != ""
  }
  apex_a_records_effective = length(var.apex_a_records) > 0 ? var.apex_a_records : (
    var.create_apex_a_record && local.compute_public_ip != "" ? [local.compute_public_ip] : []
  )
  dns_registration_names = distinct(compact(concat(
    var.enable_compute && var.mud_cname_target == "" && var.cname_target == "" ? ["${var.record_name}.${trim(var.zone_name, ".")}"] : [],
    var.enable_compute && var.www_cname_target == "" && var.mud_cname_target == "" && var.cname_target == "" ? ["www.${trim(var.zone_name, ".")}"] : [],
    [for name in local.extra_cname_names : var.enable_compute && lookup(var.extra_cname_targets, name, "") == "" && var.cname_target == "" ? "${name}.${trim(var.zone_name, ".")}" : ""],
  )))
  apex_tracks_compute = var.enable_compute && var.create_apex_a_record && length(var.apex_a_records) == 0
  tags = {
    ManagedBy = "terraform"
    Stack     = var.name_prefix
  }

  dns_registration_script = var.dns_admin_enabled && (var.create_hosted_zone || var.dns_admin_zone_id != "") ? trimspace(<<-EOT
    #!/usr/bin/env bash
    set -uo pipefail

    ZONE_ID="${local.dns_admin_zone_id}"
    REGION="${var.region}"
    APEX="${local.zone_apex}"
    APEX_ENABLED="${local.apex_tracks_compute ? "1" : "0"}"
    CNAME_LIST=$(cat <<'NAMES'
    ${join("\n", local.dns_registration_names)}
    NAMES
    )

    if ! command -v aws >/dev/null 2>&1; then
      if command -v apt-get >/dev/null 2>&1; then
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -y && apt-get install -y awscli || true
      elif command -v dnf >/dev/null 2>&1; then
        dnf -y install awscli || true
      fi
    fi

    if ! command -v aws >/dev/null 2>&1; then
      echo "aws cli is unavailable; skipping dns registration"
      exit 0
    fi

    PUBLIC_IP=""
    PUBLIC_DNS=""
    for _ in $(seq 1 30); do
      token=$(curl -fsS -m 2 -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600" || true)
      if [ -n "$token" ]; then
        PUBLIC_IP=$(curl -fsS -m 2 -H "X-aws-ec2-metadata-token: $token" "http://169.254.169.254/latest/meta-data/public-ipv4" || true)
        PUBLIC_DNS=$(curl -fsS -m 2 -H "X-aws-ec2-metadata-token: $token" "http://169.254.169.254/latest/meta-data/public-hostname" || true)
      fi
      if [ -n "$PUBLIC_IP" ] && [ -n "$PUBLIC_DNS" ]; then
        break
      fi
      sleep 2
    done

    if [ -z "$PUBLIC_IP" ] || [ -z "$PUBLIC_DNS" ]; then
      echo "could not determine public metadata; skipping dns registration"
      exit 0
    fi

    upsert_rr() {
      name="$1"
      rtype="$2"
      value="$3"
      change_batch=$(cat <<JSON
    {"Changes":[{"Action":"UPSERT","ResourceRecordSet":{"Name":"$name","Type":"$rtype","TTL":60,"ResourceRecords":[{"Value":"$value"}]}}]}
    JSON
    )
      aws route53 change-resource-record-sets \
        --region "$REGION" \
        --hosted-zone-id "$ZONE_ID" \
        --change-batch "$change_batch" >/dev/null
    }

    if [ "$APEX_ENABLED" = "1" ]; then
      upsert_rr "$APEX" "A" "$PUBLIC_IP"
    fi
    while IFS= read -r cname; do
      [ -z "$cname" ] && continue
      upsert_rr "$cname" "CNAME" "$PUBLIC_DNS"
    done <<< "$CNAME_LIST"

    echo "registered dns: ip=$PUBLIC_IP dns=$PUBLIC_DNS"
  EOT
  ) : ""

  boot_restore_bundle_uri = var.boot_restore_enabled ? "s3://${local.assets_bucket_name}/bootstrap/${var.name_prefix}/rebootstrap.tgz" : ""
  user_data_script = (local.dns_registration_script != "" || local.boot_restore_bundle_uri != "") ? templatefile("${path.module}/user_data.sh.tftpl", {
    name_prefix             = var.name_prefix
    region                  = var.region
    assets_bucket_name      = local.assets_bucket_name
    dns_registration_script = local.dns_registration_script
    restore_bundle_uri      = local.boot_restore_bundle_uri
    restore_track           = var.boot_restore_track
    restore_env_prefix      = var.boot_restore_env_prefix
  }) : null
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

locals {
  ssm_read_param_names = distinct(concat(
    var.ssm_read_parameter_names,
    (var.compliance_portal_config_json_ssm_name != "" ? [var.compliance_portal_config_json_ssm_name] : []),
  ))

  ssm_read_param_arns = [
    for n in local.ssm_read_param_names :
    "arn:aws:ssm:${var.region}:${data.aws_caller_identity.current.account_id}:parameter/${trim(n, "/")}"
  ]
}

data "aws_iam_policy_document" "ssm_read_params" {
  count = var.enable_compute && length(local.ssm_read_param_arns) > 0 ? 1 : 0

  statement {
    sid    = "SsmReadParameters"
    effect = "Allow"
    actions = [
      "ssm:GetParameter",
      "ssm:GetParameters",
      "ssm:GetParameterHistory",
    ]
    resources = local.ssm_read_param_arns
  }

  # Needed for SecureString (including aws/ssm managed key); scope via ViaService.
  statement {
    sid    = "KmsDecryptForSsm"
    effect = "Allow"
    actions = [
      "kms:Decrypt",
    ]
    resources = ["*"]
    condition {
      test     = "StringEquals"
      variable = "kms:ViaService"
      values   = ["ssm.${var.region}.amazonaws.com"]
    }
    condition {
      test     = "StringEquals"
      variable = "kms:CallerAccount"
      values   = [data.aws_caller_identity.current.account_id]
    }
  }
}

resource "aws_iam_policy" "ssm_read_params" {
  count = var.enable_compute && length(local.ssm_read_param_arns) > 0 ? 1 : 0

  name_prefix = "${var.name_prefix}-ssmread-"
  description = "Allow the instance to read specific SSM parameters (for app secrets)."
  policy      = data.aws_iam_policy_document.ssm_read_params[0].json

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "ssm_read_params" {
  count = var.enable_compute && length(local.ssm_read_param_arns) > 0 ? 1 : 0

  role       = aws_iam_role.ssm[0].name
  policy_arn = aws_iam_policy.ssm_read_params[0].arn
}

resource "aws_ssm_parameter" "compliance_portal_config_json" {
  count = var.enable_compute && var.compliance_portal_config_json_ssm_name != "" && var.compliance_portal_config_json_ssm_value != "" ? 1 : 0

  name        = var.compliance_portal_config_json_ssm_name
  description = "slopmud compliance portal config JSON"
  type        = "SecureString"
  value       = var.compliance_portal_config_json_ssm_value
  overwrite   = true

  tags = local.tags
}

data "aws_iam_policy_document" "ses_send" {
  count = var.enable_compute && var.ses_send_enabled ? 1 : 0

  statement {
    sid    = "SesSendEmail"
    effect = "Allow"
    actions = [
      "ses:SendEmail",
      "ses:SendRawEmail",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "ses_send" {
  count = var.enable_compute && var.ses_send_enabled ? 1 : 0

  name_prefix = "${var.name_prefix}-ses-send-"
  description = "Allow the instance role to send email via Amazon SES."
  policy      = data.aws_iam_policy_document.ses_send[0].json

  tags = local.tags
}

resource "aws_iam_role_policy_attachment" "ses_send" {
  count = var.enable_compute && var.ses_send_enabled ? 1 : 0

  role       = aws_iam_role.ssm[0].name
  policy_arn = aws_iam_policy.ses_send[0].arn
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

resource "aws_launch_template" "this" {
  count = var.enable_compute ? 1 : 0

  name_prefix   = "${var.name_prefix}-"
  image_id      = local.ami_id
  instance_type = var.instance_type
  key_name      = var.ssh_public_key_path != "" ? aws_key_pair.this[0].key_name : null

  iam_instance_profile {
    name = aws_iam_instance_profile.ssm[0].name
  }

  network_interfaces {
    associate_public_ip_address = var.associate_public_ip_address
    security_groups             = [aws_security_group.this[0].id]
  }

  user_data = local.user_data_script != null ? base64encode(local.user_data_script) : null

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

  block_device_mappings {
    device_name = "/dev/sda1"

    ebs {
      volume_type           = "gp3"
      volume_size           = var.root_volume_gib
      delete_on_termination = true
      encrypted             = true
    }
  }

  tag_specifications {
    resource_type = "instance"
    tags          = merge(local.tags, { Name = var.name_prefix })
  }

  tag_specifications {
    resource_type = "volume"
    tags          = local.tags
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_autoscaling_group" "this" {
  count = var.enable_compute ? 1 : 0

  name_prefix         = "${var.name_prefix}-"
  min_size            = 1
  max_size            = 1
  desired_capacity    = 1
  health_check_type   = "EC2"
  vpc_zone_identifier = [element(local.subnet_ids, 0)]
  force_delete        = false

  launch_template {
    id      = aws_launch_template.this[0].id
    version = "$Latest"
  }

  tag {
    key                 = "Name"
    value               = var.name_prefix
    propagate_at_launch = true
  }

  tag {
    key                 = "ManagedBy"
    value               = "terraform"
    propagate_at_launch = true
  }

  tag {
    key                 = "Stack"
    value               = var.name_prefix
    propagate_at_launch = true
  }

  depends_on = [aws_launch_template.this]
}

data "aws_instances" "asg_instances" {
  count = var.enable_compute ? 1 : 0

  instance_state_names = ["pending", "running"]

  filter {
    name   = "tag:aws:autoscaling:groupName"
    values = [aws_autoscaling_group.this[0].name]
  }
}

data "aws_instance" "active" {
  count = var.enable_compute ? 1 : 0

  instance_id = sort(data.aws_instances.asg_instances[0].ids)[0]
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
  count = var.create_hosted_zone && local.mud_cname_target_effective != "" ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = "${var.record_name}.${trim(var.zone_name, ".")}"
  type    = "CNAME"
  ttl     = 60
  records = [local.mud_cname_target_effective]
}

resource "aws_route53_record" "www_cname" {
  count = var.create_hosted_zone && local.www_cname_target_effective != "" ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = "www.${trim(var.zone_name, ".")}"
  type    = "CNAME"
  ttl     = 60
  records = [local.www_cname_target_effective]
}

resource "aws_route53_record" "extra_cnames" {
  for_each = var.create_hosted_zone ? local.extra_cname_target_map : {}

  zone_id = local.hosted_zone_id
  name    = "${each.key}.${trim(var.zone_name, ".")}"
  type    = "CNAME"
  ttl     = 60
  records = [each.value]
}

resource "aws_route53_record" "apex_a" {
  count = var.create_hosted_zone && length(local.apex_a_records_effective) > 0 ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = trim(var.zone_name, ".")
  type    = "A"
  ttl     = 60
  records = local.apex_a_records_effective
}

resource "aws_route53_record" "sbc_enable_a" {
  count = var.create_hosted_zone && var.sbc_enable_dns_record_name != "" ? 1 : 0

  zone_id = local.hosted_zone_id
  name    = "${var.sbc_enable_dns_record_name}.${trim(var.zone_name, ".")}"
  type    = "A"
  ttl     = var.sbc_enable_dns_record_ttl
  records = [
    var.sbc_enable_dns_record_enabled ? var.sbc_enable_dns_record_ip : var.sbc_enable_dns_record_disabled_ip,
  ]
}
