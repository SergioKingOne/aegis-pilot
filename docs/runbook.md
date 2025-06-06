# Disaster Recovery Runbook

## Overview

This runbook provides step-by-step procedures for managing the AWS Disaster Recovery system. It covers normal operations, failover procedures, and recovery processes.

## Table of Contents

1. [System Architecture](#system-architecture)
2. [Normal Operations](#normal-operations)
3. [Monitoring and Alerts](#monitoring-and-alerts)
4. [Failover Procedures](#failover-procedures)
5. [Failback Procedures](#failback-procedures)
6. [Testing Procedures](#testing-procedures)
7. [Troubleshooting](#troubleshooting)

## System Architecture

### Primary Region (us-east-1)
- **Active Components**: All Lambda functions, DynamoDB tables, S3 buckets
- **Status**: Fully operational, serving all traffic

### DR Region (us-west-2)
- **Pilot Light Components**: Minimal Lambda functions, replicated data
- **Status**: Standby mode, ready for activation

### Key Metrics
- **RTO Target**: 5-10 minutes
- **RPO Target**: < 1 minute
- **Health Check Frequency**: Every 60 seconds
- **Backup Frequency**: Daily full backups

## Normal Operations

### Daily Checks

1. **Verify Health Status**
   ```bash
   # Check primary region health
   aws cloudwatch get-metric-statistics \
     --namespace DisasterRecovery \
     --metric-name DynamoDBHealth \
     --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
     --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
     --period 300 \
     --statistics Average \
     --region us-east-1
   ```

2. **Check Replication Lag**
   ```bash
   # Monitor replication lag
   aws cloudwatch get-metric-statistics \
     --namespace DisasterRecovery \
     --metric-name ReplicationLag \
     --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
     --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
     --period 300 \
     --statistics Maximum \
     --region us-east-1
   ```

3. **Verify Backup Status**
   ```bash
   # List recent backups
   aws dynamodb scan \
     --table-name dr-backup-metadata \
     --filter-expression "timestamp > :yesterday" \
     --expression-attribute-values '{":yesterday":{"N":"'$(date -d yesterday +%s)'"}}' \
     --region us-east-1
   ```

### Weekly Tasks

1. **Test Data Integrity**
   ```bash
   # Run data validator
   aws lambda invoke \
     --function-name dr-data-validator \
     --payload '{"validation_type": "full"}' \
     --region us-east-1 \
     validation-report.json
   ```

2. **Review Metrics Dashboard**
   - Open CloudWatch Dashboard
   - Check for any anomalies in the past week
   - Document any issues in the operations log

## Monitoring and Alerts

### CloudWatch Alarms

| Alarm Name             | Description                  | Action Required                     |
| ---------------------- | ---------------------------- | ----------------------------------- |
| DR-HealthCheck-Failed  | Health check has failed      | Investigate immediately             |
| DR-ReplicationLag-High | Replication lag > 60 seconds | Check DynamoDB Global Tables status |
| DR-BackupFailed        | Backup job failed            | Run manual backup                   |

### Alert Response Procedures

#### Health Check Failed

1. **Immediate Actions**
   ```bash
   # Check Lambda function logs
   aws logs tail /aws/lambda/dr-health-check --follow --region us-east-1
   
   # Test DynamoDB connectivity
   aws dynamodb list-tables --region us-east-1
   ```

2. **Escalation Path**
   - Level 1: On-call engineer (0-5 minutes)
   - Level 2: Team lead (5-15 minutes)
   - Level 3: Initiate failover (15+ minutes)

## Failover Procedures

### Decision Criteria

Initiate failover when:
- Primary region is completely unavailable
- Replication lag exceeds 5 minutes
- Multiple critical services are failing
- AWS reports regional outage

### Step-by-Step Failover

1. **Pre-Failover Validation**
   ```bash
   # Verify DR region readiness
   aws lambda invoke \
     --function-name dr-health-check-standby \
     --payload '{"region": "us-west-2"}' \
     --region us-west-2 \
     dr-health.json
   ```

2. **Initiate Failover**
   ```bash
   # Execute failover
   aws lambda invoke \
     --function-name dr-failover-controller \
     --payload '{"action": "failover", "target_region": "us-west-2", "force": false}' \
     --region us-east-1 \
     failover-result.json
   ```

3. **Verify Failover**
   ```bash
   # Test write operations in DR region
   aws dynamodb put-item \
     --table-name dr-sentinel-table \
     --item '{"id": {"S": "failover-test"}, "timestamp": {"N": "'$(date +%s)'"}}' \
     --region us-west-2
   ```

4. **Update DNS (Manual)**
   - Log into Route53 console
   - Update weighted routing policy
   - Set us-west-2 weight to 100
   - Set us-east-1 weight to 0

5. **Scale DR Resources**
   ```bash
   # Increase Lambda concurrency
   aws lambda put-function-concurrency \
     --function-name dr-health-check-standby \
     --reserved-concurrent-executions 100 \
     --region us-west-2
   ```

6. **Notify Stakeholders**
   - Send notification to ops-alerts channel
   - Update status page
   - Create incident ticket

## Failback Procedures

### Prerequisites

Before initiating failback:
- Primary region must be fully operational
- All data must be synchronized
- Stakeholder approval obtained

### Step-by-Step Failback

1. **Verify Primary Region Health**
   ```bash
   # Run comprehensive health check
   aws lambda invoke \
     --function-name dr-health-check \
     --payload '{"region": "us-east-1", "comprehensive": true}' \
     --region us-east-1 \
     primary-health.json
   ```

2. **Synchronize Data**
   ```bash
   # Force data sync
   aws lambda invoke \
     --function-name dr-data-validator \
     --payload '{"action": "sync", "source": "us-west-2", "target": "us-east-1"}' \
     --region us-west-2 \
     sync-result.json
   ```

3. **Gradual Traffic Shift**
   - Route53: Set us-east-1 weight to 10
   - Monitor for 15 minutes
   - Increase to 50% if stable
   - Monitor for 30 minutes
   - Increase to 100%

4. **Deactivate DR Resources**
   ```bash
   # Scale down DR Lambda
   aws lambda put-function-concurrency \
     --function-name dr-health-check-standby \
     --reserved-concurrent-executions 1 \
     --region us-west-2
   ```

## Testing Procedures

### Monthly DR Drill

1. **Schedule Maintenance Window**
   - Notify stakeholders 1 week in advance
   - Schedule during low-traffic period

2. **Pre-Test Checklist**
   - [ ] Backup all production data
   - [ ] Verify monitoring dashboards
   - [ ] Confirm rollback procedures
   - [ ] Alert on-call team

3. **Execute Test Failover**
   ```bash
   # Run test failover script
   ./scripts/test-failover.sh
   ```

4. **Validation Tests**
   - Write test records
   - Read from both regions
   - Verify replication
   - Check application functionality

5. **Document Results**
   - Record RTO achieved
   - Record RPO achieved
   - Note any issues
   - Update procedures as needed

### Quarterly Full DR Test

1. **Simulate Regional Failure**
   - Block all traffic to primary region
   - Execute full failover procedure
   - Run for 2 hours minimum

2. **Load Testing**
   ```bash
   # Run load test against DR region
   artillery run dr-load-test.yaml --target https://dr.example.com
   ```

3. **Failback Test**
   - Execute full failback procedure
   - Verify zero data loss

## Troubleshooting

### Common Issues

#### Issue: High Replication Lag

**Symptoms**: ReplicationLag metric > 60 seconds

**Resolution**:
1. Check DynamoDB Global Tables status
   ```bash
   aws dynamodb describe-table --table-name dr-application-table --region us-east-1
   ```

2. Verify network connectivity between regions

3. Check for throttling
   ```bash
   aws cloudwatch get-metric-statistics \
     --namespace AWS/DynamoDB \
     --metric-name ThrottledRequests \
     --dimensions Name=TableName,Value=dr-application-table \
     --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
     --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
     --period 300 \
     --statistics Sum \
     --region us-east-1
   ```

#### Issue: Lambda Function Timeout

**Symptoms**: Function execution exceeds timeout

**Resolution**:
1. Check CloudWatch Logs for specific error
2. Increase function timeout if needed
3. Optimize function code
4. Check for downstream service issues

#### Issue: S3 Replication Failure

**Symptoms**: Objects not appearing in DR bucket

**Resolution**:
1. Verify replication configuration
   ```bash
   aws s3api get-bucket-replication --bucket dr-demo-backup-bucket-primary
   ```

2. Check IAM role permissions

3. Verify destination bucket exists

4. Check for replication metrics
   ```bash
   aws s3api head-bucket --bucket dr-demo-backup-bucket-dr --region us-west-2
   ```

### Emergency Contacts

| Role             | Name         | Contact     | Escalation Time |
| ---------------- | ------------ | ----------- | --------------- |
| On-Call Engineer | Rotation     | PagerDuty   | Immediate       |
| Team Lead        | John Doe     | +1-555-0123 | 5 minutes       |
| AWS TAM          | Jane Smith   | +1-555-0456 | 15 minutes      |
| Director         | Mike Johnson | +1-555-0789 | 30 minutes      |

## Appendix

### Useful Commands Reference

```bash
# List all DR-related resources
aws resourcegroupstaggingapi get-resources \
  --tag-filters Key=Purpose,Values=DisasterRecoveryDemo \
  --region us-east-1

# Get Lambda function configurations
aws lambda list-functions \
  --query "Functions[?starts_with(FunctionName, 'dr-')]" \
  --region us-east-1

# Check S3 bucket replication status
aws s3api get-bucket-replication \
  --bucket dr-demo-backup-bucket-primary

# Monitor DynamoDB consumed capacity
aws cloudwatch get-metric-statistics \
  --namespace AWS/DynamoDB \
  --metric-name ConsumedReadCapacityUnits \
  --dimensions Name=TableName,Value=dr-application-table \
  --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 300 \
  --statistics Sum \
  --region us-east-1
```

### Cost Monitoring

```bash
# Check current month costs
aws ce get-cost-and-usage \
  --time-period Start=$(date -d "$(date +%Y-%m-01)" +%Y-%m-%d),End=$(date +%Y-%m-%d) \
  --granularity MONTHLY \
  --metrics "UnblendedCost" \
  --filter file://dr-cost-filter.json
```

### Performance Baselines

| Metric               | Normal Range | Alert Threshold |
| -------------------- | ------------ | --------------- |
| Health Check Latency | 50-200ms     | > 500ms         |
| Replication Lag      | 0-10s        | > 60s           |
| Backup Duration      | 1-5 minutes  | > 10 minutes    |
| Failover Time        | 5-10 minutes | > 15 minutes    |

---

**Last Updated**: January 2025  
**Version**: 1.0  
**Next Review**: April 2025