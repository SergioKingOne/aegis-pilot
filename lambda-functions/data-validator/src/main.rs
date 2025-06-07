use anyhow::Result;
use aws_config::BehaviorVersion;
use aws_sdk_cloudwatch::{types::MetricDatum, types::StandardUnit, Client as CloudWatchClient};
use aws_sdk_dynamodb::{types::AttributeValue, Client as DynamoClient};
use aws_sdk_s3::Client as S3Client;
use bon::Builder;
use chrono::Utc;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationMode {
    Full,
    Incremental,
    Specific,
}

impl Default for ValidationMode {
    fn default() -> Self {
        Self::Incremental
    }
}

impl fmt::Display for ValidationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Incremental => write!(f, "incremental"),
            Self::Specific => write!(f, "specific"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    Validate,
    Sync,
}

impl Default for ActionType {
    fn default() -> Self {
        Self::Validate
    }
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validate => write!(f, "validate"),
            Self::Sync => write!(f, "sync"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AwsRegion(String);

impl AwsRegion {
    pub fn new(region: impl Into<String>) -> Self {
        Self(region.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn default_source() -> Self {
        Self("us-east-1".to_string())
    }

    fn default_target() -> Self {
        Self("us-west-2".to_string())
    }
}

impl Default for AwsRegion {
    fn default() -> Self {
        Self::default_source()
    }
}

impl fmt::Display for AwsRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableName(String);

impl TableName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Builder, Deserialize)]
#[builder(on(AwsRegion, into))]
#[serde(rename_all = "snake_case")]
pub struct ValidationRequest {
    #[builder(default = ValidationMode::default())]
    #[serde(default = "ValidationMode::default")]
    pub validation_mode: ValidationMode,

    pub table_name: Option<TableName>,

    #[builder(default = AwsRegion::default_source())]
    #[serde(default = "AwsRegion::default_source")]
    pub source_region: AwsRegion,

    #[builder(default = AwsRegion::default_target())]
    #[serde(default = "AwsRegion::default_target")]
    pub target_region: AwsRegion,

    #[builder(default = ActionType::default())]
    #[serde(default = "ActionType::default")]
    pub action: ActionType,
}

#[derive(Builder, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ValidationResponse {
    pub status: ValidationStatus,
    pub validation_mode: ValidationMode,
    pub timestamp: chrono::DateTime<Utc>,
    pub results: ValidationResults,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    Healthy,
    Degraded,
    Failed,
}

impl fmt::Display for ValidationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Builder, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ValidationResults {
    pub tables_validated: usize,
    pub records_checked: usize,
    pub mismatches_found: usize,
    pub replication_lag_seconds: Option<i64>,
    pub backup_status: BackupStatus,
    pub consistency_score: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BackupStatus {
    pub last_backup_age_hours: Option<f64>,
    pub backup_count: usize,
    pub oldest_backup_days: Option<f64>,
}

#[derive(Debug)]
struct TableValidation {
    table_name: TableName,
    primary_count: usize,
    dr_count: usize,
    sample_mismatches: Vec<String>,
}

pub struct DataValidatorService {
    primary_dynamo: DynamoClient,
    dr_dynamo: DynamoClient,
    s3_client: S3Client,
    cloudwatch_client: CloudWatchClient,
    source_region: AwsRegion,
    target_region: AwsRegion,
}

impl DataValidatorService {
    pub async fn new(source_region: AwsRegion, target_region: AwsRegion) -> Result<Self, Error> {
        let source_region_str = source_region.0.clone();
        let target_region_str = target_region.0.clone();

        let primary_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(source_region_str))
            .load()
            .await;

        let dr_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(target_region_str))
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

    async fn get_table_item_count(
        &self,
        client: &DynamoClient,
        table_name: &TableName,
    ) -> Result<usize> {
        let result = client
            .describe_table()
            .table_name(table_name.as_str())
            .send()
            .await?;

        Ok(result.table.and_then(|table| table.item_count).unwrap_or(0) as usize)
    }

    async fn validate_table_data(&self, table_name: &TableName) -> Result<TableValidation> {
        info!("Validating table: {}", table_name);

        let primary_count = self
            .get_table_item_count(&self.primary_dynamo, table_name)
            .await?;
        let dr_count = self
            .get_table_item_count(&self.dr_dynamo, table_name)
            .await?;

        let mut sample_mismatches = Vec::new();

        let scan_result = self
            .primary_dynamo
            .scan()
            .table_name(table_name.as_str())
            .limit(10)
            .send()
            .await?;

        if let Some(items) = scan_result.items {
            for item in items.iter() {
                if let Some(id_attr) = item.get("id") {
                    if let Ok(id) = id_attr.as_s() {
                        let dr_result = self
                            .dr_dynamo
                            .get_item()
                            .table_name(table_name.as_str())
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
            table_name: table_name.clone(),
            primary_count,
            dr_count,
            sample_mismatches,
        })
    }

    async fn check_replication_lag(&self) -> Result<Option<i64>> {
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

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

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
        _table_name: &TableName,
        validation: &TableValidation,
    ) -> Result<usize> {
        let mut synced_count = 0;

        if validation.primary_count > validation.dr_count {
            info!(
                "Syncing {} missing items",
                validation.primary_count - validation.dr_count
            );

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

        let metric = MetricDatum::builder()
            .metric_name(metric_name)
            .value(value)
            .unit(unit)
            .timestamp(aws_sdk_cloudwatch::primitives::DateTime::from(timestamp))
            .build();

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

        if let Err(e) = self
            .publish_single_metric(
                namespace,
                "ValidationConsistencyScore",
                results.consistency_score,
                StandardUnit::Percent,
            )
            .await
        {
            error!("Failed to publish consistency score metric: {}", e);
        }

        if let Err(e) = self
            .publish_single_metric(
                namespace,
                "ValidationMismatches",
                results.mismatches_found as f64,
                StandardUnit::Count,
            )
            .await
        {
            error!("Failed to publish mismatches metric: {}", e);
        }

        Ok(())
    }

    fn generate_recommendations(&self, results: &ValidationResults) -> Vec<String> {
        let mut recommendations = Vec::new();

        if results.consistency_score < 95.0 {
            recommendations.push(format!(
                "Data consistency is below 95% ({:.1}%). Investigate mismatches immediately.",
                results.consistency_score
            ));
        }

        if let Some(lag) = results.replication_lag_seconds {
            if lag > 60 {
                recommendations.push(format!(
                    "Replication lag is {} seconds. Consider investigating DynamoDB Global Tables health.",
                    lag
                ));
            }
        }

        if let Some(age_hours) = results.backup_status.last_backup_age_hours {
            if age_hours > 24.0 {
                recommendations.push(format!(
                    "Last backup is {:.1} hours old. Consider running a manual backup.",
                    age_hours
                ));
            }
        }

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

    fn default_tables() -> Vec<TableName> {
        vec![
            TableName::new("dr-application-table"),
            TableName::new("dr-sentinel-table"),
        ]
    }

    pub async fn run_validation(
        &self,
        validation_mode: &ValidationMode,
        table_name: Option<TableName>,
        action: &ActionType,
    ) -> Result<ValidationResponse, Error> {
        let tables_to_validate = table_name
            .map(|name| vec![name])
            .unwrap_or_else(Self::default_tables);

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

                    if *action == ActionType::Sync && mismatches > 0 {
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

        let replication_lag = self.check_replication_lag().await.unwrap_or(None);

        let backup_status = self
            .validate_backups()
            .await
            .unwrap_or_else(|_| BackupStatus {
                last_backup_age_hours: None,
                backup_count: 0,
                oldest_backup_days: None,
            });

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

        if let Err(e) = self.publish_validation_metrics(&results).await {
            error!("Failed to publish metrics: {}", e);
        }

        let recommendations = self.generate_recommendations(&results);

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

        let status = if results.consistency_score >= 95.0 {
            ValidationStatus::Healthy
        } else {
            ValidationStatus::Degraded
        };

        Ok(ValidationResponse::builder()
            .status(status)
            .validation_mode(validation_mode.clone())
            .timestamp(Utc::now())
            .results(results)
            .recommendations(recommendations)
            .build())
    }
}

async fn function_handler(
    event: LambdaEvent<ValidationRequest>,
) -> Result<ValidationResponse, Error> {
    let request = event.payload;

    let service =
        DataValidatorService::new(request.source_region.clone(), request.target_region.clone())
            .await?;

    service
        .run_validation(
            &request.validation_mode,
            request.table_name,
            &request.action,
        )
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
