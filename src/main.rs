use std::{sync::Arc, env};
use kube::{Result, Client};
pub mod model;

mod operator_config;
mod hoprd_deployment;
mod hoprd_deployment_spec;
mod hoprd;
mod cluster;
mod identity_hoprd;
mod identity_hoprd_persistence;
mod bootstrap_operator;
mod hoprd_ingress;
mod hoprd_service;
mod identity_pool;
mod identity_pool_service_monitor;
mod identity_pool_service_account;
mod servicemonitor;
mod controller_cluster;
mod controller_hoprd;
mod controller_identity;
mod controller_identity_pool;
mod context_data;
mod utils;
mod constants;

use crate:: context_data::ContextData;
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};
use tracing::info;
use tracing_subscriber::{FmtSubscriber, filter::EnvFilter};

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
    let controller_identity_pool = controller_identity_pool::run(client.clone(), context_data.clone()).fuse();
    let controller_identity_hoprd = controller_identity::run(client.clone(), context_data.clone()).fuse();
    let controller_hoprd = controller_hoprd::run(client.clone(), context_data.clone()).fuse();
    let controller_cluster = controller_cluster::run(client.clone(), context_data.clone()).fuse();

    pin_mut!(controller_identity_pool, controller_identity_hoprd, controller_hoprd, controller_cluster);
    select! {
        () = controller_identity_pool => println!("Controller IdentityPool exited"),
        () = controller_identity_hoprd => println!("Controller IdentityHoprd exited"),
        () = controller_hoprd => println!("Controller Hoprd exited"),
        () = controller_cluster => println!("Controller ClusterHoprd exited"),
    }

    Ok(())
}

