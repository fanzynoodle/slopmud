#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
USAGE:
  reconcile_raft_dns.sh --terraform-dir infra/terraform/single-az-raft-us-east-1 [--zone-id ZONE] [--ttl 30] [--no-wait]

Refreshes stable Raft DNS records from current Auto Scaling group membership.
Terraform supplies slot names, ASG names, and DNS names only; private IPs are
looked up live from AWS and are never rendered into the runtime env.
EOF
}

terraform_dir="infra/terraform/single-az-raft-us-east-1"
zone_id="${ROUTE53_ZONE_ID:-}"
ttl="${RAFT_DNS_TTL:-30}"
wait_for_insync="${RAFT_DNS_WAIT:-1}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --terraform-dir)
      terraform_dir="${2:-}"
      shift 2
      ;;
    --zone-id)
      zone_id="${2:-}"
      shift 2
      ;;
    --ttl)
      ttl="${2:-}"
      shift 2
      ;;
    --no-wait)
      wait_for_insync=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

for c in aws jq terraform; do
  if ! command -v "$c" >/dev/null 2>&1; then
    echo "ERROR: $c is required" >&2
    exit 2
  fi
done

if ! [[ "$ttl" =~ ^[0-9]+$ ]] || [[ "$ttl" -lt 1 ]]; then
  echo "ERROR: --ttl must be a positive integer" >&2
  exit 2
fi

asg_json="$(terraform -chdir="$terraform_dir" output -json raft_autoscaling_group_names)"
dns_json="$(terraform -chdir="$terraform_dir" output -json raft_dns_names)"
if [[ -z "$zone_id" ]]; then
  zone_id="$(terraform -chdir="$terraform_dir" output -raw route53_zone_id 2>/dev/null || true)"
fi
if [[ -z "$zone_id" ]]; then
  echo "ERROR: route53 zone id is required; pass --zone-id or set route53_zone_id in Terraform" >&2
  exit 2
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

node_ids=(n0 n1 n2)
for idx in "${!node_ids[@]}"; do
  node_id="${node_ids[$idx]}"
  asg_name="$(jq -r --arg node "$node_id" '.[$node] // empty' <<<"$asg_json")"
  dns_name="$(jq -r --argjson idx "$idx" '.[$idx] // empty' <<<"$dns_json")"
  if [[ -z "$asg_name" || -z "$dns_name" ]]; then
    echo "ERROR: Terraform output is missing ASG or DNS name for ${node_id}" >&2
    exit 1
  fi

  instance_id=""
  for lifecycle_state in InService Pending; do
    instance_id="$(aws autoscaling describe-auto-scaling-groups \
      --auto-scaling-group-names "$asg_name" \
      --query "AutoScalingGroups[0].Instances[?LifecycleState==\`${lifecycle_state}\`].InstanceId | [0]" \
      --output text)"
    if [[ -n "$instance_id" && "$instance_id" != "None" ]]; then
      break
    fi
    instance_id=""
  done
  if [[ -z "$instance_id" ]]; then
    echo "ERROR: no active instance found for ${node_id} ASG ${asg_name}" >&2
    exit 1
  fi

  private_ip="$(aws ec2 describe-instances \
    --instance-ids "$instance_id" \
    --query 'Reservations[0].Instances[0].PrivateIpAddress' \
    --output text)"
  if [[ -z "$private_ip" || "$private_ip" == "None" ]]; then
    echo "ERROR: no private IP found for ${node_id} instance ${instance_id}" >&2
    exit 1
  fi

  jq -n \
    --arg name "$dns_name" \
    --arg value "$private_ip" \
    --argjson ttl "$ttl" \
    '{Changes:[{Action:"UPSERT",ResourceRecordSet:{Name:$name,Type:"A",TTL:$ttl,ResourceRecords:[{Value:$value}]}}]}' \
    >"$tmp"
  change_id="$(aws route53 change-resource-record-sets \
    --hosted-zone-id "$zone_id" \
    --change-batch "file://${tmp}" \
    --query 'ChangeInfo.Id' \
    --output text)"
  if [[ "$wait_for_insync" == "1" ]]; then
    aws route53 wait resource-record-sets-changed --id "$change_id"
  fi
  printf '%s %s -> %s (%s)\n' "$node_id" "$dns_name" "$private_ip" "$instance_id"
done
