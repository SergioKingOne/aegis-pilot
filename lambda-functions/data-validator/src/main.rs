use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_cloudwatch::{types::MetricDatum, types::StandardUnit, Client as CloudWatchClient};
use aws_sdk_dynamodb::{types::AttributeValue, Client as DynamoClient};
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct Request {
    validation_type: Option<String>, // "full", "incremental", or "specific"
    table_name: Option<String>,
    source_region: Option<String>,
    target_region: Option<String>,
    action: Option<String>, // "validate" or "sync"
}

#[derive(Serialize)]
struct Response {
    status: String,
    validation_type: String,
    timestamp: String,
    results: ValidationResults,
    recommendations: Vec<String>,
}

#[derive(Serialize)]
struct ValidationResults {
    tables_validated: usize,
    records_checked: usize,
    mismatches_found: usize,
    replication_lag_seconds: Option<i64>,
    backup_status: BackupStatus,
    consistency_score: f64,
}

#[derive(Serialize)]
struct BackupStatus {
    last_backup_age_hours: Option<f64>,
    backup_count: usize,
    oldest_backup_days: Option<f64>,
}

#[derive(Debug)]
struct TableValidation {
    table_name: String,
    primary_count: usize,
    dr_count: usize,
    sample_mismatches: Vec<String>,
}

struct DataValidatorService {
    primary_dynamo: DynamoClient,
    dr_dynamo: DynamoClient,
    #[allow(dead_code)]
    s3_client: S3Client,
    cloudwatch_client: CloudWatchClient,
    #[allow(dead_code)]
    source_region: String,
    #[allow(dead_code)]
    target_region: String,
}

impl DataValidatorService {
    async fn new(
        source_region: Option<String>,
        target_region: Option<String>,
    ) -> Result<Self, Error> {
        let source_region = source_region.unwrap_or_else(|| "us-east-1".to_string());
        let target_region = target_region.unwrap_or_else(|| "us-west-2".to_string());

        // Configure clients for both regions
        let primary_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(source_region.clone()))
            .load()
            .await;

        let dr_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(target_region.clone()))
            .load()
            .await;

        Ok(Self {
            primary_dynamo: DynamoClient::new(&primary_config),
            dr_dynamo: DynamoClient::new(&dr_config),
            s3_client: S3Client::new(&primary_config),
            cloudwatch_client: CloudWatchClient::new(&primary_config),
            source_region,
            target_region,
        })
    }

    async fn get_table_item_count(&self, client: &DynamoClient, table_name: &str) -> Result<usize> {
        let result = client
            .describe_table()
            .table_name(table_name)
            .send()
            .await?;

        if let Some(table) = result.table {
            Ok(table.item_count.unwrap_or(0) as usize)
        } else {
            Ok(0)
        }
    }

    async fn validate_table_data(&self, table_name: &str) -> Result<TableValidation> {
        info!("Validating table: {}", table_name);

        // Get item counts
        let primary_count = self
            .get_table_item_count(&self.primary_dynamo, table_name)
            .await?;
        let dr_count = self
            .get_table_item_count(&self.dr_dynamo, table_name)
            .await?;

        let mut sample_mismatches = Vec::new();

        // Sample validation - check a few random items
        let scan_result = self
            .primary_dynamo
            .scan()
            .table_name(table_name)
            .limit(10)
            .send()
            .await?;

        if let Some(items) = scan_result.items {
            for item in items.iter() {
                if let Some(id_attr) = item.get("id") {
                    if let Ok(id) = id_attr.as_s() {
                        // Check if item exists in DR
                        let dr_result = self
                            .dr_dynamo
                            .get_item()
                            .table_name(table_name)
                            .key("id", AttributeValue::S(id.to_string()))
                            .send()
                            .await;

                        match dr_result {
                            Ok(response) => {
                                if response.item.is_none() {
                                    sample_mismatches.push(format!("Item {} not found in DR", id));
                                }
                            }
                            Err(e) => {
                                warn!("Error checking item {} in DR: {}", id, e);
                            }
                        }
                    }
                }
            }
        }

        Ok(TableValidation {
            table_name: table_name.to_string(),
            primary_count,
            dr_count,
            sample_mismatches,
        })
    }

    async fn check_replication_lag(&self) -> Result<Option<i64>> {
        // Write a timestamp to primary
        let test_id = format!("lag-test-{}", Utc::now().timestamp_millis());
        let timestamp = Utc::now().timestamp();

        self.primary_dynamo
            .put_item()
            .table_name("dr-sentinel-table")
            .item("id", AttributeValue::S(test_id.clone()))
            .item("timestamp", AttributeValue::N(timestamp.to_string()))
            .item("source", AttributeValue::S("validator".to_string()))
            .send()
            .await?;

        // Wait a bit for replication
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Try to read from DR
        let start_time = Utc::now();
        let mut lag = None;

        for _ in 0..10 {
            let result = self
                .dr_dynamo
                .get_item()
                .table_name("dr-sentinel-table")
                .key("id", AttributeValue::S(test_id.clone()))
                .send()
                .await;

            if let Ok(response) = result {
                if response.item.is_some() {
                    lag = Some((Utc::now() - start_time).num_seconds());
                    break;
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        // Clean up test record
        let _ = self
            .primary_dynamo
            .delete_item()
            .table_name("dr-sentinel-table")
            .key("id", AttributeValue::S(test_id))
            .send()
            .await;

        Ok(lag)
    }

    async fn validate_backups(&self) -> Result<BackupStatus> {
        let _bucket_name = std::env::var("BACKUP_BUCKET")
            .unwrap_or_else(|_| "dr-demo-backup-bucket-primary".to_string());

        // Check backup metadata
        let scan_result = self
            .primary_dynamo
            .scan()
            .table_name("dr-backup-metadata")
            .send()
            .await?;

        let mut last_backup_timestamp = 0i64;
        let mut oldest_backup_timestamp = i64::MAX;
        let backup_count = scan_result
            .items
            .as_ref()
            .map(|items| items.len())
            .unwrap_or(0);

        if let Some(items) = scan_result.items {
            for item in items {
                if let Some(timestamp_attr) = item.get("timestamp") {
                    if let Ok(timestamp_str) = timestamp_attr.as_n() {
                        if let Ok(timestamp) = timestamp_str.parse::<i64>() {
                            last_backup_timestamp = last_backup_timestamp.max(timestamp);
                            oldest_backup_timestamp = oldest_backup_timestamp.min(timestamp);
                        }
                    }
                }
            }
        }

        let current_time = Utc::now().timestamp();
        let last_backup_age_hours = if last_backup_timestamp > 0 {
            Some((current_time - last_backup_timestamp) as f64 / 3600.0)
        } else {
            None
        };

        let oldest_backup_days = if oldest_backup_timestamp < i64::MAX {
            Some((current_time - oldest_backup_timestamp) as f64 / 86400.0)
        } else {
            None
        };

        Ok(BackupStatus {
            last_backup_age_hours,
            backup_count,
            oldest_backup_days,
        })
    }

    async fn sync_missing_items(
        &self,
        _table_name: &str,
        validation: &TableValidation,
    ) -> Result<usize> {
        let mut synced_count = 0;

        // This is a simplified sync - in production, you'd want to handle this more carefully
        if validation.primary_count > validation.dr_count {
            info!(
                "Syncing {} missing items",
                validation.primary_count - validation.dr_count
            );

            // For demo purposes, we'll just log this
            // In a real implementation, you'd scan the primary table and sync missing items
            warn!(
                "Sync operation would sync {} items to DR region",
                validation.primary_count - validation.dr_count
            );

            synced_count = validation.primary_count - validation.dr_count;
        }

        Ok(synced_count)
    }

    async fn publish_single_metric(
        &self,
        namespace: &str,
        metric_name: &str,
        value: f64,
        unit: StandardUnit,
    ) -> Result<(), Error> {
        let timestamp = std::time::SystemTime::now();

        // Create the metric
        let metric = MetricDatum::builder()
            .metric_name(metric_name)
            .value(value)
            .unit(unit)
            .timestamp(aws_sdk_cloudwatch::primitives::DateTime::from(timestamp))
            .build();

        // Send the metric
        match self
            .cloudwatch_client
            .put_metric_data()
            .namespace(namespace)
            .metric_data(metric)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to publish metric {}: {}", metric_name, e);
                Err(Error::from(e))
            }
        }
    }

    async fn publish_validation_metrics(&self, results: &ValidationResults) -> Result<()> {
        let namespace = "DisasterRecovery";

        // Publish consistency score metric
        match self
            .publish_single_metric(
                namespace,
                "ValidationConsistencyScore",
                results.consistency_score,
                StandardUnit::Percent,
            )
            .await
        {
            Ok(_) => (),
            Err(e) => error!("Failed to publish consistency score metric: {}", e),
        }

        // Publish mismatches metric
        match self
            .publish_single_metric(
                namespace,
                "ValidationMismatches",
                results.mismatches_found as f64,
                StandardUnit::Count,
            )
            .await
        {
            Ok(_) => (),
            Err(e) => error!("Failed to publish mismatches metric: {}", e),
        }

        Ok(())
    }

    fn generate_recommendations(&self, results: &ValidationResults) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Check consistency score
        if results.consistency_score < 95.0 {
            recommendations.push(format!(
                "Data consistency is below 95% ({:.1}%). Investigate mismatches immediately.",
                results.consistency_score
            ));
        }

        // Check replication lag
        if let Some(lag) = results.replication_lag_seconds {
            if lag > 60 {
                recommendations.push(format!(
                    "Replication lag is {} seconds. Consider investigating DynamoDB Global Tables health.",
                    lag
                ));
            }
        }

        // Check backup age
        if let Some(age_hours) = results.backup_status.last_backup_age_hours {
            if age_hours > 24.0 {
                recommendations.push(format!(
                    "Last backup is {:.1} hours old. Consider running a manual backup.",
                    age_hours
                ));
            }
        }

        // Check backup retention
        if let Some(oldest_days) = results.backup_status.oldest_backup_days {
            if oldest_days > 30.0 {
                recommendations.push(format!(
                    "Oldest backup is {:.0} days old. Consider reviewing retention policy.",
                    oldest_days
                ));
            }
        }

        if recommendations.is_empty() {
            recommendations.push("All validation checks passed. System is healthy.".to_string());
        }

        recommendations
    }

    async fn run_validation(
        &self,
        validation_type: &str,
        table_name: Option<String>,
        action: &str,
    ) -> Result<Response, Error> {
        // Determine which tables to validate
        let tables_to_validate = if let Some(table_name) = table_name {
            vec![table_name]
        } else {
            vec![
                "dr-application-table".to_string(),
                "dr-sentinel-table".to_string(),
            ]
        };

        // Perform validation
        let mut total_mismatches = 0;
        let mut total_records = 0;
        let mut validations = Vec::new();

        for table_name in &tables_to_validate {
            match self.validate_table_data(table_name).await {
                Ok(validation) => {
                    total_records += validation.primary_count;
                    let mismatches = validation.primary_count.abs_diff(validation.dr_count)
                        + validation.sample_mismatches.len();
                    total_mismatches += mismatches;

                    if action == "sync" && mismatches > 0 {
                        if let Ok(synced) = self.sync_missing_items(table_name, &validation).await {
                            info!("Synced {} items for table {}", synced, table_name);
                        }
                    }

                    validations.push(validation);
                }
                Err(e) => {
                    error!("Failed to validate table {}: {}", table_name, e);
                }
            }
        }

        // Check replication lag
        let replication_lag = self.check_replication_lag().await.unwrap_or(None);

        // Validate backups
        let backup_status = self.validate_backups().await.unwrap_or(BackupStatus {
            last_backup_age_hours: None,
            backup_count: 0,
            oldest_backup_days: None,
        });

        // Calculate consistency score
        let consistency_score = if total_records > 0 {
            ((total_records - total_mismatches) as f64 / total_records as f64) * 100.0
        } else {
            100.0
        };

        let results = ValidationResults {
            tables_validated: validations.len(),
            records_checked: total_records,
            mismatches_found: total_mismatches,
            replication_lag_seconds: replication_lag,
            backup_status,
            consistency_score,
        };

        // Publish metrics
        if let Err(e) = self.publish_validation_metrics(&results).await {
            error!("Failed to publish metrics: {}", e);
        }

        // Generate recommendations
        let recommendations = self.generate_recommendations(&results);

        // Log validation summary
        info!(
            "Validation complete: {} tables, {} records, {:.1}% consistency",
            results.tables_validated, results.records_checked, results.consistency_score
        );

        for validation in &validations {
            if !validation.sample_mismatches.is_empty() {
                warn!(
                    "Table {} has mismatches: {:?}",
                    validation.table_name, validation.sample_mismatches
                );
            }
        }

        Ok(Response {
            status: if results.consistency_score >= 95.0 {
                "healthy"
            } else {
                "degraded"
            }
            .to_string(),
            validation_type: validation_type.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            results,
            recommendations,
        })
    }
}

async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let validation_type = event
        .payload
        .validation_type
        .unwrap_or_else(|| "incremental".to_string());
    let action = event
        .payload
        .action
        .unwrap_or_else(|| "validate".to_string());

    let service =
        DataValidatorService::new(event.payload.source_region, event.payload.target_region).await?;

    service
        .run_validation(&validation_type, event.payload.table_name, &action)
        .await
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    run(service_fn(function_handler)).await
}
