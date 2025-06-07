use backup_manager::{BackupManagerService, Request, Response};
use lambda_runtime::{run, service_fn, Error, LambdaEvent};

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
