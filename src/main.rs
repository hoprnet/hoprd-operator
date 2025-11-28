use k8s_openapi::api::core::v1::{Endpoints};
use kube::{Api, Client, Result};
use rustls::crypto::ring;
use std::{env, sync::Arc, time::Duration};

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
use tracing::{info, warn};
use tracing_subscriber::{Layer, layer::SubscriberExt};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize logger
    init_logger().expect("Failed to initialize logger");
    let version = env!("CARGO_PKG_VERSION");
    info!("Starting hoprd-operator {}", version);

    // 2. Load operator configuration
    let operator_config = load_operator_config().await;

    // 3. Determine operator mode and start appropriate components  
    let mode = std::env::var("OPERATOR_MODE").unwrap_or_else(|_| "controller".into());
    match mode.as_str() {
        "webhook" => {
            info!("Starting in Webhook mode");
            start_webhook(operator_config.clone()).await;
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

async fn start_webhook(operator_config: OperatorConfig) {
    // 1. Start webhook server in a separate task
    let webhook_server =tokio::spawn(async move {
        webhook_server::run_webhook_server(operator_config.webhook).await;
    });

    let webhook_boot = webhook_server::wait_for_webhook_ready().await;
    if webhook_boot.is_err() {
        panic!("Webhook server failed to start: {}", webhook_boot.err().unwrap());
    }

    // 2. Wait for pod to be ready
    //wait_for_service_ready(client.clone()).await;

    // 3. Keep the webhook server running
    webhook_server.await.expect("Webhook server task panicked");

}

/// Load operator configuration from file based on environment
async fn load_operator_config() -> OperatorConfig {
    let operator_environment = env::var(constants::OPERATOR_ENVIRONMENT).expect("The OPERATOR_ENVIRONMENT environment variable is not set");
    let config_path = if operator_environment.eq("production") {
        "/app/config/config.yaml".to_owned()
    } else {
        let mut path = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
        path.push_str(&format!("/test-data/sample_config-{operator_environment}.yaml"));
        path
    };
    info!("Loading operator configuration from: {}", config_path);
    let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
    let config: OperatorConfig = serde_yml::from_reader(config_file).expect("Could not read contents of config file.");
    config
}

// Wait for the operator Service endpoint to be in Ready state
async fn wait_for_service_ready(client: Client) -> () {
    if env::var(constants::OPERATOR_ENVIRONMENT).unwrap() != "production" {
        info!("Skipping Pod readiness check in non Kubernetes environment");
        return ();
    }

    let service_namespace = env::var("POD_NAMESPACE").expect("The POD_NAMESPACE environment variable is not set");
    let service_name = format!("{}-webhook", service_namespace); // TODO: Assuming chart name is same as namespace
    info!("Waiting for Service Endpoint {}/{} to be Ready...", service_namespace, service_name);

    let endpoints: Api<Endpoints> = Api::namespaced(client, service_namespace.as_str());

    loop {
        match endpoints.get(&service_name).await {
            Ok(endpoint) => {
                if let Some(subsets) = endpoint.subsets {
                    let ready_addresses: usize = subsets
                        .iter()
                        .flat_map(|subset| subset.addresses.as_ref().unwrap_or(&vec![]).clone())
                        .count();
                    if ready_addresses > 0 {
                        info!("Service {}/{} has {} ready endpoint(s)", service_namespace, service_name, ready_addresses);
                        tokio::time::sleep(Duration::from_millis(5000)).await;
                        return;
                    } else {
                        warn!("Service {}/{} has no ready addresses yet — waiting…", service_namespace, service_name);
                    }
                } else {
                    warn!("Service {}/{} has no subsets yet — waiting…", service_namespace, service_name);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(e) => {
                warn!("Failed to get Endpoints for Service {}/{}: {} — retrying…", service_namespace, service_name, e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        }
    }
}

// Start all Kubernetes controllers
async fn start_controllers(operator_config: operator_config::OperatorConfig) {
    // ⭐ 4. Initialize Kubernetes client and context data
    info!("Initializing Context Data...");
    ring::default_provider().install_default().expect("failed to install rustls ring CryptoProvider");
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone(), operator_config).await);
    ContextData::sync_identities(context_data.clone()).await;

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
