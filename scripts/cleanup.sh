#!/bin/bash
# scripts/cleanup.sh

set -e

echo "🧹 Cleaning up Disaster Recovery resources..."

# Load environment variables
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Remove global table replicas first
echo "🌐 Removing Global Table replicas..."
for table in dr-application-table dr-sentinel-table dr-backup-metadata; do
    echo "Removing replica for $table..."
    aws dynamodb update-table \
        --table-name $table \
        --replica-updates "Delete={RegionName=$AWS_DR_REGION}" \
        --region $AWS_DEFAULT_REGION 2>/dev/null || echo "Replica already removed or table doesn't exist"
done

# Wait for replicas to be removed
echo "⏳ Waiting for replicas to be removed..."
sleep 30

# Get actual bucket names from stacks
echo "🗑️ Emptying S3 buckets..."
PRIMARY_BUCKET=$(aws cloudformation describe-stacks \
    --stack-name $PRIMARY_STACK_NAME \
    --region $AWS_DEFAULT_REGION \
    --query 'Stacks[0].Outputs[?OutputKey==`BackupBucketName`].OutputValue' \
    --output text 2>/dev/null || echo "")

DR_BUCKET=$(aws cloudformation describe-stacks \
    --stack-name $DR_STACK_NAME \
    --region $AWS_DR_REGION \
    --query 'Stacks[0].Outputs[?OutputKey==`DRBackupBucketName`].OutputValue' \
    --output text 2>/dev/null || echo "")

if [ ! -z "$PRIMARY_BUCKET" ]; then
    echo "Emptying primary bucket: $PRIMARY_BUCKET"
    aws s3 rm s3://$PRIMARY_BUCKET --recursive 2>/dev/null || true
fi

if [ ! -z "$DR_BUCKET" ]; then
    echo "Emptying DR bucket: $DR_BUCKET"
    aws s3 rm s3://$DR_BUCKET --recursive 2>/dev/null || true
fi

# Delete CloudFormation stacks
echo "☁️ Deleting CloudFormation stacks..."
aws cloudformation delete-stack --stack-name $DR_STACK_NAME --region $AWS_DR_REGION 2>/dev/null || echo "DR stack already deleted"
aws cloudformation delete-stack --stack-name $PRIMARY_STACK_NAME --region $AWS_DEFAULT_REGION 2>/dev/null || echo "Primary stack already deleted"

# Wait for stack deletion
echo "⏳ Waiting for stacks to be deleted..."
aws cloudformation wait stack-delete-complete --stack-name $DR_STACK_NAME --region $AWS_DR_REGION 2>/dev/null || echo "DR stack deletion complete or timed out"
aws cloudformation wait stack-delete-complete --stack-name $PRIMARY_STACK_NAME --region $AWS_DEFAULT_REGION 2>/dev/null || echo "Primary stack deletion complete or timed out"

# Clean up any remaining DynamoDB tables
echo "🗄️ Cleaning up remaining DynamoDB tables..."
for table in dr-application-table dr-sentinel-table dr-backup-metadata; do
    aws dynamodb delete-table --table-name $table --region $AWS_DEFAULT_REGION 2>/dev/null || echo "Table $table already deleted"
done

echo "✅ Cleanup complete!"
