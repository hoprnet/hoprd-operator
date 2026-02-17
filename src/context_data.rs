use k8s_openapi::NamespaceResourceScope;
use serde_json::json;
use tracing::{debug, error};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;

use kube::{
    Api, Client, Resource, ResourceExt, api::{ListParams, Patch, PatchParams}, runtime::events::{Recorder, Reporter}
};

use crate::{ events::ResourceEvent, hoprd::hoprd_resource::Hoprd, identity_hoprd::identity_hoprd_resource::IdentityHoprd, identity_pool::identity_pool_resource::{IdentityPool, IdentityPoolPhaseEnum},
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

    pub async fn sync_identity_pools(&self) -> Result<(), String> {
        let api_identity_pools: Api<IdentityPool> = Api::all(self.client.clone());
        let identity_pools = api_identity_pools.list(&ListParams::default()).await
            .map_err(|e| {
                error!("Could not fetch IdentityPools: {}", e);
                "Could not fetch IdentityPools".to_string()
            })?
            .items;
        let api_identities: Api<IdentityHoprd> = Api::all(self.client.clone());
        let identities = api_identities.list(&ListParams::default()).await
            .map_err(|e| {
                error!("Could not fetch IdentityHoprd: {}", e);
                "Could not fetch IdentityHoprd".to_string()
            })?
            .items;

        let mut state = self.state.write().await;
        state.identity_pool.clear();
        for identity_pool in identity_pools {
            let pool_identities = identities
                .iter()
                .filter(|identity| identity.spec.identity_pool_name == identity_pool.name_any() && identity.metadata.namespace.as_ref().unwrap() == identity_pool.metadata.namespace.as_ref().unwrap())
                .cloned()
                .collect::<Vec<IdentityHoprd>>();
            let locked_pool_identities = pool_identities
                .iter()
                .filter(|identity| identity.status.is_some() && identity.status.as_ref().unwrap().hoprd_node_name.is_some())
                .cloned()
                .collect::<Vec<IdentityHoprd>>();
            if let Some(identity_pool_status) = identity_pool.status.as_ref() {
                if (identity_pool_status.size != pool_identities.len() as i32) || (identity_pool_status.locked != locked_pool_identities.len() as i32) {
                    let mut new_status = identity_pool_status.clone();
                    new_status.size = pool_identities.len() as i32;
                    new_status.locked = locked_pool_identities.len() as i32;
                    new_status.phase = IdentityPoolPhaseEnum::Ready;
                    let identity_pool_name = identity_pool.name_any();
                    let patch = Patch::Merge(json!({"status": new_status}));
                    let api: Api<IdentityPool> = Api::namespaced(self.client.clone(), &identity_pool.metadata.namespace.as_ref().unwrap());
                    match api.patch_status(&identity_pool_name, &PatchParams::default(), &patch).await {
                        Ok(_identity) => {
                            debug!("Fixed IdentityPool status {}/{}: {:?}", identity_pool.metadata.namespace.as_ref().unwrap(), identity_pool_name, new_status);
                        }
                        Err(error) => {
                            error!("Could not update status on {}/{}: {:?}", identity_pool.metadata.namespace.as_ref().unwrap(), identity_pool_name, error);
                        },
                    }
                }
            }
            state.add_identity_pool(identity_pool);
        }
        debug!("Synchronized IdentityPools in ContextData state");
        Ok(())
    }

    pub async fn sync_identities(&self) -> Result<(), String> {
        let api_identities: Api<IdentityHoprd> = Api::all(self.client.clone());
        let locked_identities = api_identities.list(&ListParams::default()).await
            .map_err(|e| {
                error!("Could not fetch IdentityHoprd: {}", e);
                "Could not fetch IdentityHoprd".to_string()
            })?
            .iter()
            .filter(|identity| identity.status.is_some() && identity.status.as_ref().unwrap().hoprd_node_name.is_some())
            .cloned()
            .collect::<Vec<IdentityHoprd>>();
        let api_hoprd = Api::<Hoprd>::all(self.client.clone());
        let all_hoprds: Vec<String> = api_hoprd
            .list(&ListParams::default())
            .await
            .unwrap()
            .items
            .iter()
            .map(|hoprd| format!("{}-{}", hoprd.metadata.namespace.as_ref().unwrap(), hoprd.metadata.name.as_ref().unwrap()))
            .collect();
        // Unlock identities that no longer have a corresponding hoprd
        for identity_hoprd in locked_identities {
            let status = identity_hoprd.status.as_ref().unwrap();
            let hoprd_node_name = status.hoprd_node_name.as_ref().unwrap();
            let identity_full_name = format!("{}-{}", identity_hoprd.to_owned().metadata.namespace.unwrap(), hoprd_node_name);
            if !all_hoprds.contains(&identity_full_name) {
                // Remove hoprd relationship
                identity_hoprd.unlock(Arc::new(self.clone())).await.expect("Could not synchronize identity");
            }
        }
        debug!("Synchronized Identities in ContextData state");
        Ok(())
    }

    pub async fn send_event<T: Resource<Scope = NamespaceResourceScope, DynamicType = ()>, K: ResourceEvent>(&self, resource: &T, event: K, attribute: Option<String>) {
        let recorder = Recorder::new(self.client.clone(), self.state.read().await.reporter.clone());
        recorder.publish(&event.to_event(attribute), &resource.object_ref(&())).await.unwrap();
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
