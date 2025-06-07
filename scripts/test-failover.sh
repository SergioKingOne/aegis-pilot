#!/bin/bash
# scripts/test-failover.sh

set -e

# Define colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Load environment variables from .env file
if [ -f .env ]; then
    echo -e "${YELLOW}Loading configuration from .env file...${NC}"
    export $(cat .env | grep -v '#' | xargs)
else
    echo -e "${RED}Warning: .env file not found. Using default configuration.${NC}"
    echo -e "${YELLOW}You can create a .env file based on .env.example${NC}"
fi

# Set default values if not provided in .env
AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION:-us-east-1}
AWS_DR_REGION=${AWS_DR_REGION:-us-west-2}
DYNAMODB_APP_TABLE=${DYNAMODB_APP_TABLE:-dr-application-table}
LAMBDA_HEALTH_CHECK=${LAMBDA_HEALTH_CHECK:-dr-health-check}
LAMBDA_FAILOVER_CONTROLLER=${LAMBDA_FAILOVER_CONTROLLER:-dr-failover-controller}

echo "🧪 Testing Disaster Recovery Failover..."

# Insert test data
echo "📝 Inserting test data..."
aws dynamodb put-item \
    --table-name ${DYNAMODB_APP_TABLE} \
    --item '{"id": {"S": "test-123"}, "data": {"S": "Test data for DR"}, "timestamp": {"N": "'$(date +%s)'"}}' \
    --region ${AWS_DEFAULT_REGION}

# Trigger health check
echo "🏥 Running health check..."
HEALTH_PAYLOAD=$(echo '{"region": "'${AWS_DEFAULT_REGION}'"}' | base64)
aws lambda invoke \
    --function-name ${LAMBDA_HEALTH_CHECK} \
    --payload "$HEALTH_PAYLOAD" \
    --region ${AWS_DEFAULT_REGION} \
    response.json

cat response.json | jq .

# Simulate failover
echo "🔄 Simulating failover to DR region..."
FAILOVER_PAYLOAD=$(echo '{"action": "failover", "target_region": "'${AWS_DR_REGION}'"}' | base64)
aws lambda invoke \
    --function-name ${LAMBDA_FAILOVER_CONTROLLER} \
    --payload "$FAILOVER_PAYLOAD" \
    --region ${AWS_DEFAULT_REGION} \
    failover-response.json

cat failover-response.json | jq .

# Verify data in DR region
echo "✅ Verifying data in DR region..."
aws dynamodb get-item \
    --table-name ${DYNAMODB_APP_TABLE} \
    --key '{"id": {"S": "test-123"}}' \
    --region ${AWS_DR_REGION} | jq .

echo -e "${GREEN}🎉 Failover test complete!${NC}" 