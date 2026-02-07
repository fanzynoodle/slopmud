#!/usr/bin/env bash
set -euo pipefail

region="${AWS_REGION:-us-east-1}"
account_id="$(aws sts get-caller-identity --query Account --output text)"

bucket="tfstate-${account_id}-${region}-slopmud"
lock_table="tf-locks-${account_id}-${region}-slopmud"

echo "region=$region"
echo "bucket=$bucket"
echo "lock_table=$lock_table"

if aws s3api head-bucket --bucket "$bucket" 2>/dev/null; then
  echo "State bucket exists."
else
  echo "Creating state bucket..."
  if [[ "$region" == "us-east-1" ]]; then
    aws s3api create-bucket --bucket "$bucket" >/dev/null
  else
    aws s3api create-bucket --bucket "$bucket" --create-bucket-configuration LocationConstraint="$region" >/dev/null
  fi

  aws s3api put-public-access-block --bucket "$bucket" --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true >/dev/null

  aws s3api put-bucket-versioning --bucket "$bucket" --versioning-configuration Status=Enabled >/dev/null

  aws s3api put-bucket-encryption --bucket "$bucket" --server-side-encryption-configuration \
    '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}' >/dev/null
fi

if aws dynamodb describe-table --table-name "$lock_table" --region "$region" >/dev/null 2>&1; then
  echo "Lock table exists."
else
  echo "Creating lock table..."
  aws dynamodb create-table \
    --table-name "$lock_table" \
    --attribute-definitions AttributeName=LockID,AttributeType=S \
    --key-schema AttributeName=LockID,KeyType=HASH \
    --billing-mode PAY_PER_REQUEST \
    --region "$region" >/dev/null

  aws dynamodb wait table-exists --table-name "$lock_table" --region "$region"
fi

echo "Done."

