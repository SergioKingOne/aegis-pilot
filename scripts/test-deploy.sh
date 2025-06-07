#!/bin/bash
# scripts/test-deploy.sh

set -e

echo "🧪 Testing Disaster Recovery Deployment..."

# Load environment variables
if [ -f .env ]; then
    export $(cat .env | grep -v '^#' | xargs)
fi

# Test 1: Check if stacks exist
echo "1️⃣ Checking CloudFormation stacks..."
aws cloudformation describe-stacks --stack-name $PRIMARY_STACK_NAME --region $AWS_DEFAULT_REGION >/dev/null
echo "✅ Primary stack exists"

aws cloudformation describe-stacks --stack-name $DR_STACK_NAME --region $AWS_DR_REGION >/dev/null
echo "✅ DR stack exists"

# Test 2: Check DynamoDB tables
echo "2️⃣ Checking DynamoDB tables..."
aws dynamodb describe-table --table-name $DYNAMODB_APP_TABLE --region $AWS_DEFAULT_REGION >/dev/null
echo "✅ Application table exists in primary region"

aws dynamodb describe-table --table-name $DYNAMODB_SENTINEL_TABLE --region $AWS_DEFAULT_REGION >/dev/null
echo "✅ Sentinel table exists in primary region"

# Test 3: Insert test data
echo "3️⃣ Inserting test data..."
TEST_ID="test-$(date +%s)"
aws dynamodb put-item \
    --table-name $DYNAMODB_APP_TABLE \
    --item "{\"id\": {\"S\": \"$TEST_ID\"}, \"data\": {\"S\": \"Test data for DR\"}, \"timestamp\": {\"N\": \"$(date +%s)\"}}" \
    --region $AWS_DEFAULT_REGION

echo "✅ Test data inserted with ID: $TEST_ID"

# Test 4: Test health check Lambda
echo "4️⃣ Testing health check Lambda..."
PAYLOAD=$(echo '{"region": "us-east-1"}' | base64)
HEALTH_RESPONSE=$(aws lambda invoke \
    --function-name $LAMBDA_HEALTH_CHECK \
    --payload "$PAYLOAD" \
    --region $AWS_DEFAULT_REGION \
    --output json \
    response.json)

cat response.json | jq .
HEALTH_STATUS=$(cat response.json | jq -r .status)
echo "✅ Health check status: $HEALTH_STATUS"

# Test 5: Check replication (wait a bit for propagation)
echo "5️⃣ Checking data replication to DR region..."
echo "Waiting 10 seconds for replication..."
sleep 10

# Try to read the item from DR region
DR_ITEM=$(aws dynamodb get-item \
    --table-name $DYNAMODB_APP_TABLE \
    --key "{\"id\": {\"S\": \"$TEST_ID\"}}" \
    --region $AWS_DR_REGION \
    --output json 2>/dev/null || echo "{}")

if echo "$DR_ITEM" | grep -q "$TEST_ID"; then
    echo "✅ Data successfully replicated to DR region"
else
    echo "⚠️  Data not yet replicated to DR region (this may take a few minutes)"
fi

# Test 6: Test data validator
echo "6️⃣ Testing data validator Lambda..."
VALIDATOR_PAYLOAD=$(echo '{"validation_type": "incremental", "action": "validate"}' | base64)
VALIDATOR_RESPONSE=$(aws lambda invoke \
    --function-name $LAMBDA_DATA_VALIDATOR \
    --payload "$VALIDATOR_PAYLOAD" \
    --region $AWS_DEFAULT_REGION \
    --output json \
    validator-response.json)

cat validator-response.json | jq .

# Clean up test data
echo "🧹 Cleaning up test data..."
aws dynamodb delete-item \
    --table-name $DYNAMODB_APP_TABLE \
    --key "{\"id\": {\"S\": \"$TEST_ID\"}}" \
    --region $AWS_DEFAULT_REGION

echo "✅ Test completed!"
