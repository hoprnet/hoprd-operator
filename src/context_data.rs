use k8s_openapi::NamespaceResourceScope;
use tracing::{debug, info};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

use kube::{
    Api, Client, Resource, ResourceExt, api::{DynamicObject, GroupVersionKind, ListParams}, runtime::events::{Recorder, Reporter}
};

use crate::{ events::ResourceEvent, hoprd::hoprd_resource::Hoprd, identity_hoprd::identity_hoprd_resource::IdentityHoprd, identity_pool::identity_pool_resource::IdentityPool,
    operator_config::OperatorConfig,
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
    pub async fn new(client: Client, config: OperatorConfig) -> Self {
        let api = Api::<IdentityPool>::all(client.clone());
        let pools: Vec<IdentityPool> = match api.list(&ListParams::default()).await {
            Ok(list) => list.items.clone(),
            Err(e) => {
                debug!("Could not fetch IdentityPools: {}", e);
                vec![]
            }
        };

        ContextData {
            client,
            state: Arc::new(RwLock::new(State::new(pools))),
            config,
        }
    }

    pub async fn sync_identities(context_data: Arc<ContextData>) {
        // BEGIN DEBUGGING BUG
        info!("IdentityHoprd apiVersion from type at runtime: {}",<IdentityHoprd as Resource>::api_version(&()));
        let gvk = GroupVersionKind::gvk("hoprnet.org", "v1alpha3", "IdentityHoprd");
        let ar = kube::core::ApiResource::from_gvk(&gvk);
        let api: Api<DynamicObject> = Api::namespaced_with(context_data.client.clone(), "core-team", &ar);
        let object = api.get("core-node-1").await.unwrap();
        debug!("Got IdentityHoprd {:#?}", object);
        // END DEBUGGING BUG

        let api_identities: Api<IdentityHoprd> = Api::all(context_data.client.clone());
        let identities = api_identities.list(&ListParams::default()).await.unwrap().items.clone();
        let api_hoprd = Api::<Hoprd>::all(context_data.client.clone());
        let all_hoprds: Vec<String> = api_hoprd
            .list(&ListParams::default())
            .await
            .unwrap()
            .items
            .clone()
            .iter()
            .map(|hoprd| format!("{}-{}", hoprd.metadata.namespace.as_ref().unwrap(), hoprd.metadata.name.as_ref().unwrap()))
            .collect();
        // Unlock identities that no longer have a corresponding hoprd
        for identity_hoprd in identities {
            if let Some(status) = identity_hoprd.to_owned().status {
                if let Some(hoprd_node_name) = status.hoprd_node_name {
                    let identity_full_name = format!("{}-{}", identity_hoprd.to_owned().metadata.namespace.unwrap(), hoprd_node_name);
                    if !all_hoprds.contains(&identity_full_name) {
                        // Remove hoprd relationship
                        identity_hoprd.unlock(context_data.clone()).await.expect("Could not synchronize identity");
                    }
                }
            }
        }
    }

    pub async fn send_event<T: Resource<Scope = NamespaceResourceScope, DynamicType = ()>, K: ResourceEvent>(&self, resource: &T, event: K, attribute: Option<String>) {
        let recorder = Recorder::new(self.client.clone(), self.state.read().await.reporter.clone(), resource.object_ref(&()));
        recorder.publish(event.to_event(attribute)).await.unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub reporter: Reporter,
    pub identity_pool: BTreeMap<String, Arc<IdentityPool>>,
}

impl State {
    pub fn new(identity_pools: Vec<IdentityPool>) -> State {
        State {
            reporter: Reporter::from("hopr-operator-controller"),
            identity_pool: identity_pools
                .into_iter()
                .map(|identity_pool| (format!("{}-{}", identity_pool.namespace().unwrap(), identity_pool.name_any()), Arc::new(identity_pool)))
                .collect(),
        }
    }

    pub fn add_identity_pool(&mut self, identity_pool: IdentityPool) {
        self.identity_pool
            .insert(format!("{}-{}", identity_pool.namespace().unwrap(), identity_pool.name_any()), Arc::new(identity_pool));
    }

    pub fn remove_identity_pool(&mut self, namespace: &String, identity_pool_name: &String) {
        self.identity_pool.remove(&format!("{}-{}", namespace, identity_pool_name));
    }

    pub fn get_identity_pool(&self, namespace: &String, identity_pool_name: &String) -> Option<Arc<IdentityPool>> {
        self.identity_pool.get(&format!("{}-{}", namespace, identity_pool_name)).cloned()
    }

    pub fn update_identity_pool(&mut self, identity_pool: IdentityPool) {
        self.remove_identity_pool(identity_pool.metadata.namespace.as_ref().unwrap(), identity_pool.metadata.name.as_ref().unwrap());
        self.add_identity_pool(identity_pool);
    }
}
