use std::{env, sync::Arc, collections::BTreeMap};
use k8s_openapi::NamespaceResourceScope;
use tokio::sync::RwLock;

use kube::{
    runtime::events::{Recorder, Reporter},
    Client, Resource, Api, api::ListParams, ResourceExt};

use crate::{
    constants, hoprd::Hoprd, identity_hoprd::IdentityHoprd,
    identity_pool::IdentityPool, operator_config::OperatorConfig, events::ResourceEvent,
};

#[derive(Clone)]
pub struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    pub client: Client,
    /// In memory state
    pub state: Arc<RwLock<State>>,

    pub config: OperatorConfig,
}

/// State wrapper around the controller outputs for the web server
impl ContextData {
    // Create a Controller Context that can update State
    pub async fn new(client: Client) -> Self {
        let operator_environment = env::var(constants::OPERATOR_ENVIRONMENT).unwrap();
        let config_path = if operator_environment.eq("production") {
            "/app/config/config.yaml".to_owned()
        } else {
            let mut path = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
            path.push_str(&format!("/test-data/sample_config-{operator_environment}.yaml"));
            path
        };
        let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
        let config: OperatorConfig = serde_yaml::from_reader(config_file).expect("Could not read contents of config file.");

        let api = Api::<IdentityPool>::all(client.clone());
        let pools = api.list(&ListParams::default()).await.unwrap().items.clone();

        ContextData {
            client,
            state: Arc::new(RwLock::new(State::new(pools))),
            config,
        }
    }

    pub async fn sync_identities(context_data: Arc<ContextData>) {
        let api_identities = Api::<IdentityHoprd>::all(context_data.client.clone());
        let all_identities = api_identities.list(&ListParams::default()).await.unwrap().items.clone();
        let api_hoprd = Api::<Hoprd>::all(context_data.client.clone());
        let all_hoprds: Vec<String> = api_hoprd.list(&ListParams::default()).await.unwrap().items.clone()
            .iter().map(|hoprd|  format!("{}-{}", hoprd.metadata.namespace.as_ref().unwrap(), hoprd.metadata.name.as_ref().unwrap())).collect();
        for identity_hoprd in all_identities {
            if let Some(status) = identity_hoprd.to_owned().status {
                if let Some(hoprd_node_name) = status.hoprd_node_name {
                    let identity_full_name = format!("{}-{}", identity_hoprd.to_owned().metadata.namespace.unwrap(), hoprd_node_name);
                    if ! all_hoprds.contains(&identity_full_name) {
                        // Remove hoprd relationship
                        identity_hoprd.unlock(context_data.clone()).await.expect("Could not synchronize identity");
                    }
                }
            }
        }
    }

    pub async fn send_event<T: Resource<Scope = NamespaceResourceScope, DynamicType = ()>, K: ResourceEvent>(
        &self,
        resource: &T,
        event: K,
        attribute: Option<String>
    ) {
        let recorder = Recorder::new(self.client.clone(), self.state.read().await.reporter.clone(), resource.object_ref(&()));
        recorder.publish(event.to_event(attribute)).await.unwrap();
    }
}



#[derive(Debug, Clone)]
pub struct State {
    pub reporter: Reporter,
    pub identity_pool: BTreeMap<String, Arc<IdentityPool>>
}

impl State {
    pub fn new(identity_pools: Vec<IdentityPool>) -> State {
        State {
            reporter: Reporter::from("hopr-operator-controller"),
            identity_pool: identity_pools.into_iter().map(|identity_pool| (format!("{}-{}",identity_pool.namespace().unwrap(), identity_pool.name_any()), Arc::new(identity_pool))).collect()
        }
    }

    pub fn add_identity_pool(&mut self, identity_pool: IdentityPool) {
        self.identity_pool.insert(format!("{}-{}",identity_pool.namespace().unwrap(), identity_pool.name_any()), Arc::new(identity_pool));
    }

    pub fn remove_identity_pool(&mut self, namespace: &String, identity_pool_name: &String) {
        self.identity_pool.remove(&format!("{}-{}",namespace, identity_pool_name));
    }

    pub fn get_identity_pool(&self, namespace: &String, identity_pool_name: &String) -> Option<Arc<IdentityPool>> {
        self.identity_pool.get(&format!("{}-{}",namespace, identity_pool_name)).cloned()
    }

    pub fn update_identity_pool(&mut self, identity_pool: IdentityPool) {
        self.remove_identity_pool(identity_pool.metadata.namespace.as_ref().unwrap(), identity_pool.metadata.name.as_ref().unwrap());
        self.add_identity_pool(identity_pool);
    }

}