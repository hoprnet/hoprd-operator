use std::{sync::Arc, env};
use tokio::sync::RwLock;

use kube::{Client, runtime::events::{Reporter, Recorder}, Resource};
use serde::Serialize;

use crate::{operator_config::OperatorConfig, constants, hoprd::Hoprd, cluster::ClusterHoprd, identity_hoprd::IdentityHoprd, identity_pool::IdentityPool};



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
            path.push_str(&format!("/test-data/sample_config-{operator_environment}.yaml"));
            path
        };
        let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
        let config: OperatorConfig = serde_yaml::from_reader(config_file).expect("Could not read contents of config file.");

        ContextData {
            client,
            state: Arc::new(RwLock::new(State::default())),
            config
        }
    }
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

    pub fn generate_identity_hoprd_event(&self, client: Client, identity_hoprd: &IdentityHoprd) -> Recorder {
        Recorder::new(client, self.reporter.clone(), identity_hoprd.object_ref(&()))
    }

    pub fn generate_identity_pool_event(&self, client: Client, identity_pool: &IdentityPool) -> Recorder {
        Recorder::new(client, self.reporter.clone(), identity_pool.object_ref(&()))
    }

    pub fn generate_hoprd_event(&self, client: Client, hoprd: &Hoprd) -> Recorder {
        Recorder::new(client, self.reporter.clone(), hoprd.object_ref(&()))
    }

    pub fn generate_cluster_hoprd_event(&self, client: Client, cluster_hoprd: &ClusterHoprd) -> Recorder {
        Recorder::new(client, self.reporter.clone(), cluster_hoprd.object_ref(&()))
    }

}