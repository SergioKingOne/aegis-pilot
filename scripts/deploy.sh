#!/bin/bash
# scripts/deploy.sh

set -e

echo "🚀 Starting Disaster Recovery deployment..."

# Build Lambda functions
echo "📦 Building Lambda functions..."
cd lambda-functions

for func in health-check backup-manager failover-controller data-validator; do
    echo "Building $func..."
    cd $func
    cargo lambda build --release
    cd ..
done
cd ..

# Deploy Primary Region
echo "☁️ Deploying Primary Region (us-east-1)..."
sam deploy \
    --template-file cloudformation/primary-region.yaml \
    --stack-name dr-demo-primary \
    --capabilities CAPABILITY_IAM \
    --region us-east-1 \
    --resolve-s3

# Deploy DR Region
echo "☁️ Deploying DR Region (us-west-2)..."
sam deploy \
    --template-file cloudformation/dr-region.yaml \
    --stack-name dr-demo-dr \
    --capabilities CAPABILITY_IAM \
    --region us-west-2 \
    --resolve-s3

# Enable Global Tables
echo "🌐 Configuring DynamoDB Global Tables..."
aws dynamodb create-global-table \
    --global-table-name dr-application-table \
    --replication-group RegionName=us-east-1 RegionName=us-west-2 \
    --region us-east-1 || true

aws dynamodb create-global-table \
    --global-table-name dr-sentinel-table \
    --replication-group RegionName=us-east-1 RegionName=us-west-2 \
    --region us-east-1 || true

echo "✅ Deployment complete!"
echo "📊 View CloudWatch Dashboard: https://console.aws.amazon.com/cloudwatch/"
echo "🔍 Test failover: ./scripts/test-failover.sh" 