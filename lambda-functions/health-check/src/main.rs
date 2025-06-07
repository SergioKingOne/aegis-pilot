use health_check::{HealthCheckService, Request, Response};
use lambda_runtime::{run, service_fn, Error, LambdaEvent};

async fn function_handler(event: LambdaEvent<Request>) -> Result<Response, Error> {
    let service = HealthCheckService::new(event.payload.region).await?;
    service.run_health_check().await
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    run(service_fn(function_handler)).await
}
