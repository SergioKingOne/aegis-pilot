use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client as DynamoClient;
use chrono::Utc;
use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct Request {
    action: String,        // "failover" or "failback"
    target_region: String, // Region to failover/failback to
    force: Option<bool>,   // Force failover even if health checks fail
}

#[derive(Serialize)]
struct Response {
    status: String,
    message: String,
    action: String,
    timestamp: String,
}

struct FailoverService {
    dynamo_client: DynamoClient,
    current_region: String,
}

impl FailoverService {
    async fn new() -> Result<Self, Error> {
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;

        let current_region = std::env::var("AWS_REGION")?;

        Ok(Self {
            dynamo_client: DynamoClient::new(&config),
            current_region,
        })
    }

    async fn check_health(&self, region: &str) -> Result<bool, Error> {
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

    async fn update_failover_status(&self, to_region: &str, action: &str) -> Result<(), Error> {
        let _item = aws_sdk_dynamodb::types::AttributeValue::M(
            [
                (
                    "id".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::S("failover_status".to_string()),
                ),
                (
                    "timestamp".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::N(Utc::now().timestamp().to_string()),
                ),
                (
                    "action".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::S(action.to_string()),
                ),
                (
                    "source_region".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::S(self.current_region.clone()),
                ),
                (
                    "target_region".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::S(to_region.to_string()),
                ),
                (
                    "status".to_string(),
                    aws_sdk_dynamodb::types::AttributeValue::S("completed".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );

        self.dynamo_client
            .put_item()
            .table_name("dr-metadata")
            .item(
                "id",
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

    async fn execute_failover(&self, target_region: &str, force: bool) -> Result<Response, Error> {
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

    async fn execute_failback(&self, target_region: &str, force: bool) -> Result<Response, Error> {
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

    async fn handle_request(
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

async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let service = FailoverService::new().await?;

    let action = &event.payload.action;
    let target_region = &event.payload.target_region;
    let force = event.payload.force.unwrap_or(false);

    service.handle_request(action, target_region, force).await
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    run(service_fn(function_handler)).await
}
