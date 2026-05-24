output "availability_zone" {
  value = var.availability_zone
}

output "subnet_id" {
  value = local.subnet_id
}

output "raft_subnet_ids" {
  value = local.raft_subnet_ids
}

output "raft_availability_zones" {
  value = local.raft_availability_zones
}

output "gateway_public_ip" {
  value = aws_instance.gateway.public_ip
}

output "gateway_public_dns" {
  value = aws_instance.gateway.public_dns
}

output "gateway_private_ip" {
  value = aws_instance.gateway.private_ip
}

output "raft_dns_names" {
  value = local.raft_dns_names
}

output "route53_zone_id" {
  value = var.route53_zone_id
}

output "raft_instance_ids" {
  value = local.raft_current_instance_ids
}

output "raft_autoscaling_group_names" {
  value = {
    for idx, group in aws_autoscaling_group.raft : local.node_ids[idx] => group.name
  }
}

output "raft_launch_template_ids" {
  value = {
    for idx, template in aws_launch_template.raft : local.node_ids[idx] => template.id
  }
}

output "gateway_data_volume_id" {
  value = var.data_volume_enabled ? aws_ebs_volume.gateway_data[0].id : ""
}

output "raft_data_volume_ids" {
  value = [for v in aws_ebs_volume.raft_data : v.id]
}

output "assets_bucket_name" {
  value = local.assets_bucket_name
}

output "wal_backup_bucket_name" {
  value = local.wal_backup_bucket_name
}

output "s3_gateway_endpoint_id" {
  value = try(aws_vpc_endpoint.s3[0].id, "")
}

output "ec2_interface_endpoint_id" {
  value = try(aws_vpc_endpoint.ec2[0].id, "")
}

output "shard_addrs" {
  value = join(",", [for name in local.raft_dns_names : "${name}:${var.shard_port}"])
}

output "raft_rpc_addrs" {
  value = join(",", [for name in local.raft_dns_names : "${name}:${var.raft_rpc_port}"])
}

output "raft_node_ids" {
  value = join(",", local.node_ids)
}

output "recommended_env" {
  description = "Non-secret env values to combine with env/prd.env-style secrets for split deploy scripts."
  value = {
    HOST                              = aws_instance.gateway.public_dns
    GATEWAY_HOST                      = aws_instance.gateway.public_dns
    SSH_PORT                          = "22"
    REMOTE_ROOT                       = "/opt/slopmud"
    SLOPMUD_BIND                      = "0.0.0.0:${var.gateway_bind_port}"
    SHARD_ADDRS                       = join(",", [for name in local.raft_dns_names : "${name}:${var.shard_port}"])
    SHARD_NODE_HOSTS                  = join(",", local.raft_dns_names)
    SHARD_NODE_IDS                    = join(",", local.node_ids)
    SHARD_PORT                        = tostring(var.shard_port)
    SHARD_RAFT_PORT                   = tostring(var.raft_rpc_port)
    SHARD_RAFT_NODE_IDS               = join(",", local.node_ids)
    SHARD_TRIO_BINDS                  = join(",", [for _ in local.raft_dns_names : "0.0.0.0:${var.shard_port}"])
    SHARD_TRIO_RAFT_BINDS             = join(",", [for _ in local.raft_dns_names : "0.0.0.0:${var.raft_rpc_port}"])
    SHARD_TRIO_RAFT_PEERS             = join(",", [for idx, name in local.raft_dns_names : "${local.node_ids[idx]}@${name}:${var.raft_rpc_port}"])
    SINGLE_AZ_RAFT_SUBNET             = local.subnet_id
    SINGLE_AZ_RAFT_AZ                 = var.availability_zone
    RAFT_SUBNETS                      = join(",", local.raft_subnet_ids)
    RAFT_AVAILABILITY_ZONES           = join(",", local.raft_availability_zones)
    SINGLE_AZ_RAFT_BACKHAUL           = "private-vpc"
    ASSETS_BUCKET                     = local.assets_bucket_name
    SLOPMUD_WAL_BACKUP_ENABLED        = "1"
    SLOPMUD_WAL_BACKUP_DIR            = "/opt/slopmud/state/walbackup"
    SLOPMUD_WAL_BACKUP_INTERVAL_S     = "60"
    SLOPMUD_WAL_BACKUP_S3_BUCKET      = local.wal_backup_bucket_name
    SLOPMUD_WAL_BACKUP_S3_PREFIX      = local.wal_backup_s3_prefix
    SLOPMUD_WAL_BACKUP_UPLOAD_ENABLED = "1"
    SLOPMUD_WAL_RESTORE_ENABLED       = "auto"
    SLOPMUD_WAL_RESTORE_CACHE_DIR     = "/opt/slopmud/state/walrestore-cache"
    SLOPMUD_WAL_RESTORE_MISSING_OK    = "1"
  }
}

output "ssh_examples" {
  value = merge(
    {
      gateway = "ssh admin@${aws_instance.gateway.public_dns}"
    },
    {
      for idx, name in local.raft_dns_names :
      "raft_${local.node_ids[idx]}" => "ssh -J admin@${aws_instance.gateway.public_dns} admin@${name}"
    }
  )
}
