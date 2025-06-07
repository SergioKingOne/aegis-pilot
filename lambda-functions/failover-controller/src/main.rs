use failover_controller::{FailoverService, Request, Response};
use lambda_runtime::{run, service_fn, Error, LambdaEvent};

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
