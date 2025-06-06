#!/bin/bash
# scripts/cleanup.sh

set -e

echo "🧹 Cleaning up Disaster Recovery resources..."

# Remove global table configuration
echo "🌐 Removing Global Table configuration..."
aws dynamodb update-table \
    --table-name dr-application-table \
    --replica-updates 'DeleteRegion={RegionName=us-west-2}' \
    --region us-east-1 || true

aws dynamodb update-table \
    --table-name dr-sentinel-table \
    --replica-updates 'DeleteRegion={RegionName=us-west-2}' \
    --region us-east-1 || true

# Empty S3 buckets
echo "🗑️ Emptying S3 buckets..."
aws s3 rm s3://dr-demo-backup-bucket-primary --recursive || true
aws s3 rm s3://dr-demo-backup-bucket-dr --recursive || true

# Delete CloudFormation stacks
echo "☁️ Deleting CloudFormation stacks..."
aws cloudformation delete-stack --stack-name dr-demo-dr --region us-west-2 || true
aws cloudformation delete-stack --stack-name dr-demo-primary --region us-east-1 || true

# Wait for stack deletion
echo "⏳ Waiting for stacks to be deleted..."
aws cloudformation wait stack-delete-complete --stack-name dr-demo-dr --region us-west-2 || true
aws cloudformation wait stack-delete-complete --stack-name dr-demo-primary --region us-east-1 || true

echo "✅ Cleanup complete!" 