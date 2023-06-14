use std::{sync::{Arc}, env};
use kube::{Result, Client};
pub mod model;
mod hoprd_persistence;
mod operator_config;
mod hoprd_deployment;
mod hoprd_deployment_spec;
mod hoprd;
mod cluster;
mod bootstrap_operator;
mod hoprd_ingress;
mod hoprd_jobs;
mod hoprd_secret;
mod hoprd_service;
mod hoprd_service_monitor;
mod servicemonitor;
mod controller_hoprd;
mod controller_cluster;
mod context_data;
mod utils;
mod constants;

use crate:: context_data::ContextData;
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};
use tracing::{info};
use tracing_subscriber::{FmtSubscriber, filter::{EnvFilter}};

#[tokio::main]
async fn main() -> Result<()> {

    // Initialize Tracing crate
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let version: &str = env!("CARGO_PKG_VERSION");
    info!("Starting hoprd-operator {}", version);
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone()).await);
    // Initiatilize Kubernetes controller state
    bootstrap_operator::start(client.clone(), context_data.clone()).await;
    let controller_hoprd = controller_hoprd::run(client.clone(), context_data.clone()).fuse();
    let controller_cluster = controller_cluster::run(client.clone(), context_data.clone()).fuse();

    pin_mut!(controller_hoprd, controller_cluster);
    select! {
        () = controller_hoprd => println!("Controller Hoprd exited"),
        () = controller_cluster => println!("Controller ClusterHoprd exited"),
    }

    Ok(())
}

