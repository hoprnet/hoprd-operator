use kube::{Client, Result};
use std::{env, sync::Arc};

mod bootstrap_operator;
mod cluster;
mod constants;
mod context_data;
mod events;
mod hoprd;
mod identity_hoprd;
mod identity_pool;
pub mod model;
mod resource_generics;
mod webhook_server;
mod operator_config;
mod servicemonitor;
mod utils;

use crate::context_data::ContextData;
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};
use tracing::info;
use tracing_subscriber::{filter::EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize Tracing crate
    let subscriber = FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("Starting hoprd-operator {}", env!("CARGO_PKG_VERSION"));

    // ⭐ 1. Load operator configuration
    let operator_config = context_data::load_operator_config().await; // Preload config to fail fast if invalid

    // ⭐ 2. Start webhook IMMEDIATELY
    let webhook_config = operator_config.webhook.clone();
    let webhook_handle = tokio::spawn(async move {
        webhook_server::run_webhook_server(webhook_config)
            .await;
    });

    // ⭐ 3. Wait until webhook port is ready
    let webhook_boot = webhook_server::wait_for_webhook_ready().await;
    if webhook_boot.is_err() {
        panic!("Webhook server failed to start: {}", webhook_boot.err().unwrap());
    }

    // ⭐ 4. Initialize Kubernetes client and context data
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone(), operator_config).await);
    ContextData::sync_identities(context_data.clone()).await;

    // ⭐ 5. Initiatilize Kubernetes controllers
    bootstrap_operator::start(client.clone(), context_data.clone()).await;
    let controller_identity_pool = identity_pool::identity_pool_controller::run(client.clone(), context_data.clone()).fuse();
    let controller_identity_hoprd = identity_hoprd::identity_hoprd_controller::run(client.clone(), context_data.clone()).fuse();
    let controller_hoprd = hoprd::hoprd_controller::run(client.clone(), context_data.clone()).fuse();
    let controller_cluster = cluster::cluster_controller::run(client.clone(), context_data.clone()).fuse();

    pin_mut!(controller_identity_pool, controller_identity_hoprd, controller_hoprd, controller_cluster);
    select! {
        () = controller_identity_pool => println!("Controller IdentityPool exited"),
        () = controller_identity_hoprd => println!("Controller IdentityHoprd exited"),
        () = controller_hoprd => println!("Controller Hoprd exited"),
        () = controller_cluster => println!("Controller ClusterHoprd exited"),
    }
    webhook_handle.await.expect("Webhook task panicked");

    Ok(())
}
