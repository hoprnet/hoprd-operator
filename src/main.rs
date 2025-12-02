use kube::{Client, Result};
use rustls::crypto::ring;
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
mod operator_config;
mod resource_generics;
mod servicemonitor;
mod utils;
mod webhook_server;

use crate::{context_data::ContextData, operator_config::OperatorConfig};
use futures::{
    future::FutureExt, // for `.fuse()`
    pin_mut,
    select,
};
use tracing::{info};
use tracing_subscriber::{Layer, layer::SubscriberExt};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize logger
    init_logger().expect("Failed to initialize logger");
    let version = env!("CARGO_PKG_VERSION");
    info!("Starting hoprd-operator {}", version);

    // 2. Load operator configuration
    let operator_config = load_operator_config().await.expect("Failed to load operator configuration");

    // 3. Determine operator mode and start appropriate components  
    let mode = std::env::var("OPERATOR_MODE").unwrap_or_else(|_| "controller".into());
    match mode.as_str() {
        "webhook" => {
            info!("Starting in Webhook mode");
            webhook_server::run_webhook_server(operator_config.webhook).await;
        }
        "controller" => {
            info!("Starting in Controller mode");
            start_controllers(operator_config.clone()).await;
        }
        _ => {
            panic!("Invalid OPERATOR_MODE: {}. Must be either 'webhook' or 'controller'", mode);
        }
    }

    Ok(())
}

/// Load operator configuration from file based on environment
async fn load_operator_config() -> Result<OperatorConfig, String> {
    let operator_environment = env::var(constants::OPERATOR_ENVIRONMENT).expect("The OPERATOR_ENVIRONMENT environment variable is not set");
    let config_path = if operator_environment.eq("production") {
        "/app/config/config.yaml".to_owned()
    } else {
        let mut path = env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {}", e))?
            .to_str()
            .ok_or_else(|| "Current directory path contains invalid UTF-8".to_string())?
            .to_owned();
        path.push_str(&format!("/test-data/sample_config-{operator_environment}.yaml"));
        path
    };
    info!("Loading operator configuration from: {}", config_path);
    let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
    let config: OperatorConfig = serde_yml::from_reader(config_file).expect("Could not read contents of config file.");
    Ok(config)
}

// Start all Kubernetes controllers
async fn start_controllers(operator_config: operator_config::OperatorConfig) {
    // ⭐ 4. Initialize Kubernetes client and context data
    info!("Initializing Context Data...");
    ring::default_provider().install_default().expect("failed to install rustls ring CryptoProvider");
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone(), operator_config).await);
    ContextData::sync_identities(context_data.clone()).await.expect("Failed to sync identities");

    // ⭐ 5. Initiatilize Kubernetes controllers
    info!("Starting Controllers...");
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
}


    // let subscriber = FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    // tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

fn init_logger() -> anyhow::Result<()> {
    let env_filter = match tracing_subscriber::EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => tracing_subscriber::filter::EnvFilter::new("info")
            .add_directive("kube=info".parse()?)
            .add_directive("kube_client=info".parse()?)
    };

    let registry = tracing_subscriber::Registry::default().with(env_filter);

    let format = tracing_subscriber::fmt::layer()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(false);

    let format = format.json().boxed();

    let registry = registry.with(format);

    tracing::subscriber::set_global_default(registry)?;

    Ok(())
}
