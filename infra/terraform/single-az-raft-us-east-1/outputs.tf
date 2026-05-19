output "availability_zone" {
  value = var.availability_zone
}

output "subnet_id" {
  value = local.subnet_id
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

output "raft_private_ips" {
  value = [for i in aws_instance.raft : i.private_ip]
}

output "raft_instance_ids" {
  value = [for i in aws_instance.raft : i.id]
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

output "s3_gateway_endpoint_id" {
  value = var.s3_gateway_endpoint_enabled ? aws_vpc_endpoint.s3[0].id : ""
}

output "shard_addrs" {
  value = join(",", [for i in aws_instance.raft : "${i.private_ip}:${var.shard_port}"])
}

output "raft_rpc_addrs" {
  value = join(",", [for i in aws_instance.raft : "${i.private_ip}:${var.raft_rpc_port}"])
}

output "raft_node_ids" {
  value = join(",", local.node_ids)
}

output "recommended_env" {
  description = "Non-secret env values to combine with env/prd.env-style secrets for split deploy scripts."
  value = {
    HOST                    = aws_instance.gateway.public_dns
    GATEWAY_HOST            = aws_instance.gateway.public_dns
    SSH_PORT                = "22"
    REMOTE_ROOT             = "/opt/slopmud"
    SLOPMUD_BIND            = "0.0.0.0:4200"
    SHARD_ADDRS             = join(",", [for i in aws_instance.raft : "${i.private_ip}:${var.shard_port}"])
    SHARD_NODE_HOSTS        = join(",", [for i in aws_instance.raft : i.private_ip])
    SHARD_NODE_IDS          = join(",", local.node_ids)
    SHARD_PORT              = tostring(var.shard_port)
    SHARD_RAFT_PORT         = tostring(var.raft_rpc_port)
    SHARD_RAFT_NODE_IDS     = join(",", local.node_ids)
    SHARD_TRIO_BINDS        = join(",", [for i in aws_instance.raft : "0.0.0.0:${var.shard_port}"])
    SHARD_TRIO_RAFT_BINDS   = join(",", [for i in aws_instance.raft : "0.0.0.0:${var.raft_rpc_port}"])
    SHARD_TRIO_RAFT_PEERS   = join(",", [for idx, i in aws_instance.raft : "${local.node_ids[idx]}@${i.private_ip}:${var.raft_rpc_port}"])
    SINGLE_AZ_RAFT_SUBNET   = local.subnet_id
    SINGLE_AZ_RAFT_AZ       = var.availability_zone
    SINGLE_AZ_RAFT_BACKHAUL = "private-vpc"
    ASSETS_BUCKET           = local.assets_bucket_name
  }
}

output "ssh_examples" {
  value = {
    gateway = "ssh admin@${aws_instance.gateway.public_dns}"
    raft_n0 = "ssh -J admin@${aws_instance.gateway.public_dns} admin@${aws_instance.raft[0].private_ip}"
    raft_n1 = "ssh -J admin@${aws_instance.gateway.public_dns} admin@${aws_instance.raft[1].private_ip}"
    raft_n2 = "ssh -J admin@${aws_instance.gateway.public_dns} admin@${aws_instance.raft[2].private_ip}"
  }
}
