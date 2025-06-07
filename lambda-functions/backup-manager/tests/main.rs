use backup_manager::{generate_backup_id, BackupMetadata, GenericItem, Request, Response};
use lambda_runtime::{Context, LambdaEvent};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_request_parsing() {
    // Test with full request
    let json = json!({
        "table_name": "my-table",
        "backup_type": "incremental"
    });

    let request: Request = serde_json::from_value(json).unwrap();
    assert_eq!(request.table_name, "my-table");
    assert_eq!(request.backup_type, Some("incremental".to_string()));

    // Test with minimal request
    let json_minimal = json!({
        "table_name": "another-table"
    });

    let request_minimal: Request = serde_json::from_value(json_minimal).unwrap();
    assert_eq!(request_minimal.table_name, "another-table");
    assert_eq!(request_minimal.backup_type, None);
}

#[test]
fn test_response_structure() {
    let response = Response {
        status: "success".to_string(),
        backup_id: "table-full-1234567890".to_string(),
        timestamp: "2025-01-06T12:00:00Z".to_string(),
        items_backed_up: 150,
    };

    let json = serde_json::to_value(&response).unwrap();

    assert_eq!(json["status"], "success");
    assert_eq!(json["backup_id"], "table-full-1234567890");
    assert_eq!(json["items_backed_up"], 150);
}

#[test]
fn test_backup_metadata_serialization() {
    let metadata = BackupMetadata {
        backup_id: "test-backup-123".to_string(),
        table_name: "test-table".to_string(),
        timestamp: "1234567890".to_string(),
        items_count: 75,
        status: "completed".to_string(),
    };

    // Test serialization
    let json = serde_json::to_string(&metadata).unwrap();
    assert!(json.contains("test-backup-123"));
    assert!(json.contains("75"));

    // Test deserialization
    let deserialized: BackupMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.backup_id, metadata.backup_id);
    assert_eq!(deserialized.items_count, metadata.items_count);
}

#[test]
fn test_generic_item_handling() {
    let mut attributes = HashMap::new();
    attributes.insert("id".to_string(), json!("user-123"));
    attributes.insert("name".to_string(), json!("John Doe"));
    attributes.insert("age".to_string(), json!(30));
    attributes.insert("active".to_string(), json!(true));

    let item = GenericItem {
        attributes: attributes.clone(),
    };

    // Test serialization preserves all types
    let json_str = serde_json::to_string(&item).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["id"], "user-123");
    assert_eq!(parsed["name"], "John Doe");
    assert_eq!(parsed["age"], 30);
    assert_eq!(parsed["active"], true);
}

#[test]
fn test_backup_id_format() {
    // Test standard format
    let id = generate_backup_id("users-table", "full", 1704556800);
    assert_eq!(id, "users-table-full-1704556800");

    // Test with special characters in table name
    let id_special = generate_backup_id("users-table-prod", "incremental", 1704556800);
    assert_eq!(id_special, "users-table-prod-incremental-1704556800");
}

#[test]
fn test_lambda_event_structure() {
    let event_json = json!({
        "table_name": "production-data",
        "backup_type": "full"
    });

    let context = Context::default();
    let event = LambdaEvent {
        payload: serde_json::from_value::<Request>(event_json).unwrap(),
        context,
    };

    assert_eq!(event.payload.table_name, "production-data");
    assert_eq!(event.payload.backup_type, Some("full".to_string()));
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn test_empty_table_name_handling() {
        let json = json!({
            "table_name": "",
            "backup_type": "full"
        });

        let request: Request = serde_json::from_value(json).unwrap();
        assert_eq!(request.table_name, "");
        // In production, this should be validated
    }

    #[test]
    fn test_invalid_backup_type() {
        let json = json!({
            "table_name": "test",
            "backup_type": "invalid-type"
        });

        let request: Request = serde_json::from_value(json).unwrap();
        assert_eq!(request.backup_type, Some("invalid-type".to_string()));
        // In production, this should be validated to only accept "full" or "incremental"
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_large_item_serialization_performance() {
        let mut large_attributes = HashMap::new();

        // Create a large item with 1000 attributes
        for i in 0..1000 {
            large_attributes.insert(format!("field_{}", i), json!(format!("value_{}", i)));
        }

        let item = GenericItem {
            attributes: large_attributes,
        };

        let start = Instant::now();
        let json = serde_json::to_string(&item).unwrap();
        let duration = start.elapsed();

        // Should serialize in under 10ms even for large items
        assert!(duration.as_millis() < 10);
        assert!(json.len() > 10000); // Ensure it's actually a large JSON
    }

    #[test]
    fn test_backup_metadata_batch_processing() {
        let start = Instant::now();

        // Simulate processing 100 backup metadata records
        for i in 0..100 {
            let metadata = BackupMetadata {
                backup_id: format!("backup-{}", i),
                table_name: "test-table".to_string(),
                timestamp: i.to_string(),
                items_count: i * 10,
                status: "completed".to_string(),
            };

            let _ = serde_json::to_string(&metadata).unwrap();
        }

        let duration = start.elapsed();

        // Should process 100 records in under 50ms
        assert!(duration.as_millis() < 50);
    }
}

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn test_unicode_in_item_attributes() {
        let mut attributes = HashMap::new();
        attributes.insert("name".to_string(), json!("测试用户")); // Chinese
        attributes.insert("emoji".to_string(), json!("🚀🔥💯"));
        attributes.insert("mixed".to_string(), json!("Hello мир 世界"));

        let item = GenericItem { attributes };

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: GenericItem = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.attributes.get("name").unwrap(),
            &json!("测试用户")
        );
        assert_eq!(
            deserialized.attributes.get("emoji").unwrap(),
            &json!("🚀🔥💯")
        );
    }

    #[test]
    fn test_nested_json_in_attributes() {
        let mut attributes = HashMap::new();
        attributes.insert(
            "user".to_string(),
            json!({
                "id": 123,
                "profile": {
                    "name": "Test User",
                    "preferences": ["option1", "option2"]
                }
            }),
        );

        let item = GenericItem { attributes };

        let json = serde_json::to_string(&item).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["user"]["id"], 123);
        assert_eq!(parsed["user"]["profile"]["name"], "Test User");
        assert!(parsed["user"]["profile"]["preferences"].is_array());
    }
}

// Integration tests that would require AWS resources
#[cfg(test)]
mod integration_tests {
    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_backup_service_initialization() {
        std::env::set_var("BACKUP_BUCKET", "test-bucket");
        std::env::set_var("METADATA_TABLE", "test-metadata");

        // This would require AWS credentials or LocalStack
        // let service = BackupManagerService::new().await;
        // assert!(service.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_full_backup_workflow() {
        // This would test the complete backup workflow
        // including DynamoDB scanning and S3 upload
    }
}
