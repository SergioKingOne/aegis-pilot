# Disaster Recovery Testing Procedures

This document outlines the procedures for testing the disaster recovery setup to ensure it functions as expected during an actual disaster.

## Types of Tests

We conduct three types of DR testing:

1. **Component Testing**: Testing individual components of the DR solution
2. **Scenario Testing**: Testing specific failure scenarios
3. **Full DR Test**: Complete failover and failback testing

## Test Schedule

| Test Type         | Frequency | Duration  | Impact                      |
| ----------------- | --------- | --------- | --------------------------- |
| Component Testing | Weekly    | 1-2 hours | No production impact        |
| Scenario Testing  | Monthly   | 3-4 hours | Minimal production impact   |
| Full DR Test      | Quarterly | 8 hours   | Temporary production impact |

## Component Testing

### DynamoDB Replication Test

Verify that DynamoDB Global Tables are replicating correctly:

```bash
# 1. Write test data to primary region
aws dynamodb put-item \
    --table-name dr-application-table \
    --item '{"id": {"S": "dr-test-'$(date +%s)'"}, "data": {"S": "DR test data"}}' \
    --region us-east-1

# 2. Wait for replication (typically 1-2 seconds)
sleep 5

# 3. Read from DR region to verify replication
aws dynamodb scan \
    --table-name dr-application-table \
    --filter-expression "begins_with(id, :prefix)" \
    --expression-attribute-values '{":prefix": {"S": "dr-test-"}}' \
    --region us-west-2
```

### S3 Replication Test

Verify S3 Cross-Region Replication is working:

```bash
# 1. Upload test file to primary bucket
echo "DR test file $(date)" > test-file.txt
aws s3 cp test-file.txt s3://dr-demo-backup-bucket-primary/dr-testing/test-file.txt

# 2. Wait for replication (may take a few minutes)
sleep 120

# 3. Verify file exists in DR bucket
aws s3 ls s3://dr-demo-backup-bucket-dr/dr-testing/test-file.txt
```

### Health Check Lambda Test

Verify health check lambda is correctly reporting status:

```bash
# Invoke health check lambda and check response
aws lambda invoke \
    --function-name dr-health-check \
    --payload '{"region": "us-east-1"}' \
    --region us-east-1 \
    health-check-response.json

cat health-check-response.json | jq .
```

## Scenario Testing

### Network Partition Simulation

Simulate a network partition between regions:

1. Temporarily modify security groups to block cross-region traffic
2. Observe replication lag increase
3. Verify monitoring alerts trigger appropriately
4. Restore connectivity and verify recovery

### Database Failure Simulation

Simulate a DynamoDB failure:

1. Create a CloudWatch alarm with a custom metric to simulate failure
2. Trigger the alarm manually
3. Verify that the monitoring system detects the "failure"
4. Verify that the failover decision process is initiated
5. Cancel the alarm before actual failover occurs

## Full DR Test

### Preparation

1. Schedule the test at least 2 weeks in advance
2. Notify all stakeholders
3. Create a detailed test plan with success criteria
4. Prepare rollback procedures
5. Ensure all monitoring systems are working

### Execution

1. Follow the Failover Procedure in the runbook.md document
2. Record the time taken for each step
3. Verify all applications are functioning in the DR region
4. Run application-specific tests to verify functionality
5. Maintain the DR environment for at least 1 hour
6. Follow the Failback Procedure in the runbook.md document
7. Verify successful return to primary region

### Post-Test Activities

1. Document all issues encountered
2. Measure actual RTO/RPO achieved
3. Compare with target RTO/RPO
4. Update procedures based on lessons learned
5. Present findings to stakeholders

## Test Documentation

For each test, document the following:

- Date and time of test
- Test participants
- Test type and scenario
- Steps performed
- Actual RTO/RPO achieved
- Issues encountered
- Lessons learned
- Action items for improvement

## Success Criteria

A successful DR test must meet these criteria:

1. All data is successfully replicated to DR region
2. All applications are functional in DR region
3. RTO is within 10 minutes
4. RPO is within 1 minute
5. Successful failback to primary region
6. No data loss during failover/failback

## Continuous Improvement

After each test:

1. Update runbooks based on findings
2. Improve monitoring and alerting
3. Optimize failover procedures
4. Reduce RTO/RPO if possible
5. Automate more of the process 