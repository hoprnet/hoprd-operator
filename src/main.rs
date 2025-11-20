use k8s_openapi::api::core::v1::{Endpoints};
use kube::{Api, Client, Result};
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
use tracing_subscriber::{filter::EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> Result<()> {
    // ⭐ 1. Initialize Tracing Subscriber
    let subscriber = FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    info!("Starting hoprd-operator {}", env!("CARGO_PKG_VERSION"));

    // ⭐ 2. Load operator configuration
    let operator_config = load_operator_config().await;

    // ⭐ 3. Start webhook server in a separate task
    start_webhook_server(operator_config.webhook.clone()).await;

    // ⭐ 4. Initialize Kubernetes client
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");

    // ⭐ 5. Wait for pod to be ready
    wait_for_service_ready(client.clone()).await;

    // ⭐ 6. Start controllers
    start_controllers(operator_config, client.clone()).await;

    Ok(())
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
    let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
    let config: OperatorConfig = serde_yaml::from_reader(config_file).expect("Could not read contents of config file.");
    config
}

// Start the webhook server in a separate task
async fn start_webhook_server(webhook_config: operator_config::WebhookConfig) {
    let webhook_server =tokio::spawn(async move {
        webhook_server::run_webhook_server(webhook_config).await;
    });

    let webhook_boot = webhook_server::wait_for_webhook_ready().await;
    if webhook_boot.is_err() {
        panic!("Webhook server failed to start: {}", webhook_boot.err().unwrap());
    }

    // Blocking here to keep the webhook server running. Uncomment if you want to test webhook server only.
    // webhook_server.await.expect("Webhook task panicked");
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
async fn start_controllers(operator_config: operator_config::OperatorConfig, client: Client) {
    // ⭐ 4. Initialize Kubernetes client and context data
    info!("Initializing Context Data...");
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
