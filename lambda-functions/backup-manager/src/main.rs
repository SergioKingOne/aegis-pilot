use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoClient;
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use serde_dynamo::{from_items, to_item};
use tracing::info;

#[derive(Deserialize)]
struct Request {
    table_name: String,
    backup_type: Option<String>, // "full" or "incremental"
}

#[derive(Serialize)]
struct Response {
    status: String,
    backup_id: String,
    timestamp: String,
    items_backed_up: usize,
}

// This struct is used to serialize/deserialize data to/from DynamoDB
#[derive(Serialize, Deserialize, Debug)]
struct BackupMetadata {
    backup_id: String,
    table_name: String,
    timestamp: String,
    items_count: usize,
    status: String,
}

// This is a generic struct that can be serialized from DynamoDB items
#[derive(Serialize, Deserialize, Debug)]
struct GenericItem {
    #[serde(flatten)]
    attributes: std::collections::HashMap<String, serde_json::Value>,
}

struct BackupManagerService {
    dynamo_client: DynamoClient,
    s3_client: S3Client,
    backup_bucket: String,
    metadata_table: String,
}

impl BackupManagerService {
    async fn new() -> Result<Self, Error> {
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;

        let backup_bucket = std::env::var("BACKUP_BUCKET")
            .unwrap_or_else(|_| "dr-demo-backup-bucket-primary".to_string());
        let metadata_table =
            std::env::var("METADATA_TABLE").unwrap_or_else(|_| "dr-backup-metadata".to_string());

        Ok(Self {
            dynamo_client: DynamoClient::new(&config),
            s3_client: S3Client::new(&config),
            backup_bucket,
            metadata_table,
        })
    }

    async fn create_backup(
        &self,
        table_name: &str,
        backup_type: &str,
    ) -> Result<(String, usize), Error> {
        let backup_id = format!("{}-{}-{}", table_name, backup_type, Utc::now().timestamp());

        // Scan the table (for demo purposes - in production, use DynamoDB's built-in backup)
        let mut items = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut scan_request = self.dynamo_client.scan().table_name(table_name);

            if let Some(key) = last_evaluated_key {
                scan_request = scan_request.set_exclusive_start_key(Some(key));
            }

            let result = scan_request.send().await?;

            // Convert DynamoDB items to a generic format
            if let Some(scan_items) = result.items {
                let generic_items: Vec<GenericItem> = from_items(scan_items)?;
                items.extend(generic_items);
            }

            if result.last_evaluated_key.is_none() {
                break;
            }

            last_evaluated_key = result.last_evaluated_key;
        }

        // Convert items to JSON and upload to S3
        let backup_data = serde_json::to_string(&items)?;
        let key = format!("backups/{}/{}.json", table_name, backup_id);

        self.s3_client
            .put_object()
            .bucket(&self.backup_bucket)
            .key(&key)
            .body(backup_data.into_bytes().into())
            .send()
            .await?;

        info!("Created backup {} with {} items", backup_id, items.len());

        Ok((backup_id, items.len()))
    }

    async fn update_backup_metadata(
        &self,
        backup_id: &str,
        table_name: &str,
        items_count: usize,
    ) -> Result<(), Error> {
        let metadata = BackupMetadata {
            backup_id: backup_id.to_string(),
            table_name: table_name.to_string(),
            timestamp: Utc::now().timestamp().to_string(),
            items_count,
            status: "completed".to_string(),
        };

        // Convert to DynamoDB item
        let item = to_item(metadata)?;

        self.dynamo_client
            .put_item()
            .table_name(&self.metadata_table)
            .set_item(Some(item))
            .send()
            .await?;

        Ok(())
    }

    async fn run_backup(&self, table_name: &str, backup_type: &str) -> Result<Response, Error> {
        // Create backup
        let (backup_id, items_count) = self.create_backup(table_name, backup_type).await?;

        // Update metadata
        self.update_backup_metadata(&backup_id, table_name, items_count)
            .await?;

        Ok(Response {
            status: "success".to_string(),
            backup_id,
            timestamp: Utc::now().to_rfc3339(),
            items_backed_up: items_count,
        })
    }
}

async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let service = BackupManagerService::new().await?;

    let table_name = &event.payload.table_name;
    let backup_type = event
        .payload
        .backup_type
        .unwrap_or_else(|| "full".to_string());

    service.run_backup(table_name, &backup_type).await
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    run(service_fn(function_handler)).await
}
