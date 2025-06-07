# AWS Disaster Recovery Project with Rust

## Project Overview

This project implements a **Pilot Light DR strategy** using AWS services within the free tier. It demonstrates key disaster recovery concepts for AWS SAA preparation, including:

- **RTO (Recovery Time Objective)**: ~5-10 minutes
- **RPO (Recovery Point Objective)**: ~1 minute for critical data
- **Multi-region deployment** with automated failover
- **Infrastructure as Code** using CloudFormation
- **Continuous data replication** and backup strategies

## Setup and Running

### Prerequisites
- AWS CLI installed and configured
- AWS SAM CLI installed
- Rust toolchain (cargo, rustc)
- jq for processing JSON responses

### Environment Setup
1. Create an `.env` file based on the provided `.env.example`:
   ```bash
   cp .env.example .env
   ```

2. Edit the `.env` file with your AWS credentials and configuration:
   ```
   # AWS Credentials
   AWS_ACCESS_KEY_ID=your_access_key_here
   AWS_SECRET_ACCESS_KEY=your_secret_key_here
   AWS_DEFAULT_REGION=us-east-1
   AWS_DR_REGION=us-west-2
   ```

### Deployment
1. Build and deploy the project:
   ```bash
   ./scripts/deploy.sh
   ```
   This will compile the Lambda functions, package them, and deploy the infrastructure to both primary and DR regions.

### Testing Failover
1. Run the failover test script:
   ```bash
   ./scripts/test-failover.sh
   ```
   This will test the disaster recovery functionality by simulating a failover scenario.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Primary Region (us-east-1)                    │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐   │
│  │   Lambda     │    │  DynamoDB    │    │       S3 Bucket      │   │
│  │  Functions   │───▶│  (Global     │    │   (Cross-Region     │   │
│  │             │    │   Table)     │    │    Replication)     │   │
│  └─────────────┘    └──────────────┘    └─────────────────────┘   │
│         │                   │                        │               │
│         └───────────────────┴────────────────────────┘              │
│                             │                                        │
│                    ┌────────────────┐                               │
│                    │   CloudWatch    │                              │
│                    │   Monitoring    │                              │
│                    └────────────────┘                               │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                │ Replication
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      DR Region (us-west-2)                          │
├─────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌──────────────┐    ┌─────────────────────┐   │
│  │   Lambda     │    │  DynamoDB    │    │    S3 Bucket        │   │
│  │  (Minimal)   │    │  (Replica)   │    │   (Replica)         │   │
│  └─────────────┘    └──────────────┘    └─────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

## Project Structure

```
aws-dr-project/
├── Cargo.toml                          # Workspace configuration
├── README.md                           # Project documentation
├── .gitignore
├── .env.example                        # Example environment configuration
├── cloudformation/                     # Infrastructure as Code
│   ├── primary-region.yaml            # Primary region resources
│   ├── dr-region.yaml                 # DR region resources
│   └── global-resources.yaml          # Global resources (Route53, etc.)
├── lambda-functions/                   # Lambda function code
│   ├── health-check/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs               # Health check function
│   ├── backup-manager/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs               # Automated backup function
│   ├── failover-controller/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs               # Failover orchestration
│   └── data-validator/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs               # Data consistency validator
├── scripts/                           # Deployment and testing scripts
│   ├── deploy.sh                     # Deployment script
│   ├── test-failover.sh             # Failover testing
│   └── cleanup.sh                    # Resource cleanup
├── monitoring/                        # Monitoring configurations
│   └── cloudwatch-dashboards.json    # CloudWatch dashboard definitions
└── docs/                             # Additional documentation
    ├── runbook.md                    # DR runbook
    └── testing-procedures.md         # Testing procedures
```

## Core Components

### 1. DynamoDB Global Tables
- Provides multi-region, multi-master replication
- Near real-time data synchronization
- Automatic failover capabilities

### 2. S3 Cross-Region Replication
- Backup storage with automatic replication
- Versioning enabled for point-in-time recovery
- Lifecycle policies for cost optimization

### 3. Lambda Functions
- **Health Check**: Monitors system health across regions
- **Backup Manager**: Automated backup creation and management
- **Failover Controller**: Orchestrates failover process
- **Data Validator**: Ensures data consistency between regions

### 4. CloudWatch Monitoring
- Custom metrics for DR readiness
- Alarms for automatic failover triggers
- Dashboards for visual monitoring


## Free Tier Considerations

This project is designed to stay within AWS Free Tier limits:
- **DynamoDB**: Uses on-demand pricing (25 GB free)
- **Lambda**: Well under 1 million requests/month
- **S3**: Stays under 5 GB storage limit
- **CloudWatch**: Uses less than 10 custom metrics

## Next Steps

1. Deploy the project following the deployment script
2. Run the test scenarios to understand failover behavior
3. Monitor costs in AWS Cost Explorer
4. Experiment with different DR strategies
5. Practice recovery procedures

## Additional Resources

- [AWS Well-Architected Framework - Reliability Pillar](https://docs.aws.amazon.com/wellarchitected/latest/reliability-pillar/welcome.html)
- [AWS Disaster Recovery Whitepaper](https://docs.aws.amazon.com/whitepapers/latest/disaster-recovery-workloads-on-aws/introduction.html)
- [DynamoDB Global Tables](https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GlobalTables.html)
- [S3 Cross-Region Replication](https://docs.aws.amazon.com/AmazonS3/latest/userguide/replication.html) 