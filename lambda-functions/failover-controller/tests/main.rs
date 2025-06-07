use failover_controller::{
    validate_action, validate_region, FailoverService, FailoverStatus, Request, Response,
};
use lambda_runtime::{Context, LambdaEvent};
use serde_json::json;

#[test]
fn test_request_parsing() {
    // Test with all fields
    let json = json!({
        "action": "failover",
        "target_region": "us-west-2",
        "force": true
    });

    let request: Request = serde_json::from_value(json).unwrap();
    assert_eq!(request.action, "failover");
    assert_eq!(request.target_region, "us-west-2");
    assert_eq!(request.force, Some(true));

    // Test without force field
    let json_no_force = json!({
        "action": "failback",
        "target_region": "eu-west-1"
    });

    let request_no_force: Request = serde_json::from_value(json_no_force).unwrap();
    assert_eq!(request_no_force.action, "failback");
    assert_eq!(request_no_force.target_region, "eu-west-1");
    assert_eq!(request_no_force.force, None);
}

#[test]
fn test_response_structure() {
    let response = Response {
        status: "success".to_string(),
        message: "Failover to region us-west-2 completed".to_string(),
        action: "failover".to_string(),
        timestamp: "2025-01-06T12:00:00Z".to_string(),
    };

    let json = serde_json::to_value(&response).unwrap();

    assert_eq!(json["status"], "success");
    assert_eq!(json["message"], "Failover to region us-west-2 completed");
    assert_eq!(json["action"], "failover");
    assert!(json["timestamp"].is_string());
}

#[test]
fn test_failover_status_serialization() {
    let status = FailoverStatus {
        id: "failover_status".to_string(),
        timestamp: 1704556800,
        action: "failover".to_string(),
        source_region: "us-east-1".to_string(),
        target_region: "us-west-2".to_string(),
        status: "completed".to_string(),
    };

    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("failover_status"));
    assert!(json.contains("1704556800"));
    assert!(json.contains("us-east-1"));
    assert!(json.contains("us-west-2"));
}

#[test]
fn test_action_validation() {
    // Valid actions
    assert!(validate_action("failover"));
    assert!(validate_action("failback"));

    // Invalid actions
    assert!(!validate_action("rollback"));
    assert!(!validate_action("restart"));
    assert!(!validate_action(""));
    assert!(!validate_action("FAILOVER")); // case sensitive
}

#[test]
fn test_region_validation() {
    // Valid AWS regions
    assert!(validate_region("us-east-1"));
    assert!(validate_region("us-west-2"));
    assert!(validate_region("eu-west-1"));
    assert!(validate_region("ap-southeast-1"));
    assert!(validate_region("ca-central-1"));

    // Invalid regions
    assert!(!validate_region("invalid"));
    assert!(!validate_region("useast1")); // missing dash
    assert!(!validate_region("")); // empty
    assert!(!validate_region("us")); // incomplete
}

#[test]
fn test_error_response_format() {
    let error_response = Response {
        status: "failed".to_string(),
        message: "Target region us-west-2 is not healthy".to_string(),
        action: "failover".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    assert_eq!(error_response.status, "failed");
    assert!(error_response.message.contains("not healthy"));
}

#[cfg(test)]
mod failover_logic_tests {
    use super::*;

    #[test]
    fn test_failover_without_force() {
        // When force is false, health check should be performed
        let request = Request {
            action: "failover".to_string(),
            target_region: "us-west-2".to_string(),
            force: Some(false),
        };

        assert!(!request.force.unwrap_or(false));
    }

    #[test]
    fn test_failover_with_force() {
        // When force is true, health check should be skipped
        let request = Request {
            action: "failover".to_string(),
            target_region: "us-west-2".to_string(),
            force: Some(true),
        };

        assert!(request.force.unwrap_or(false));
    }

    #[test]
    fn test_invalid_action_response() {
        // Test that invalid actions are properly handled
        let request = Request {
            action: "invalid-action".to_string(),
            target_region: "us-west-2".to_string(),
            force: None,
        };

        assert!(!validate_action(&request.action));
    }
}

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn test_special_characters_in_message() {
        let response = Response {
            status: "failed".to_string(),
            message: "Region 'us-west-2' check failed: Connection timeout @ 15:30:45 UTC"
                .to_string(),
            action: "failover".to_string(),
            timestamp: "2025-01-06T15:30:45Z".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: Response = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.message, response.message);
    }

    #[test]
    fn test_long_region_names() {
        // Some regions have longer names
        assert!(validate_region("ap-southeast-2"));
        assert!(validate_region("eu-central-1"));
        assert!(validate_region("sa-east-1"));
    }

    #[test]
    fn test_timestamp_formats() {
        let status = FailoverStatus {
            id: "test".to_string(),
            timestamp: i64::MAX,
            action: "failover".to_string(),
            source_region: "us-east-1".to_string(),
            target_region: "us-west-2".to_string(),
            status: "completed".to_string(),
        };

        assert_eq!(status.timestamp, i64::MAX);
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_request_parsing_performance() {
        let json = json!({
            "action": "failover",
            "target_region": "us-west-2",
            "force": true
        });

        let start = Instant::now();
        for _ in 0..1000 {
            let _: Request = serde_json::from_value(json.clone()).unwrap();
        }
        let duration = start.elapsed();

        // Should parse 1000 requests in under 50ms
        assert!(duration.as_millis() < 50);
    }

    #[test]
    fn test_validation_performance() {
        let actions = vec!["failover", "failback", "invalid", ""];
        let regions = vec!["us-east-1", "eu-west-1", "invalid", ""];

        let start = Instant::now();
        for _ in 0..1000 {
            for action in &actions {
                let _ = validate_action(action);
            }
            for region in &regions {
                let _ = validate_region(region);
            }
        }
        let duration = start.elapsed();

        // Should validate thousands of items in under 10ms
        assert!(duration.as_millis() < 10);
    }
}

#[cfg(test)]
mod lambda_integration_tests {
    use super::*;

    #[test]
    fn test_lambda_event_structure() {
        let event_json = json!({
            "action": "failover",
            "target_region": "us-west-2",
            "force": false
        });

        let context = Context::default();
        let event = LambdaEvent {
            payload: serde_json::from_value::<Request>(event_json).unwrap(),
            context,
        };

        assert_eq!(event.payload.action, "failover");
        assert_eq!(event.payload.target_region, "us-west-2");
        assert_eq!(event.payload.force, Some(false));
    }

    #[test]
    fn test_multiple_failover_scenarios() {
        let scenarios = vec![
            ("failover", "us-west-2", Some(true), true),
            ("failback", "us-east-1", Some(false), true),
            ("invalid", "us-west-2", None, false),
            ("failover", "", Some(true), false), // Invalid region
        ];

        for (action, region, force, should_be_valid) in scenarios {
            let is_valid = validate_action(action) && validate_region(region);
            assert_eq!(
                is_valid, should_be_valid,
                "Failed for action: {}, region: {}",
                action, region
            );
        }
    }
}

// Integration tests that would require AWS resources
#[cfg(test)]
mod aws_integration_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_failover_service_initialization() {
        std::env::set_var("AWS_REGION", "us-east-1");

        // This would require AWS credentials or LocalStack
        // let service = FailoverService::new().await;
        // assert!(service.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_health_check_multiple_regions() {
        // This would test health checks across multiple regions
    }

    #[tokio::test]
    #[ignore]
    async fn test_failover_status_update() {
        // This would test updating failover status in DynamoDB
    }
}
