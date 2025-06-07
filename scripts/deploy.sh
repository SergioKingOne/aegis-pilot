#!/bin/bash
# scripts/deploy.sh

set -e

echo "🚀 Starting Disaster Recovery deployment..."

# Load environment variables
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Get AWS Account ID
AWS_ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

# Build Lambda functions
echo "📦 Building Lambda functions..."
cd lambda-functions

for func in health-check backup-manager failover-controller data-validator; do
    echo "Building $func..."
    cd $func
    cargo lambda build --release --arm64
    
    # Create the expected directory structure for SAM
    mkdir -p target/lambda/$func
    cp ../../target/lambda/${func}-bootstrap/bootstrap target/lambda/$func/
    
    cd ..
done
cd ..

# Create S3 bucket for SAM deployments if it doesn't exist
DEPLOYMENT_BUCKET="aegis-pilot-sam-deployments-${AWS_ACCOUNT_ID:-349622182257}"
echo "🪣 Checking deployment bucket: $DEPLOYMENT_BUCKET"
aws s3 mb s3://$DEPLOYMENT_BUCKET --region $AWS_DEFAULT_REGION 2>/dev/null || echo "Bucket already exists"

echo "🪣 Creating DR deployment bucket..."
aws s3 mb s3://$DEPLOYMENT_BUCKET-dr --region $AWS_DR_REGION 2>/dev/null || echo "DR bucket already exists"

# Deploy Primary Region
echo "☁️ Deploying Primary Region ($AWS_DEFAULT_REGION)..."
sam deploy \
    --template-file cloudformation/primary-region.yaml \
    --stack-name $PRIMARY_STACK_NAME \
    --capabilities CAPABILITY_IAM \
    --region $AWS_DEFAULT_REGION \
    --s3-bucket $DEPLOYMENT_BUCKET \
    --no-confirm-changeset \
    --no-fail-on-empty-changeset

# Get outputs from primary stack
PRIMARY_OUTPUTS=$(aws cloudformation describe-stacks \
    --stack-name $PRIMARY_STACK_NAME \
    --region $AWS_DEFAULT_REGION \
    --query 'Stacks[0].Outputs' \
    --output json)

echo "Primary stack outputs:"
echo "$PRIMARY_OUTPUTS"

# Deploy DR Region
echo "☁️ Deploying DR Region ($AWS_DR_REGION)..."
sam deploy \
    --template-file cloudformation/dr-region.yaml \
    --stack-name $DR_STACK_NAME \
    --capabilities CAPABILITY_IAM \
    --region $AWS_DR_REGION \
    --s3-bucket $DEPLOYMENT_BUCKET-dr \
    --no-confirm-changeset \
    --no-fail-on-empty-changeset

# Wait for stacks to be ready
echo "⏳ Waiting for stacks to be ready..."
aws cloudformation wait stack-create-complete \
    --stack-name $PRIMARY_STACK_NAME \
    --region $AWS_DEFAULT_REGION || true

aws cloudformation wait stack-create-complete \
    --stack-name $DR_STACK_NAME \
    --region $AWS_DR_REGION || true

# Enable Global Tables (if not already enabled)
echo "🌐 Configuring DynamoDB Global Tables..."
# Check if global table already exists
GLOBAL_TABLE_EXISTS=$(aws dynamodb describe-table \
    --table-name $DYNAMODB_APP_TABLE \
    --region $AWS_DEFAULT_REGION \
    --query 'Table.StreamSpecification.StreamEnabled' \
    --output text 2>/dev/null || echo "false")

if [ "$GLOBAL_TABLE_EXISTS" != "true" ]; then
    echo "Creating global tables..."
    # Note: For new global tables, you need to use the CreateTable API with Replicas parameter
    # For existing tables, we'll skip this step as it requires recreating the tables
    echo "⚠️  Note: Manual configuration of global tables may be required for existing tables"
fi



# Configure S3 Cross-Region Replication
echo "🔄 Configuring S3 Cross-Region Replication..."
# Get the replication role ARN from stack outputs
REPLICATION_ROLE_ARN=$(aws cloudformation describe-stacks \
    --stack-name $PRIMARY_STACK_NAME \
    --region $AWS_DEFAULT_REGION \
    --query 'Stacks[0].Outputs[?OutputKey==`S3ReplicationRoleArn`].OutputValue' \
    --output text)

if [ ! -z "$REPLICATION_ROLE_ARN" ]; then
    cat > /tmp/replication-config.json <<EOF
{
    "Role": "$REPLICATION_ROLE_ARN",
    "Rules": [
        {
            "ID": "ReplicateToDR",
            "Status": "Enabled",
            "Priority": 1,
            "DeleteMarkerReplication": { "Status": "Enabled" },
            "Filter": {},
            "Destination": {
                "Bucket": "arn:aws:s3:::dr-demo-backup-bucket-primary-dr",
                "ReplicationTime": {
                    "Status": "Enabled",
                    "Time": { "Minutes": 15 }
                },
                "Metrics": {
                    "Status": "Enabled",
                    "EventThreshold": { "Minutes": 15 }
                }
            }
        }
    ]
}
EOF

    # Enable versioning on both buckets (required for replication)
    aws s3api put-bucket-versioning \
        --bucket dr-demo-backup-bucket-primary \
        --versioning-configuration Status=Enabled \
        --region $AWS_DEFAULT_REGION || echo "Versioning already enabled"

    aws s3api put-bucket-versioning \
        --bucket dr-demo-backup-bucket-primary-dr \
        --versioning-configuration Status=Enabled \
        --region $AWS_DR_REGION || echo "Versioning already enabled"

    # Apply replication configuration
    aws s3api put-bucket-replication \
        --bucket dr-demo-backup-bucket-primary \
        --replication-configuration file:///tmp/replication-config.json \
        --region $AWS_DEFAULT_REGION || echo "⚠️  Replication configuration may need manual setup"
fi

echo "✅ Deployment complete!"
echo "📊 View CloudWatch Dashboard: https://console.aws.amazon.com/cloudwatch/home?region=$AWS_DEFAULT_REGION"
echo "🔍 Test deployment: ./scripts/test-deploy.sh"
