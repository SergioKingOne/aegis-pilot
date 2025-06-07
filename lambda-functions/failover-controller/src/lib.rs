use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoClient;
use chrono::Utc;
use lambda_runtime::Error;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[derive(Deserialize, Debug, Clone)]
pub struct Request {
    pub action: String,        // "failover" or "failback"
    pub target_region: String, // Region to failover/failback to
    pub force: Option<bool>,   // Force failover even if health checks fail
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Response {
    pub status: String,
    pub message: String,
    pub action: String,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FailoverStatus {
    pub id: String,
    pub timestamp: i64,
    pub action: String,
    pub source_region: String,
    pub target_region: String,
    pub status: String,
}

pub struct FailoverService {
    pub dynamo_client: DynamoClient,
    pub current_region: String,
}

impl FailoverService {
    pub async fn new() -> Result<Self, Error> {
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;

        let current_region = std::env::var("AWS_REGION")?;

        Ok(Self {
            dynamo_client: DynamoClient::new(&config),
            current_region,
        })
    }

    pub async fn check_health(&self, region: &str) -> Result<bool, Error> {
        // In a real implementation, you would do more comprehensive health checks
        // This is a simplified version that just checks if we can connect to DynamoDB

        // Use the current client if checking the current region
        if region == self.current_region {
            let result = self.dynamo_client.list_tables().limit(1).send().await;
            return Ok(result.is_ok());
        }

        // Otherwise, create a client for the target region
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;

        let client = DynamoClient::new(&config);
        let result = client.list_tables().limit(1).send().await;

        Ok(result.is_ok())
    }

    pub async fn update_failover_status(&self, to_region: &str, action: &str) -> Result<(), Error> {
        self.dynamo_client
            .put_item()
            .table_name("dr-backup-metadata")
            .item(
                "backup_id",
                aws_sdk_dynamodb::types::AttributeValue::S("failover_status".to_string()),
            )
            .item(
                "timestamp",
                aws_sdk_dynamodb::types::AttributeValue::N(Utc::now().timestamp().to_string()),
            )
            .item(
                "action",
                aws_sdk_dynamodb::types::AttributeValue::S(action.to_string()),
            )
            .item(
                "source_region",
                aws_sdk_dynamodb::types::AttributeValue::S(self.current_region.clone()),
            )
            .item(
                "target_region",
                aws_sdk_dynamodb::types::AttributeValue::S(to_region.to_string()),
            )
            .item(
                "status",
                aws_sdk_dynamodb::types::AttributeValue::S("completed".to_string()),
            )
            .send()
            .await?;

        Ok(())
    }

    pub async fn execute_failover(
        &self,
        target_region: &str,
        force: bool,
    ) -> Result<Response, Error> {
        info!("Executing failover to region: {}", target_region);

        // Check health of target region
        if !force {
            let is_healthy = self.check_health(target_region).await?;

            if !is_healthy {
                warn!(
                    "Target region {} is not healthy. Use force=true to override.",
                    target_region
                );
                return Ok(Response {
                    status: "failed".to_string(),
                    message: format!("Target region {} is not healthy", target_region),
                    action: "failover".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                });
            }
        }

        // In a real implementation, you would:
        // 1. Update DNS to point to the DR region
        // 2. Promote standby resources to active
        // 3. Scale up resources as needed

        // Update failover status
        self.update_failover_status(target_region, "failover")
            .await?;

        Ok(Response {
            status: "success".to_string(),
            message: format!("Failover to region {} completed", target_region),
            action: "failover".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        })
    }

    pub async fn execute_failback(
        &self,
        target_region: &str,
        force: bool,
    ) -> Result<Response, Error> {
        info!("Executing failback to region: {}", target_region);

        // Check health of target region
        if !force {
            let is_healthy = self.check_health(target_region).await?;

            if !is_healthy {
                warn!(
                    "Target region {} is not healthy. Use force=true to override.",
                    target_region
                );
                return Ok(Response {
                    status: "failed".to_string(),
                    message: format!("Target region {} is not healthy", target_region),
                    action: "failback".to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                });
            }
        }

        // In a real implementation, you would:
        // 1. Verify data synchronization
        // 2. Update DNS to point back to primary region
        // 3. Scale down DR resources

        // Update failover status
        self.update_failover_status(target_region, "failback")
            .await?;

        Ok(Response {
            status: "success".to_string(),
            message: format!("Failback to region {} completed", target_region),
            action: "failback".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        })
    }

    pub async fn handle_request(
        &self,
        action: &str,
        target_region: &str,
        force: bool,
    ) -> Result<Response, Error> {
        match action {
            "failover" => self.execute_failover(target_region, force).await,
            "failback" => self.execute_failback(target_region, force).await,
            _ => {
                error!("Invalid action: {}", action);
                Ok(Response {
                    status: "failed".to_string(),
                    message: format!("Invalid action: {}", action),
                    action: action.to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                })
            }
        }
    }
}

// Utility functions for testing
pub fn validate_action(action: &str) -> bool {
    matches!(action, "failover" | "failback")
}

pub fn validate_region(region: &str) -> bool {
    // Basic validation - in production, you'd check against a list of valid AWS regions
    !region.is_empty() && region.contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"action": "failover", "target_region": "us-west-2", "force": true}"#;
        let request: Request = serde_json::from_str(json).unwrap();
        assert_eq!(request.action, "failover");
        assert_eq!(request.target_region, "us-west-2");
        assert_eq!(request.force, Some(true));
    }

    #[test]
    fn test_response_serialization() {
        let response = Response {
            status: "success".to_string(),
            message: "Failover completed".to_string(),
            action: "failover".to_string(),
            timestamp: "2025-01-06T12:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("Failover completed"));
    }

    #[test]
    fn test_validate_action() {
        assert!(validate_action("failover"));
        assert!(validate_action("failback"));
        assert!(!validate_action("invalid"));
        assert!(!validate_action(""));
    }

    #[test]
    fn test_validate_region() {
        assert!(validate_region("us-east-1"));
        assert!(validate_region("eu-west-1"));
        assert!(!validate_region("invalid"));
        assert!(!validate_region(""));
    }

    #[test]
    fn test_failover_status() {
        let status = FailoverStatus {
            id: "failover_status".to_string(),
            timestamp: 1234567890,
            action: "failover".to_string(),
            source_region: "us-east-1".to_string(),
            target_region: "us-west-2".to_string(),
            status: "completed".to_string(),
        };

        assert_eq!(status.id, "failover_status");
        assert_eq!(status.action, "failover");
    }
}
