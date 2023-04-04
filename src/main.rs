use std::sync::Arc;

use kube::{Result, Client};
pub mod model;
mod hoprd_deployment;
mod hoprd;
mod hoprd_ingress;
mod hoprd_jobs;
mod hoprd_secret;
mod hoprd_service;
mod hoprd_service_monitor;
mod servicemonitor;
mod controller_hoprd;
mod context_data;
mod utils;
mod constants;

use crate:: context_data::ContextData;


#[tokio::main]
async fn main() -> Result<()> {    
    println!("[INFO] Starting hoprd-operator");
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone()).await);
    // Initiatilize Kubernetes controller state
    let _controller = controller_hoprd::run(client, context_data).await;
    Ok(())
}

