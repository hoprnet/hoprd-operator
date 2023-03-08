use kube::Result;
pub mod model;
mod hoprd_deployment;
mod hoprd;
mod hoprd_ingress;
mod hoprd_jobs;
mod hoprd_secret;
mod hoprd_service;
mod hoprd_service_monitor;
mod servicemonitor;
mod controller;
mod utils;
mod constants;

#[tokio::main]
async fn main() -> Result<()> {    
    println!("[INFO] Starting hoprd-operator");
    // Initiatilize Kubernetes controller state
    let _controller = controller::run().await;
    Ok(())
}

