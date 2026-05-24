provider "aws" {
  region = var.region
}

data "aws_caller_identity" "current" {}

data "aws_partition" "current" {}

data "aws_vpc" "default" {
  count   = var.vpc_id == "" ? 1 : 0
  default = true
}

locals {
  vpc_id                 = var.vpc_id != "" ? var.vpc_id : data.aws_vpc.default[0].id
  assets_bucket_name     = var.assets_bucket_name != "" ? var.assets_bucket_name : "slopmud-assets-${data.aws_caller_identity.current.account_id}-${var.region}"
  wal_backup_bucket_name = var.wal_backup_bucket_name != "" ? var.wal_backup_bucket_name : local.assets_bucket_name
  wal_backup_s3_prefix   = "${var.name_prefix}/wal-backups"
  node_ids               = ["n0", "n1", "n2"]
  raft_dns_zone_name     = trimsuffix(var.route53_zone_name, ".")
  raft_dns_names         = [for node_id in local.node_ids : "${var.raft_dns_record_prefix}-${node_id}.${local.raft_dns_zone_name}"]
  gateway_dns_policy     = var.route53_zone_id != "" && (var.dns_upsert_enabled || var.raft_dns_reconcile_enabled)
  raft_data_volume_arns  = [for volume in aws_ebs_volume.raft_data : "arn:${data.aws_partition.current.partition}:ec2:${var.region}:${data.aws_caller_identity.current.account_id}:volume/${volume.id}"]
  account_instance_arn   = "arn:${data.aws_partition.current.partition}:ec2:${var.region}:${data.aws_caller_identity.current.account_id}:instance/*"
  tags = {
    ManagedBy = "terraform"
    Stack     = var.name_prefix
    Topology  = "single-az-gateway-raft"
  }
}

data "aws_subnets" "selected_az" {
  filter {
    name   = "vpc-id"
    values = [local.vpc_id]
  }

  filter {
    name   = "availability-zone"
    values = [var.availability_zone]
  }
}

data "aws_subnets" "raft_az" {
  count = length(var.raft_subnet_ids) == 0 && length(var.raft_availability_zones) == 3 ? 3 : 0

  filter {
    name   = "vpc-id"
    values = [local.vpc_id]
  }

  filter {
    name   = "availability-zone"
    values = [var.raft_availability_zones[count.index]]
  }
}

locals {
  subnet_id = var.subnet_id != "" ? var.subnet_id : sort(data.aws_subnets.selected_az.ids)[0]
  raft_subnet_ids = length(var.raft_subnet_ids) == 3 ? var.raft_subnet_ids : (
    length(var.raft_availability_zones) == 3 ? [
      for idx in range(length(local.node_ids)) : sort(data.aws_subnets.raft_az[idx].ids)[0]
      ] : [
      for _ in local.node_ids : local.subnet_id
    ]
  )
}

data "aws_subnet" "selected" {
  id = local.subnet_id
}

data "aws_subnet" "raft" {
  count = length(local.node_ids)

  id = local.raft_subnet_ids[count.index]
}

data "aws_route_tables" "vpc" {
  vpc_id = local.vpc_id
}

locals {
  raft_availability_zones = [for subnet in data.aws_subnet.raft : subnet.availability_zone]
}

check "explicit_raft_placement_uses_distinct_azs" {
  assert {
    condition = (
      length(var.raft_availability_zones) == 0 && length(var.raft_subnet_ids) == 0
    ) || length(distinct(local.raft_availability_zones)) == length(local.node_ids)
    error_message = "Explicit Raft placement must put n0,n1,n2 in three distinct availability zones."
  }
}

check "raft_asg_does_not_pin_private_ips" {
  assert {
    condition     = length(var.raft_private_ips) == 0
    error_message = "Raft Auto Scaling groups cannot pin private IPs in launch templates; leave raft_private_ips empty and use the stable Raft DNS records."
  }
}

data "aws_ami" "debian12" {
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

resource "aws_key_pair" "deploy" {
  count = var.ssh_public_key_path != "" ? 1 : 0

  key_name_prefix = "${var.name_prefix}-"
  public_key      = file(var.ssh_public_key_path)

  tags = local.tags
}

resource "aws_security_group" "gateway" {
  name_prefix = "${var.name_prefix}-gateway-"
  description = "Public telnet/web/websocket gateway; private egress to Raft nodes."
  vpc_id      = local.vpc_id

  ingress {
    description = "SSH from operator CIDR"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.ssh_allowed_cidr]
  }

  dynamic "ingress" {
    for_each = toset(var.gateway_public_tcp_ports)
    content {
      description = "Gateway public TCP ${ingress.value}"
      from_port   = ingress.value
      to_port     = ingress.value
      protocol    = "tcp"
      cidr_blocks = ["0.0.0.0/0"]
    }
  }

  egress {
    description = "Allow all egress"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(local.tags, { Name = "${var.name_prefix}-gateway-sg" })
}

resource "aws_security_group" "raft" {
  name_prefix = "${var.name_prefix}-raft-"
  description = "Private Raft shard nodes; no public ingress."
  vpc_id      = local.vpc_id

  egress {
    description = "Allow all egress"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(local.tags, { Name = "${var.name_prefix}-raft-sg" })
}

resource "aws_security_group" "interface_endpoint" {
  count = var.ec2_interface_endpoint_enabled ? 1 : 0

  name_prefix = "${var.name_prefix}-vpce-"
  description = "Interface VPC endpoints used by private Raft nodes."
  vpc_id      = local.vpc_id

  ingress {
    description     = "HTTPS from private Raft nodes"
    from_port       = 443
    to_port         = 443
    protocol        = "tcp"
    security_groups = [aws_security_group.raft.id]
  }

  egress {
    description = "Allow endpoint egress"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(local.tags, { Name = "${var.name_prefix}-vpce-sg" })
}

resource "aws_security_group_rule" "raft_ssh_from_gateway" {
  type                     = "ingress"
  security_group_id        = aws_security_group.raft.id
  source_security_group_id = aws_security_group.gateway.id
  protocol                 = "tcp"
  from_port                = 22
  to_port                  = 22
  description              = "SSH from gateway only"
}

resource "aws_security_group_rule" "raft_app_from_gateway" {
  type                     = "ingress"
  security_group_id        = aws_security_group.raft.id
  source_security_group_id = aws_security_group.gateway.id
  protocol                 = "tcp"
  from_port                = var.shard_port
  to_port                  = var.shard_port
  description              = "Broker on gateway to shard app port"
}

resource "aws_security_group_rule" "raft_rpc_from_gateway" {
  type                     = "ingress"
  security_group_id        = aws_security_group.raft.id
  source_security_group_id = aws_security_group.gateway.id
  protocol                 = "tcp"
  from_port                = var.raft_rpc_port
  to_port                  = var.raft_rpc_port
  description              = "Gateway health/debug access to Raft RPC"
}

resource "aws_security_group_rule" "raft_rpc_self" {
  type              = "ingress"
  security_group_id = aws_security_group.raft.id
  self              = true
  protocol          = "tcp"
  from_port         = var.raft_rpc_port
  to_port           = var.raft_rpc_port
  description       = "Raft peer replication inside one security group"
}

resource "aws_security_group_rule" "raft_app_self" {
  type              = "ingress"
  security_group_id = aws_security_group.raft.id
  self              = true
  protocol          = "tcp"
  from_port         = var.shard_port
  to_port           = var.shard_port
  description       = "Shard app peer/admin access inside Raft group"
}

resource "aws_iam_role" "gateway" {
  name_prefix = "${var.name_prefix}-gateway-"

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

resource "aws_iam_role_policy_attachment" "gateway_ssm_core" {
  role       = aws_iam_role.gateway.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "gateway" {
  name_prefix = "${var.name_prefix}-gateway-"
  role        = aws_iam_role.gateway.name
  tags        = local.tags
}

data "aws_iam_policy_document" "gateway_assets" {
  statement {
    sid    = "ListAssetsBucket"
    effect = "Allow"
    actions = [
      "s3:ListBucket",
    ]
    resources = ["arn:aws:s3:::${local.assets_bucket_name}"]
  }

  statement {
    sid    = "ReadAssetsObjects"
    effect = "Allow"
    actions = [
      "s3:GetObject",
    ]
    resources = ["arn:aws:s3:::${local.assets_bucket_name}/*"]
  }
}

resource "aws_iam_policy" "gateway_assets" {
  name_prefix = "${var.name_prefix}-assets-read-"
  description = "Allow gateway to fetch release artifacts."
  policy      = data.aws_iam_policy_document.gateway_assets.json
  tags        = local.tags
}

resource "aws_iam_role_policy_attachment" "gateway_assets" {
  role       = aws_iam_role.gateway.name
  policy_arn = aws_iam_policy.gateway_assets.arn
}

resource "aws_iam_role_policy_attachment" "raft_assets" {
  role       = aws_iam_role.raft.name
  policy_arn = aws_iam_policy.gateway_assets.arn
}

data "aws_iam_policy_document" "raft_wal_backups" {
  statement {
    sid    = "ListWalBackupBucket"
    effect = "Allow"
    actions = [
      "s3:ListBucket",
    ]
    resources = ["arn:aws:s3:::${local.wal_backup_bucket_name}"]
  }

  statement {
    sid    = "ReadWriteWalBackupObjects"
    effect = "Allow"
    actions = [
      "s3:GetObject",
      "s3:PutObject",
      "s3:AbortMultipartUpload",
      "s3:ListMultipartUploadParts",
    ]
    resources = ["arn:aws:s3:::${local.wal_backup_bucket_name}/${local.wal_backup_s3_prefix}/*"]
  }
}

resource "aws_iam_policy" "raft_wal_backups" {
  name_prefix = "${var.name_prefix}-wal-backups-"
  description = "Allow Raft nodes to stream WAL backups to S3 and restore them after replacement."
  policy      = data.aws_iam_policy_document.raft_wal_backups.json
  tags        = local.tags
}

resource "aws_iam_role_policy_attachment" "raft_wal_backups" {
  role       = aws_iam_role.raft.name
  policy_arn = aws_iam_policy.raft_wal_backups.arn
}

data "aws_iam_policy_document" "raft_data_volume_attach" {
  count = var.data_volume_enabled ? 1 : 0

  statement {
    sid    = "DescribeRaftDataVolumes"
    effect = "Allow"
    actions = [
      "ec2:DescribeInstances",
      "ec2:DescribeVolumes",
      "ec2:DescribeVolumeStatus",
    ]
    resources = ["*"]
  }

  statement {
    sid    = "AttachRaftSlotDataVolume"
    effect = "Allow"
    actions = [
      "ec2:AttachVolume",
      "ec2:CreateTags",
    ]
    resources = concat(local.raft_data_volume_arns, [local.account_instance_arn])
  }
}

resource "aws_iam_policy" "raft_data_volume_attach" {
  count = var.data_volume_enabled ? 1 : 0

  name_prefix = "${var.name_prefix}-raft-data-attach-"
  description = "Allow Raft replacement instances to attach their explicit slot data volume."
  policy      = data.aws_iam_policy_document.raft_data_volume_attach[0].json
  tags        = local.tags
}

resource "aws_iam_role_policy_attachment" "raft_data_volume_attach" {
  count = var.data_volume_enabled ? 1 : 0

  role       = aws_iam_role.raft.name
  policy_arn = aws_iam_policy.raft_data_volume_attach[0].arn
}

data "aws_iam_policy_document" "gateway_ssm_read" {
  count = length(var.ssm_read_parameter_names) > 0 ? 1 : 0

  statement {
    sid    = "SsmReadParameters"
    effect = "Allow"
    actions = [
      "ssm:GetParameter",
      "ssm:GetParameters",
      "ssm:GetParameterHistory",
    ]
    resources = [
      for n in var.ssm_read_parameter_names :
      "arn:aws:ssm:${var.region}:${data.aws_caller_identity.current.account_id}:parameter/${trim(n, "/")}"
    ]
  }

  statement {
    sid       = "KmsDecryptForSsm"
    effect    = "Allow"
    actions   = ["kms:Decrypt"]
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

resource "aws_iam_policy" "gateway_ssm_read" {
  count = length(var.ssm_read_parameter_names) > 0 ? 1 : 0

  name_prefix = "${var.name_prefix}-ssmread-"
  description = "Allow gateway to read app secrets from SSM."
  policy      = data.aws_iam_policy_document.gateway_ssm_read[0].json
  tags        = local.tags
}

resource "aws_iam_role_policy_attachment" "gateway_ssm_read" {
  count = length(var.ssm_read_parameter_names) > 0 ? 1 : 0

  role       = aws_iam_role.gateway.name
  policy_arn = aws_iam_policy.gateway_ssm_read[0].arn
}

data "aws_iam_policy_document" "gateway_dns" {
  count = local.gateway_dns_policy ? 1 : 0

  statement {
    sid    = "Route53ZoneAdmin"
    effect = "Allow"
    actions = [
      "route53:ChangeResourceRecordSets",
      "route53:GetHostedZone",
      "route53:ListResourceRecordSets",
    ]
    resources = ["arn:aws:route53:::hostedzone/${var.route53_zone_id}"]
  }

  statement {
    sid       = "Route53ChangeRead"
    effect    = "Allow"
    actions   = ["route53:GetChange"]
    resources = ["arn:aws:route53:::change/*"]
  }

  statement {
    sid    = "RaftDnsDiscoveryRead"
    effect = "Allow"
    actions = [
      "autoscaling:DescribeAutoScalingGroups",
      "ec2:DescribeInstances",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "gateway_dns" {
  count = local.gateway_dns_policy ? 1 : 0

  name_prefix = "${var.name_prefix}-dns-"
  description = "Allow gateway to upsert cutover DNS records."
  policy      = data.aws_iam_policy_document.gateway_dns[0].json
  tags        = local.tags
}

resource "aws_iam_role_policy_attachment" "gateway_dns" {
  count = local.gateway_dns_policy ? 1 : 0

  role       = aws_iam_role.gateway.name
  policy_arn = aws_iam_policy.gateway_dns[0].arn
}

resource "aws_iam_role" "raft" {
  name_prefix = "${var.name_prefix}-raft-"

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

resource "aws_iam_role_policy_attachment" "raft_ssm_core" {
  role       = aws_iam_role.raft.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_instance_profile" "raft" {
  name_prefix = "${var.name_prefix}-raft-"
  role        = aws_iam_role.raft.name
  tags        = local.tags
}

resource "aws_vpc_endpoint" "s3" {
  count = var.s3_gateway_endpoint_enabled ? 1 : 0

  vpc_id            = local.vpc_id
  service_name      = "com.amazonaws.${var.region}.s3"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = data.aws_route_tables.vpc.ids

  tags = merge(local.tags, {
    Name = "${var.name_prefix}-s3-gateway"
    Role = "release-artifact-fetch"
  })
}

resource "aws_vpc_endpoint" "ec2" {
  count = var.ec2_interface_endpoint_enabled ? 1 : 0

  vpc_id              = local.vpc_id
  service_name        = "com.amazonaws.${var.region}.ec2"
  vpc_endpoint_type   = "Interface"
  subnet_ids          = distinct(local.raft_subnet_ids)
  security_group_ids  = [aws_security_group.interface_endpoint[0].id]
  private_dns_enabled = true

  tags = merge(local.tags, {
    Name = "${var.name_prefix}-ec2-interface"
    Role = "raft-volume-attach"
  })
}

locals {
  dns_record_words  = join(" ", var.dns_record_names)
  gateway_user_data = <<-EOT
    #!/usr/bin/env bash
    set -euxo pipefail

    if command -v apt-get >/dev/null 2>&1; then
      export DEBIAN_FRONTEND=noninteractive
      apt-get update -y
      apt-get install -y ca-certificates awscli rsync curl
    fi

    if ! id -u slopmud >/dev/null 2>&1; then
      useradd --system --home /opt/slopmud --create-home --shell /usr/sbin/nologin slopmud
    fi
    install -d -m 0755 -o slopmud -g slopmud /opt/slopmud /opt/slopmud/bin /opt/slopmud/var /opt/slopmud/assets /opt/slopmud/web_homepage

    if [ "${var.dns_upsert_enabled ? "1" : "0"}" = "1" ] && [ -n "${var.route53_zone_id}" ]; then
      token="$(curl -fsS -m 2 -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600" || true)"
      PUBLIC_IP=""
      PUBLIC_DNS=""
      if [ -n "$token" ]; then
        PUBLIC_IP="$(curl -fsS -m 2 -H "X-aws-ec2-metadata-token: $token" "http://169.254.169.254/latest/meta-data/public-ipv4" || true)"
        PUBLIC_DNS="$(curl -fsS -m 2 -H "X-aws-ec2-metadata-token: $token" "http://169.254.169.254/latest/meta-data/public-hostname" || true)"
      fi
      if [ -n "$PUBLIC_IP" ] && [ -n "$PUBLIC_DNS" ]; then
        first="1"
        for name in ${local.dns_record_words}; do
          [ -z "$name" ] && continue
          if [ "$first" = "1" ]; then
            rtype="A"
            value="$PUBLIC_IP"
            first="0"
          else
            rtype="CNAME"
            value="$PUBLIC_DNS"
          fi
          change_batch="{\"Changes\":[{\"Action\":\"UPSERT\",\"ResourceRecordSet\":{\"Name\":\"$name\",\"Type\":\"$rtype\",\"TTL\":60,\"ResourceRecords\":[{\"Value\":\"$value\"}]}}]}"
          aws route53 change-resource-record-sets --region "${var.region}" --hosted-zone-id "${var.route53_zone_id}" --change-batch "$change_batch" >/dev/null
        done
      fi
    fi
  EOT

}

resource "aws_instance" "gateway" {
  ami                         = data.aws_ami.debian12.id
  instance_type               = var.gateway_instance_type
  subnet_id                   = local.subnet_id
  vpc_security_group_ids      = [aws_security_group.gateway.id]
  iam_instance_profile        = aws_iam_instance_profile.gateway.name
  associate_public_ip_address = true
  key_name                    = var.ssh_public_key_path != "" ? aws_key_pair.deploy[0].key_name : null
  user_data_replace_on_change = true
  user_data                   = local.gateway_user_data

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.gateway_root_volume_gib
    delete_on_termination = true
    encrypted             = true
  }

  lifecycle {
    # The gateway is the stable player-facing endpoint. Node replacement and
    # AMI refreshes happen through the Raft ASGs; do not roll this instance as a
    # side effect of unrelated topology applies.
    ignore_changes = [
      ami,
      user_data,
      root_block_device,
    ]
  }

  tags = merge(local.tags, {
    Name = "${var.name_prefix}-gateway"
    Role = "gateway"
  })
}

resource "aws_ebs_volume" "gateway_data" {
  count = var.data_volume_enabled ? 1 : 0

  availability_zone = var.availability_zone
  size              = var.gateway_data_volume_gib
  type              = var.data_volume_type
  encrypted         = true

  tags = merge(local.tags, {
    Name = "${var.name_prefix}-gateway-data"
    Role = "gateway-data"
  })
}

resource "aws_volume_attachment" "gateway_data" {
  count = var.data_volume_enabled ? 1 : 0

  device_name  = "/dev/sdf"
  volume_id    = aws_ebs_volume.gateway_data[0].id
  instance_id  = aws_instance.gateway.id
  force_detach = true
}

resource "aws_ebs_volume" "raft_data" {
  count = var.data_volume_enabled ? 3 : 0

  availability_zone = local.raft_availability_zones[count.index]
  size              = var.raft_data_volume_gib
  type              = var.data_volume_type
  encrypted         = true

  tags = merge(local.tags, {
    Name = "${var.name_prefix}-raft-${local.node_ids[count.index]}-data"
    Role = "raft-data"
    Node = local.node_ids[count.index]
  })
}

resource "aws_launch_template" "raft" {
  count = 3

  name_prefix   = "${var.name_prefix}-raft-${local.node_ids[count.index]}-"
  image_id      = data.aws_ami.debian12.id
  instance_type = var.raft_instance_type
  key_name      = var.ssh_public_key_path != "" ? aws_key_pair.deploy[0].key_name : null
  user_data = base64encode(templatefile("${path.module}/userdata_raft_node.sh.tftpl", {
    data_volume_enabled = var.data_volume_enabled ? "1" : "0"
    data_volume_id      = var.data_volume_enabled ? aws_ebs_volume.raft_data[count.index].id : ""
    mount_path          = "/opt/slopmud/var"
    node_id             = local.node_ids[count.index]
    region              = var.region
  }))

  iam_instance_profile {
    name = aws_iam_instance_profile.raft.name
  }

  network_interfaces {
    associate_public_ip_address = false
    security_groups             = [aws_security_group.raft.id]
  }

  instance_market_options {
    market_type = "spot"

    spot_options {
      instance_interruption_behavior = "terminate"
      max_price                      = var.raft_spot_max_price != "" ? var.raft_spot_max_price : null
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
      volume_size           = var.raft_root_volume_gib
      delete_on_termination = true
      encrypted             = true
    }
  }

  tag_specifications {
    resource_type = "instance"
    tags = merge(local.tags, {
      Name = "${var.name_prefix}-raft-${local.node_ids[count.index]}"
      Role = "raft"
      Node = local.node_ids[count.index]
    })
  }

  tag_specifications {
    resource_type = "volume"
    tags = merge(local.tags, {
      Role = "raft-root"
      Node = local.node_ids[count.index]
    })
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_autoscaling_group" "raft" {
  count = 3

  name_prefix               = "${var.name_prefix}-raft-${local.node_ids[count.index]}-"
  min_size                  = 1
  max_size                  = 1
  desired_capacity          = 1
  health_check_type         = "EC2"
  health_check_grace_period = var.raft_health_check_grace_period_seconds
  vpc_zone_identifier       = [local.raft_subnet_ids[count.index]]
  wait_for_capacity_timeout = var.raft_wait_for_capacity_timeout

  launch_template {
    id      = aws_launch_template.raft[count.index].id
    version = "$Latest"
  }

  tag {
    key                 = "Name"
    value               = "${var.name_prefix}-raft-${local.node_ids[count.index]}"
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

  tag {
    key                 = "Topology"
    value               = local.tags.Topology
    propagate_at_launch = true
  }

  tag {
    key                 = "Role"
    value               = "raft"
    propagate_at_launch = true
  }

  tag {
    key                 = "Node"
    value               = local.node_ids[count.index]
    propagate_at_launch = true
  }

  depends_on = [aws_iam_role_policy_attachment.raft_data_volume_attach]
}

data "aws_instances" "raft" {
  count = 3

  instance_state_names = ["pending", "running"]

  filter {
    name   = "tag:aws:autoscaling:groupName"
    values = [aws_autoscaling_group.raft[count.index].name]
  }

  depends_on = [aws_autoscaling_group.raft]
}

locals {
  raft_current_instance_ids = [for idx in range(length(local.node_ids)) : try(data.aws_instances.raft[idx].ids[0], "")]
}
