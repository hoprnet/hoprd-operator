
use futures::StreamExt;
use k8s_openapi::api::{apps::v1::Deployment, networking::v1::Ingress, core::v1::{Service, Secret}};
use kube::{
    api::{Api, ListParams},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{ Recorder, Reporter}
    },
    Resource, Result, Error,
};

use serde::{ Serialize};
use std::{sync::Arc, collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, env};
use tokio::{sync::RwLock, time::Duration};

use crate::{ constants::{self}, hoprd::{Hoprd, HoprdSpec}, model::OperatorConfig, servicemonitor::ServiceMonitor};

/// Action to be taken upon an `Hoprd` resource during reconciliation
enum HoprdAction {
    /// Create the subresources, this includes spawning `n` pods with Hoprd service
    Create,
    /// Modify Hoprd resource
    Modify,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `Hoprd` resource is in desired state and requires no actions to be taken
    NoOp,
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `Hoprd` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `HoprdAction` enum.
///
/// # Arguments
/// - `hoprd`: A reference to `Hoprd` being reconciled to decide next action upon.
fn determine_action(hoprd: &Hoprd) -> HoprdAction {
    return if hoprd.meta().deletion_timestamp.is_some() {
        HoprdAction::Delete
    } else if hoprd
        .meta()
        .finalizers
        .as_ref()
        .map_or(true, |finalizers| finalizers.is_empty())
    {
        HoprdAction::Create
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        let hoprd_spec: HoprdSpec = hoprd.spec.clone();
        hoprd_spec.clone().hash(&mut hasher);
        let hash: String = hasher.finish().to_string();
        let current_checksum = format!("checksum-{}",hash.to_string());
        let previous_checksum: String = hoprd.status.as_ref().map_or("0".to_owned(), |status| status.checksum.to_owned());
        if current_checksum.eq(&previous_checksum) {
            HoprdAction::NoOp
        } else {
            HoprdAction::Modify
        }   
    };
}

async fn reconciler(hoprd: Arc<Hoprd>, context: Arc<ContextData>) -> Result<Action> {
    // Performs action as decided by the `determine_action` function.
    return match determine_action(&hoprd) {
        HoprdAction::Create => hoprd.create(context.clone()).await,
        HoprdAction::Modify => hoprd.modify(context.clone()).await,
        HoprdAction::Delete => hoprd.delete(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        HoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))),
    };
}


/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
pub fn on_error(hoprd: Arc<Hoprd>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!("[ERROR] Reconciliation error:\n{:?}.\n{:?}", error, hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))
}

#[derive(Clone, Serialize)]
pub struct State {
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for State {
    fn default() -> Self {
        Self {
            reporter: Reporter::from("hopr-operator-controller"),
        }
    }
}
impl State {
    pub fn recorder(&self, client: Client, hoprd: &Hoprd) -> Recorder {
        Recorder::new(client, self.reporter.clone(), hoprd.object_ref(&()))
    }
}


#[derive(Clone)]
pub struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    pub client: Client,
    /// In memory state
    pub state: Arc<RwLock<State>>,

    pub config: OperatorConfig
}

/// State wrapper around the controller outputs for the web server
impl ContextData {

    // Create a Controller Context that can update State
    pub async fn new(client: Client) -> Self {
        let operator_environment= env::var(constants::OPERATOR_ENVIRONMENT).unwrap();
        let config_path = if operator_environment.eq("production") {
            let path = "/app/config/config.yaml".to_owned();
            path
        } else {
            let mut path = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
            path.push_str("/sample_config.yaml");
            path
        };
        let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
        let config: OperatorConfig = serde_yaml::from_reader(config_file).expect("Could not read contents of config file.");

        ContextData {
            client,
            state: Arc::new(RwLock::new(State::default())),
            config: config
        }
    }
}

/// Initialize the controller
pub async fn run() {
    let client: Client = Client::try_default().await.expect("Failed to create kube Client");
    let owned_api: Api<Hoprd> = Api::<Hoprd>::all(client.clone());
    let deployment = Api::<Deployment>::all(client.clone());
    let secret = Api::<Secret>::all(client.clone());
    let service = Api::<Service>::all(client.clone());
    let service_monitor = Api::<ServiceMonitor>::all(client.clone());
    let ingress = Api::<Ingress>::all(client.clone());

    let context_data: Arc<ContextData> = Arc::new(ContextData::new(client.clone()).await);
    Controller::new(owned_api, ListParams::default())
        .owns(deployment, ListParams::default())
        .owns(secret, ListParams::default())
        .owns(service, ListParams::default())
        .owns(service_monitor, ListParams::default())
        .owns(ingress, ListParams::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") {
                        // https://github.com/kube-rs/kube/issues/712
                            eprintln!("[ERROR] Reconciliation error: {:?}", err_string)
                    }
                }
            }
        })
        .await;
}
