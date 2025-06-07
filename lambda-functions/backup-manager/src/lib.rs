use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoClient;
use aws_sdk_s3::Client as S3Client;
use chrono::Utc;
use lambda_runtime::Error;
use serde::{Deserialize, Serialize};
use serde_dynamo::{from_items, to_item};
use tracing::info;

#[derive(Deserialize, Debug, Clone)]
pub struct Request {
    pub table_name: String,
    pub backup_type: Option<String>, // "full" or "incremental"
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Response {
    pub status: String,
    pub backup_id: String,
    pub timestamp: String,
    pub items_backed_up: usize,
}

// This struct is used to serialize/deserialize data to/from DynamoDB
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BackupMetadata {
    pub backup_id: String,
    pub table_name: String,
    pub timestamp: String,
    pub items_count: usize,
    pub status: String,
}

// This is a generic struct that can be serialized from DynamoDB items
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenericItem {
    #[serde(flatten)]
    pub attributes: std::collections::HashMap<String, serde_json::Value>,
}

pub struct BackupManagerService {
    pub dynamo_client: DynamoClient,
    pub s3_client: S3Client,
    pub backup_bucket: String,
    pub metadata_table: String,
}

impl BackupManagerService {
    pub async fn new() -> Result<Self, Error> {
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

    pub async fn create_backup(
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

    pub async fn update_backup_metadata(
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

    pub async fn run_backup(&self, table_name: &str, backup_type: &str) -> Result<Response, Error> {
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

// Utility functions for testing
pub fn generate_backup_id(table_name: &str, backup_type: &str, timestamp: i64) -> String {
    format!("{}-{}-{}", table_name, backup_type, timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"table_name": "test-table", "backup_type": "incremental"}"#;
        let request: Request = serde_json::from_str(json).unwrap();
        assert_eq!(request.table_name, "test-table");
        assert_eq!(request.backup_type, Some("incremental".to_string()));
    }

    #[test]
    fn test_response_serialization() {
        let response = Response {
            status: "success".to_string(),
            backup_id: "test-123".to_string(),
            timestamp: "2025-01-06T12:00:00Z".to_string(),
            items_backed_up: 100,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("test-123"));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_backup_metadata() {
        let metadata = BackupMetadata {
            backup_id: "backup-123".to_string(),
            table_name: "test-table".to_string(),
            timestamp: "1234567890".to_string(),
            items_count: 50,
            status: "completed".to_string(),
        };

        assert_eq!(metadata.backup_id, "backup-123");
        assert_eq!(metadata.items_count, 50);
    }

    #[test]
    fn test_generic_item_serialization() {
        use std::collections::HashMap;
        
        let mut attributes = HashMap::new();
        attributes.insert("id".to_string(), serde_json::json!("123"));
        attributes.insert("name".to_string(), serde_json::json!("test"));
        
        let item = GenericItem { attributes };
        
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"id\":\"123\""));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_backup_id_generation() {
        let id = generate_backup_id("my-table", "full", 1234567890);
        assert_eq!(id, "my-table-full-1234567890");
    }
}
