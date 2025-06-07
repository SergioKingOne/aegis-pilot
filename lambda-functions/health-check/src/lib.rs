use aws_sdk_cloudwatch::{
    types::{MetricDatum, StandardUnit},
    Client as CloudWatchClient,
};
use aws_sdk_dynamodb::Client as DynamoClient;
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Deserialize, Debug, Clone)]
pub struct Request {
    pub region: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Response {
    pub status: String,
    pub region: String,
    pub timestamp: String,
    pub services: ServiceStatus,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ServiceStatus {
    pub dynamodb: bool,
    pub s3: bool,
    pub replication_lag: Option<i64>,
}

pub struct HealthCheckService {
    dynamo_client: DynamoClient,
    s3_client: S3Client,
    cloudwatch_client: CloudWatchClient,
    region: String,
}

impl HealthCheckService {
    pub async fn new(region: Option<String>) -> Result<Self, lambda_runtime::Error> {
        let region_str = region.unwrap_or_else(|| {
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string())
        });

        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;

        Ok(Self {
            dynamo_client: DynamoClient::new(&config),
            s3_client: S3Client::new(&config),
            cloudwatch_client: CloudWatchClient::new(&config),
            region: region_str,
        })
    }

    pub async fn check_dynamodb_health(&self) -> Result<bool, lambda_runtime::Error> {
        let result = self.dynamo_client.list_tables().limit(1).send().await;
        Ok(result.is_ok())
    }

    pub async fn check_s3_health(&self) -> Result<bool, lambda_runtime::Error> {
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

    pub async fn check_replication_lag(&self) -> Result<Option<i64>, lambda_runtime::Error> {
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

    pub async fn publish_metrics(
        &self,
        status: &ServiceStatus,
    ) -> Result<(), lambda_runtime::Error> {
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
                    Err(lambda_runtime::Error::from(e))
                }
            }
        } else {
            error!("No valid metrics to publish");
            Ok(())
        }
    }

    pub async fn run_health_check(&self) -> Result<Response, lambda_runtime::Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"region": "us-west-2"}"#;
        let request: Request = serde_json::from_str(json).unwrap();
        assert_eq!(request.region, Some("us-west-2".to_string()));

        let json_empty = r#"{}"#;
        let request_empty: Request = serde_json::from_str(json_empty).unwrap();
        assert_eq!(request_empty.region, None);
    }

    #[test]
    fn test_response_serialization() {
        let response = Response {
            status: "healthy".to_string(),
            region: "us-east-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            services: ServiceStatus {
                dynamodb: true,
                s3: true,
                replication_lag: Some(5),
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("healthy"));
        assert!(json.contains("us-east-1"));
        assert!(json.contains("2025-01-01T00:00:00Z"));
        assert!(json.contains("\"dynamodb\":true"));
        assert!(json.contains("\"s3\":true"));
        assert!(json.contains("\"replication_lag\":5"));
    }

    #[test]
    fn test_service_status_healthy() {
        let status = ServiceStatus {
            dynamodb: true,
            s3: true,
            replication_lag: Some(5),
        };

        assert!(status.dynamodb);
        assert!(status.s3);
        assert_eq!(status.replication_lag, Some(5));
    }

    #[test]
    fn test_service_status_unhealthy() {
        let status = ServiceStatus {
            dynamodb: false,
            s3: false,
            replication_lag: None,
        };

        assert!(!status.dynamodb);
        assert!(!status.s3);
        assert_eq!(status.replication_lag, None);
    }

    #[test]
    fn test_status_determination() {
        let healthy_status = ServiceStatus {
            dynamodb: true,
            s3: true,
            replication_lag: Some(10),
        };

        let unhealthy_dynamo = ServiceStatus {
            dynamodb: false,
            s3: true,
            replication_lag: Some(10),
        };

        let unhealthy_s3 = ServiceStatus {
            dynamodb: true,
            s3: false,
            replication_lag: Some(10),
        };

        // Test the logic for determining overall health
        assert!(healthy_status.dynamodb && healthy_status.s3);
        assert!(!(unhealthy_dynamo.dynamodb && unhealthy_dynamo.s3));
        assert!(!(unhealthy_s3.dynamodb && unhealthy_s3.s3));
    }

    #[test]
    fn test_response_equality() {
        let response1 = Response {
            status: "healthy".to_string(),
            region: "us-east-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            services: ServiceStatus {
                dynamodb: true,
                s3: true,
                replication_lag: Some(5),
            },
        };

        let response2 = Response {
            status: "healthy".to_string(),
            region: "us-east-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            services: ServiceStatus {
                dynamodb: true,
                s3: true,
                replication_lag: Some(5),
            },
        };

        assert_eq!(response1, response2);
    }
}
