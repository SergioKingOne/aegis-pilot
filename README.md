# AWS Disaster Recovery Demo - Aegis Pilot

A production-ready AWS disaster recovery solution demonstrating cross-region failover, data replication, and automated backup strategies.

## 🏗️ Architecture Overview

This project implements a multi-region disaster recovery solution with:

- **Primary Region**: us-east-1 (N. Virginia)
- **DR Region**: us-west-2 (Oregon)
- **Global DynamoDB Tables** for automatic cross-region replication
- **Lambda Functions** for health monitoring and failover orchestration
- **CloudWatch Alarms** for proactive monitoring
- **S3 Cross-Region Replication** for backup redundancy

## 📋 Prerequisites

- AWS CLI
- AWS SAM CLI (v1.100+)
- Rust toolchain with cargo-lambda
- jq for JSON processing

## 🚀 Quick Start

1. **Clone and setup environment**:
   ```bash
   cp .env.example .env
   # Edit .env with your AWS credentials
   ```

2. **Deploy infrastructure**:
   ```bash
   ./scripts/deploy.sh
   ```

3. **Test deployment**:
   ```bash
   ./scripts/test-deploy.sh
   ```

4. **Test failover**:
   ```bash
   ./scripts/test-failover.sh
   ```

5. **Cleanup resources**:
   ```bash
   ./scripts/cleanup.sh
   ```

## 🔧 Key Components

### Lambda Functions

- **health-check**: Monitors service health in both regions
- **backup-manager**: Manages automated backups
- **failover-controller**: Orchestrates failover process
- **data-validator**: Validates data consistency across regions

### DynamoDB Tables

- **dr-application-table**: Main application data (global table)
- **dr-sentinel-table**: Health check sentinel data (global table)
- **dr-backup-metadata**: Backup tracking metadata (global table)

### Monitoring

- CloudWatch dashboards for real-time monitoring
- Alarms for replication lag and health check failures
- Automated alerts via SNS (configurable)

## 📊 Testing Results

During testing, the system demonstrated:

- ✅ Successful cross-region data replication
- ✅ Lambda functions operational in both regions
- ✅ Automated health monitoring
- ✅ Data consistency validation
- ⚠️ Minor issue with failover controller (requires Lambda deployment in DR region)

## 🛠️ Troubleshooting

### Common Issues

1. **"Table already exists" during deployment**
   - Run cleanup script first: `./scripts/cleanup.sh`
   - Wait 1-2 minutes for resources to be fully deleted

2. **Replication not working**
   - Global tables take 1-2 minutes to establish replication
   - Check CloudWatch logs for DynamoDB streams

3. **Failover controller errors**
   - Ensure Lambda functions are deployed in both regions
   - Check IAM permissions for cross-region access

## 📁 Project Structure

```
aegis-pilot/
├── cloudformation/          # Infrastructure templates
├── lambda-functions/        # Rust Lambda functions
├── scripts/                # Deployment & testing scripts
├── docs/                   # Documentation
└── monitoring/             # CloudWatch dashboards
```

## 🔒 Security Considerations

- All Lambda functions use least-privilege IAM roles
- DynamoDB encryption at rest enabled
- S3 buckets configured with versioning and encryption
- Cross-region replication uses secure IAM roles

## 💰 Cost Optimization

- DynamoDB tables use on-demand billing
- Lambda functions optimized for minimal memory usage
- S3 lifecycle policies for backup retention
- CloudWatch logs retention set to 7 days

## 🚧 Future Enhancements

- [ ] Implement Route53 health checks for DNS failover
- [ ] Add RDS cross-region read replicas
- [ ] Integrate with AWS Backup for centralized management
- [ ] Implement automated failback procedures
- [ ] Add comprehensive integration tests

## 📄 License

This is a demo project for educational purposes.
