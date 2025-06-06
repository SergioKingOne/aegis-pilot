#!/bin/bash
# scripts/test-failover.sh

set -e

echo "🧪 Testing Disaster Recovery Failover..."

# Insert test data
echo "📝 Inserting test data..."
aws dynamodb put-item \
    --table-name dr-application-table \
    --item '{"id": {"S": "test-123"}, "data": {"S": "Test data for DR"}, "timestamp": {"N": "'$(date +%s)'"}}' \
    --region us-east-1

# Trigger health check
echo "🏥 Running health check..."
aws lambda invoke \
    --function-name dr-health-check \
    --payload '{"region": "us-east-1"}' \
    --region us-east-1 \
    response.json

cat response.json | jq .

# Simulate failover
echo "🔄 Simulating failover to DR region..."
aws lambda invoke \
    --function-name dr-failover-controller \
    --payload '{"action": "failover", "target_region": "us-west-2"}' \
    --region us-east-1 \
    failover-response.json

cat failover-response.json | jq .

# Verify data in DR region
echo "✅ Verifying data in DR region..."
aws dynamodb get-item \
    --table-name dr-application-table \
    --key '{"id": {"S": "test-123"}}' \
    --region us-west-2 | jq .

echo "🎉 Failover test complete!" 