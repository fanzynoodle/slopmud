output "account_id" {
  value = data.aws_caller_identity.current.account_id
}

output "instance_ids" {
  value = [for i in aws_instance.spot : i.id]
}

output "instance_states" {
  value = [for i in aws_instance.spot : i.instance_state]
}

output "public_ips" {
  value = [for i in aws_instance.spot : i.public_ip]
}

output "private_ips" {
  value = [for i in aws_instance.spot : i.private_ip]
}

