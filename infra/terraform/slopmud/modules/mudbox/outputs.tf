output "account_id" {
  value = data.aws_caller_identity.current.account_id
}

output "instance_id" {
  value = try(aws_instance.this[0].id, null)
}

output "public_ip" {
  value = try(aws_instance.this[0].public_ip, null)
}

output "public_dns" {
  value = try(aws_instance.this[0].public_dns, null)
}

output "ssh_allowed_cidr" {
  value = var.ssh_allowed_cidr
}

output "hosted_zone_id" {
  value = var.create_hosted_zone ? aws_route53_zone.this[0].zone_id : null
}

output "hosted_zone_name_servers" {
  value = var.create_hosted_zone ? aws_route53_zone.this[0].name_servers : []
}

output "mud_fqdn" {
  value = var.create_hosted_zone && length(aws_route53_record.mud_cname) > 0 ? aws_route53_record.mud_cname[0].fqdn : null
}

output "www_fqdn" {
  value = var.create_hosted_zone && length(aws_route53_record.www_cname) > 0 ? aws_route53_record.www_cname[0].fqdn : null
}

output "assets_bucket_name" {
  value = try(aws_s3_bucket.assets[0].bucket, null)
}

output "assets_bucket_arn" {
  value = try(aws_s3_bucket.assets[0].arn, null)
}
