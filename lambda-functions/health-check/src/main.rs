use aws_config::BehaviorVersion;
use aws_sdk_cloudwatch::{
    types::{MetricDatum, StandardUnit},
    Client as CloudWatchClient,
};
use aws_sdk_dynamodb::Client as DynamoClient;
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Deserialize)]
struct Request {
    region: Option<String>,
}

#[derive(Serialize)]
struct Response {
    status: String,
    region: String,
    timestamp: String,
    services: ServiceStatus,
}

#[derive(Serialize)]
struct ServiceStatus {
    dynamodb: bool,
    s3: bool,
    replication_lag: Option<i64>,
}

struct HealthCheckService {
    dynamo_client: DynamoClient,
    s3_client: S3Client,
    cloudwatch_client: CloudWatchClient,
    region: String,
}

impl HealthCheckService {
    async fn new(region: Option<String>) -> Result<Self, Error> {
        let region_str = region.unwrap_or_else(|| {
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string())
        });

        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;

        Ok(Self {
            dynamo_client: DynamoClient::new(&config),
            s3_client: S3Client::new(&config),
            cloudwatch_client: CloudWatchClient::new(&config),
            region: region_str,
        })
    }

    async fn check_dynamodb_health(&self) -> Result<bool, Error> {
        let result = self.dynamo_client.list_tables().limit(1).send().await;
        Ok(result.is_ok())
    }

    async fn check_s3_health(&self) -> Result<bool, Error> {
        // Get bucket name from environment variable or use default
        let bucket_name = std::env::var("BACKUP_BUCKET")
            .unwrap_or_else(|_| format!("dr-demo-backup-bucket-{}", self.region));

        // Try to list objects (with a limit of 1) to check connectivity
        let result = self
            .s3_client
            .list_objects_v2()
            .bucket(&bucket_name)
            .max_keys(1)
            .send()
            .await;

        Ok(result.is_ok())
    }

    async fn check_replication_lag(&self) -> Result<Option<i64>, Error> {
        // Check a sentinel record to measure replication lag
        let result = self
            .dynamo_client
            .get_item()
            .table_name("dr-sentinel-table")
            .key(
                "id",
                aws_sdk_dynamodb::types::AttributeValue::S("sentinel".to_string()),
            )
            .send()
            .await;

        if let Ok(response) = result {
            if let Some(item) = response.item {
                if let Some(timestamp_attr) = item.get("last_updated") {
                    if let Ok(timestamp_str) = timestamp_attr.as_n() {
                        if let Ok(timestamp) = timestamp_str.parse::<i64>() {
                            let current_time = Utc::now().timestamp();
                            return Ok(Some(current_time - timestamp));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    async fn publish_metrics(&self, status: &ServiceStatus) -> Result<(), Error> {
        let namespace = "DisasterRecovery";
        let timestamp = std::time::SystemTime::now();
        let aws_timestamp = aws_sdk_cloudwatch::primitives::DateTime::from(timestamp);

        // Create metrics for publishing
        let mut metrics = Vec::new();

        // DynamoDB health metric
        let dynamodb_metric = MetricDatum::builder()
            .metric_name("DynamoDBHealth")
            .value(if status.dynamodb { 1.0 } else { 0.0 })
            .unit(StandardUnit::None)
            .timestamp(aws_timestamp.clone())
            .build();

        metrics.push(dynamodb_metric);

        // S3 health metric
        let s3_metric = MetricDatum::builder()
            .metric_name("S3Health")
            .value(if status.s3 { 1.0 } else { 0.0 })
            .unit(StandardUnit::None)
            .timestamp(aws_timestamp.clone())
            .build();

        metrics.push(s3_metric);

        // Replication lag metric (if available)
        if let Some(lag) = status.replication_lag {
            let replication_metric = MetricDatum::builder()
                .metric_name("ReplicationLag")
                .value(lag as f64)
                .unit(StandardUnit::Seconds)
                .timestamp(aws_timestamp.clone())
                .build();

            metrics.push(replication_metric);
        }

        // If we have metrics to publish, send them
        if !metrics.is_empty() {
            info!("Publishing {} metrics to CloudWatch", metrics.len());

            // Publish all metrics in a single call
            match self
                .cloudwatch_client
                .put_metric_data()
                .namespace(namespace)
                .set_metric_data(Some(metrics))
                .send()
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("Failed to publish metrics: {}", e);
                    Err(Error::from(e))
                }
            }
        } else {
            error!("No valid metrics to publish");
            Ok(())
        }
    }

    async fn run_health_check(&self) -> Result<Response, Error> {
        // Check service health
        let dynamodb_health = self.check_dynamodb_health().await?;
        let s3_health = self.check_s3_health().await?;
        let replication_lag = self.check_replication_lag().await?;

        let status = ServiceStatus {
            dynamodb: dynamodb_health,
            s3: s3_health,
            replication_lag,
        };

        // Publish metrics to CloudWatch
        if let Err(e) = self.publish_metrics(&status).await {
            error!("Failed to publish metrics: {}", e);
        }

        Ok(Response {
            status: if dynamodb_health && s3_health {
                "healthy"
            } else {
                "unhealthy"
            }
            .to_string(),
            region: self.region.clone(),
            timestamp: Utc::now().to_rfc3339(),
            services: status,
        })
    }
}

async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let service = HealthCheckService::new(event.payload.region).await?;
    service.run_health_check().await
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    run(service_fn(function_handler)).await
}
