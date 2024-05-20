use crate::cluster::cluster_hoprd::{ClusterHoprd, ClusterHoprdPhaseEnum};
use crate::constants::SupportedReleaseEnum;
use crate::events::{ClusterHoprdEventEnum, HoprdEventEnum, IdentityHoprdEventEnum, IdentityPoolEventEnum};
use crate::resource_generics;
use crate::identity_hoprd::identity_hoprd_resource::{IdentityHoprd, IdentityHoprdPhaseEnum};
use crate::identity_pool::identity_pool_resource::{IdentityPool, IdentityPoolPhaseEnum};
use crate::model::Error;
use crate::{
    hoprd::{hoprd_deployment_spec::HoprdDeploymentSpec, hoprd_deployment, hoprd_service, hoprd_ingress, hoprd_service::{ServiceTypeEnum, HoprdServiceSpec}},
    constants, context_data::ContextData, 
};
use chrono::Utc;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::WatchParams;
use kube::core::object::HasSpec;
use kube::core::{WatchEvent, ObjectMeta};
use kube::Resource;
use kube::{
    api::{Api, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::controller::Action,
    CustomResource, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};


/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha2",
    kind = "Hoprd",
    plural = "hoprds",
    derive = "PartialEq",
    namespaced
)]
#[kube(status = "HoprdStatus", shortname = "hoprd")]
#[serde(rename_all = "camelCase")]
pub struct HoprdSpec {
    pub identity_pool_name: String,
    pub identity_name: Option<String>,
    pub version: String,
    pub config: String,
    pub enabled: Option<bool>,
    pub delete_database: Option<bool>,
    pub supported_release: SupportedReleaseEnum,
    pub service: Option<HoprdServiceSpec>,
    pub deployment: Option<HoprdDeploymentSpec>,
}


/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HoprdStatus {
    pub update_timestamp: String,
    pub phase: HoprdPhaseEnum,
    pub checksum: String,
    pub identity_name: Option<String>,
}

impl Default for HoprdStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            phase: HoprdPhaseEnum::Initializing,
            checksum: "init".to_owned(),
            identity_name: None
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum HoprdPhaseEnum {
    // The node is not yet created
    Initializing,
    /// The node is running
    Running,
    /// The node is stopped
    Stopped,
    /// The node is in failed status
    Failed,
    /// Event that triggers when node is modified
    Modified,
    /// Event that triggers when node is being deleted
    Deleting,

}

impl Display for HoprdPhaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            HoprdPhaseEnum::Initializing => write!(f, "Initializing"),
            HoprdPhaseEnum::Running => write!(f, "Running"),
            HoprdPhaseEnum::Stopped => write!(f, "Stopped"),
            HoprdPhaseEnum::Modified => write!(f, "Modified"),
            HoprdPhaseEnum::Failed => write!(f, "Failed"),
            HoprdPhaseEnum::Deleting => write!(f, "Deleting"),
        }
    }
}

impl Default for Hoprd {
    fn default() -> Self {
        Self {
            metadata: ObjectMeta::default(),
            spec: HoprdSpec::default(),
            status: Some(HoprdStatus::default()),
        }
    }
}

impl Hoprd {
    // Creates all the related resources
    pub async fn create(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let hoprd_name: String = self.name_any();
        context_data.send_event(self, HoprdEventEnum::Initializing, None).await;
        self.update_status(client.clone(), HoprdPhaseEnum::Initializing, None).await?;
        info!("Starting to create Hoprd node {hoprd_name} in namespace {hoprd_namespace}");
        let owner_reference: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        if let Some(identity) = self.lock_identity(context_data.clone()).await?
        {
            resource_generics::add_finalizer(client.clone(), self).await;
            let service_type = self.spec.service.as_ref().unwrap_or(&HoprdServiceSpec::default()).r#type.clone();
            let p2p_port = hoprd_ingress::create_ingress(context_data.clone(), &service_type, &hoprd_name,&hoprd_namespace,&context_data.config.ingress,owner_reference.to_owned()).await?;
            let hoprd_host = hoprd_service::create_service(context_data.clone(), &hoprd_name, &hoprd_namespace, &self.spec.identity_pool_name, service_type, p2p_port, owner_reference.to_owned()).await.unwrap();
            hoprd_deployment::create_deployment(context_data.clone(),self,&identity, &hoprd_host, p2p_port).await?;
            self.wait_deployment(client.clone()).await?;
            
            self.set_running_status(context_data.clone(), Some(identity.name_any())).await?;
            let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
            if self.spec_mut().delete_database.is_none() {
                let patch = Patch::Merge(json!({ "spec": { "deleteDatabase": false } }));
                match api.patch(&hoprd_name, &PatchParams::default(), &patch).await
                {
                    Ok(_) => {},
                    Err(error) => error!("Could not update the deleteDatabase field {hoprd_name}: {:?}", error)
                };
            }
            info!("Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully created");        
        } else {
            context_data.send_event(self, HoprdEventEnum::Failed, None).await;
            return Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_LONG_FREQUENCY)))
        };
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    // Creates all the related resources
    pub async fn modify(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let hoprd_name: String = self.name_any();
        if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&HoprdPhaseEnum::Failed) {
            // Assumes that the next modification of the resource is to recover to a good state
            context_data.send_event(self,HoprdEventEnum::Modified, None).await;
            if self.spec.enabled.unwrap_or(true) {
                self.update_status(client.clone(), HoprdPhaseEnum::Running, None).await?;
            } else {
                self.update_status(client.clone(), HoprdPhaseEnum::Stopped, None).await?;
            }           
            warn!("Detected a change in Hoprd {hoprd_name} while was in Failed phase. Automatically recovering to a Running/Stopped phase");

        } else  if self.annotations().contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
                let previous_hoprd_text: String = self.annotations().get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
                match serde_json::from_str::<Hoprd>(&previous_hoprd_text) {
                    Ok(previous_hoprd) => {
                        if self.changed_inmutable_fields(&previous_hoprd.spec) {
                            context_data.send_event(self, HoprdEventEnum::Failed,None).await;
                            self.update_status(client.clone(), HoprdPhaseEnum::Failed, None).await?;
                        } else if let Some(identity) = self.get_identity(client.clone()).await? {
                                self.apply_modification(context_data.clone(), &identity).await?;
                            } else {
                                error!("Hoprd node {hoprd_name} does not have a linked identity and is inconsistent");
                                context_data.send_event(self, HoprdEventEnum::Failed, None).await;
                                self.update_status(client.clone(), HoprdPhaseEnum::Failed, None).await?;
                            }
                    },
                    Err(_err) => {
                        error!("Could not parse the last applied configuration of Hoprd {hoprd_name}");
                        context_data.send_event(self, HoprdEventEnum::Failed, None).await;
                        self.update_status(client.clone(), HoprdPhaseEnum::Failed, None).await?;
                    }
                }
                self.update_last_configuration(context_data.client.clone()).await?;
            } else {
                error!("Could not modify Hoprd {hoprd_name} because cannot recover last configuration");
                context_data.send_event(self, HoprdEventEnum::Failed, None).await;
                self.update_status(client.clone(), HoprdPhaseEnum::Failed, None).await?;
            }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    async fn set_running_status(&self, context_data: Arc<ContextData>, identity_name: Option<String>) -> Result<(), Error> {
        if self.spec.enabled.unwrap_or(true) {
            context_data.send_event(self, HoprdEventEnum::Running, None).await;
            self.update_status(context_data.client.clone(), HoprdPhaseEnum::Running, identity_name).await?;
        } else {
            context_data.send_event(self, HoprdEventEnum::Stopped, None).await;
            self.update_status(context_data.client.clone(), HoprdPhaseEnum::Stopped, identity_name).await?;
        }
        Ok(())
    }

    async fn apply_modification(&mut self, context_data: Arc<ContextData>, identity: &IdentityHoprd) -> Result<(), Error> {
        let hoprd_namespace: String = self.namespace().unwrap();
        let hoprd_name: String = self.name_any();
        hoprd_deployment::modify_deployment(context_data.clone(), &hoprd_name.to_owned(),&hoprd_namespace.to_owned(),&self.spec.to_owned(),identity).await?;
        if self.spec_mut().delete_database.unwrap_or(false) {
            info!("Deleting database for Hoprd node {hoprd_name} in namespace {hoprd_namespace}");
            hoprd_deployment::delete_database(context_data.clone(), &hoprd_name.to_owned(),&hoprd_namespace.to_owned()).await?;
            let client: Client = context_data.client.clone();
            let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
            let patch = Patch::Merge(json!({ "spec": { "deleteDatabase": false } }));
            match api.patch(&hoprd_name, &PatchParams::default(), &patch).await
            {
                Ok(_) => {},
                Err(error) => error!("Could not update the deleteDatabaseField on {hoprd_name}: {:?}", error)
            };
        }
        match self.wait_deployment(context_data.client.clone()).await {
            Ok(()) =>  {
                info!("Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully modified");
                context_data.send_event(self, HoprdEventEnum::Modified, None).await;
                self.set_running_status(context_data.clone(), None).await
            },
            Err(_) => Ok(warn!("Error waiting for deployment of {hoprd_name} to become ready")),
        }
    }

    // Deletes all the related resources
    pub async fn delete(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let hoprd_name = self.name_any();
        let hoprd_namespace = self.namespace().unwrap();
        let client: Client = context_data.client.clone();
        let current_pahse = self.status.to_owned().unwrap_or_default().phase;
        if current_pahse.eq(&HoprdPhaseEnum::Deleting) {
            info!("HoprdNode {} is already being deleted", hoprd_name);
            return Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_LONG_FREQUENCY)))
        }
        self.update_status(client.clone(), HoprdPhaseEnum::Deleting, None).await?;
        context_data.send_event(self, HoprdEventEnum::Deleting, None).await;
        info!("Starting to delete Hoprd node {hoprd_name} from namespace {hoprd_namespace}");
        // Deletes any subresources related to this `Hoprd` resources. If and only if all subresources
        // are deleted, the finalizer is removed and Kubernetes is free to remove the `Hoprd` resource.
        let service_type = self.spec.service.as_ref().unwrap_or(&HoprdServiceSpec::default()).r#type.clone();
        hoprd_ingress::delete_ingress(context_data.clone(), &hoprd_name, &hoprd_namespace, &service_type).await?;
        hoprd_service::delete_service(client.clone(), &hoprd_name, &hoprd_namespace, &service_type).await?;
        hoprd_deployment::delete_depoyment(client.clone(), &hoprd_name, &hoprd_namespace).await.unwrap();
        if let Some(identity) = self.get_identity(client.clone()).await? {
            identity.unlock(context_data.clone()).await?;
        } else {
            warn!("HoprdNode {hoprd_name} in namespace {hoprd_namespace} does not have a locked identity")
        }
        // Once all the resources are successfully removed, remove the finalizer to make it possible
        // for Kubernetes to delete the `Hoprd` resource.
        context_data.send_event(self, HoprdEventEnum::Deleted, None).await;
        if let Some(cluster) = self.notify_deletion_to_cluster(context_data.clone()).await? {
            resource_generics::delete_finalizer(client.clone(), self).await;
            context_data.send_event(&cluster, ClusterHoprdEventEnum::NodeDeleted, None).await;
            cluster.update_status(context_data.clone(), ClusterHoprdPhaseEnum::NodeDeleted).await.unwrap();
        } else {
            resource_generics::delete_finalizer(client.clone(), self).await;
        }
        info!("Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully deleted");
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }

    // Locks a given identity from a Hoprd node
    async fn lock_identity(&self, context_data: Arc<ContextData>) -> Result<Option<IdentityHoprd>, Error> {
        let hoprd_name = Some(self.name_any());
        let identity_pool_name = self.spec.identity_pool_name.to_owned();
        let identity_name = self.spec.identity_name.to_owned();
        let identity_created: Option<IdentityHoprd>;
        {
            let mut context_state = context_data.state.write().await;
            let identity_pool_option = context_state.get_identity_pool(&self.namespace().unwrap(), &identity_pool_name);
            if identity_pool_option.is_some() {
                let mut identity_pool_arc = identity_pool_option.unwrap();
                let identity_pool:  &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_arc);
                if let Some(identity) = identity_pool.get_ready_identity(context_data.client.clone(), identity_name).await? {
                    identity.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::InUse, hoprd_name.clone()).await?;
                    
                    identity_pool.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Locked).await?;
                    context_state.update_identity_pool(identity_pool.to_owned());
                    identity_created = Some(identity.clone());
                } else {
                    warn!("Could not get lock the identity for node {}", self.name_any());
                    identity_created = None
                }
            } else {
                warn!("Identity pool {} not exists yet in namespace {}", identity_pool_name, &self.namespace().unwrap());
                identity_created = None
            }
        }
        if identity_created.as_ref().is_some() {
            context_data.send_event(&identity_created.as_ref().unwrap().get_identity_pool(context_data.client.clone()).await.unwrap(), IdentityPoolEventEnum::Locked, hoprd_name.clone()).await;
            context_data.send_event(identity_created.as_ref().unwrap(), IdentityHoprdEventEnum::InUse, hoprd_name.clone()).await;
            Ok(identity_created)

        } else {
            Ok(None)
        }
    }

    fn changed_inmutable_fields(&self, previous_hoprd: &HoprdSpec) -> bool {
        if !self.spec.identity_pool_name.eq(&previous_hoprd.identity_pool_name) {
            error!("Hoprd configuration is invalid, 'identity_pool_name' field cannot be changed on {}.", self.name_any());
            true
        } else if previous_hoprd.identity_name.is_some() && self.spec.identity_name.is_some() && !self.spec.identity_name.clone().unwrap().eq(&previous_hoprd.identity_name.clone().unwrap()) {
            error!("Hoprd configuration is invalid, 'identity_name' field cannot be changed on {}.",self.name_any());
            true
        } else if previous_hoprd.identity_name.is_some() && self.spec.identity_name.is_none() {
            error!("Hoprd configuration is invalid, 'identity_name' cannot be removed on {}.",self.name_any());
            true
        } else {
            false
        }
    }


    async fn update_status(&self, client: Client, phase: HoprdPhaseEnum, identity_name: Option<String>) -> Result<(), Error> {
        let hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();

        let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        let mut status = self.status.as_ref().unwrap_or(&HoprdStatus::default()).to_owned();
        status.update_timestamp = Utc::now().to_rfc3339();
        status.phase = phase;
        status.checksum = self.get_checksum();
        if identity_name.is_some() {
            status.identity_name = identity_name;
        }
        let patch = Patch::Merge(json!({ "status": status }));
        match api.patch(&hoprd_name, &PatchParams::default(), &patch).await
        {
            Ok(_) => Ok(()),
            Err(error) => Ok(error!("Could not update status on node {hoprd_name}: {:?}",error))
        }
    }

    async fn update_last_configuration(&self, client: Client) -> Result<(), Error> {
        let api: Api<Hoprd> = Api::namespaced(client, &self.namespace().unwrap());
        let mut cloned_hoprd = self.clone();
        cloned_hoprd.status = None;
        cloned_hoprd.metadata.managed_fields = None;
        cloned_hoprd.metadata.creation_timestamp = None;
        cloned_hoprd.metadata.finalizers = None;
        cloned_hoprd.metadata.annotations = None;
        cloned_hoprd.metadata.generation = None;
        cloned_hoprd.metadata.resource_version = None;
        cloned_hoprd.metadata.uid = None;
        let hoprd_last_configuration = serde_json::to_string(&cloned_hoprd).unwrap();
        let mut annotations = BTreeMap::new();
        annotations.insert(constants::ANNOTATION_LAST_CONFIGURATION.to_string(), hoprd_last_configuration);
        let patch = Patch::Merge(json!({
            "metadata": { 
                "annotations": annotations 
            }
        }));
        match api.patch(&self.name_any(), &PatchParams::default(), &patch).await
        {
            Ok(_cluster_hopr) => Ok(()),
            Err(error) => Ok(error!("Could not update last configuration annotation on Hoprd {}: {:?}", self.name_any(), error))
        }
    }

    async fn notify_deletion_to_cluster(&self, context_data: Arc<ContextData>) -> Result<Option<ClusterHoprd>, Error> {
        if let Some(owner_reference) = self.owner_references().to_owned().first() {
            let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
            let api: Api<ClusterHoprd> = Api::namespaced(context_data.client.clone(), &hoprd_namespace.to_owned());
            if let Some(cluster) = api.get_opt(&owner_reference.name).await? {
                let current_phase = cluster.to_owned().status.unwrap().phase;
                if current_phase.ne(&ClusterHoprdPhaseEnum::Deleting) {
                    debug!("Current Phase {} of {}", current_phase, self.name_any().to_owned());
                    info!("Notifying ClusterHoprd {} that hoprd node {} is being deleted", &owner_reference.name, self.name_any().to_owned());
                    return Ok(Some(cluster));
                }
            } else {
                debug!("ClusterHoprd {} not found", &owner_reference.name);
            }
        }
        Ok(None)
    }

    pub fn get_checksum(&self) -> String {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        hasher.finish().to_string()
    }

    async fn get_identity(&self, client: Client) -> Result<Option<IdentityHoprd>, Error> {
        match &self.status {
            Some(status) => {
                let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &self.namespace().unwrap());
                match status.to_owned().identity_name {
                    Some(identity_name) => Ok(api.get_opt(identity_name.as_str()).await.unwrap()),
                    None => {
                        warn!("HoprdNode status for {} is incomplete as it does not contain a identity_name", &self.name_any());
                        Ok(None)
                    }
                }
            },
            None => {
            warn!("HoprdNode {} has empty status", &self.name_any());
            Ok(None)
            }
        }
    }
    // Wait for the Hoprd deployment to be created
    pub async fn wait_deployment(&self, client: Client) -> Result<(),Error> { 
        if self.spec.enabled.unwrap_or(true) {
            let lp = WatchParams::default().fields(&format!("metadata.name={}", self.name_any())).timeout(constants::OPERATOR_NODE_SYNC_TIMEOUT);
            let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &self.namespace().unwrap());
            let mut stream = deployment_api.watch(&lp, "0").await?.boxed();
            while let Some(deployment) = stream.try_next().await? {
                match deployment {
                    WatchEvent::Added(deployment) => {
                        if deployment.status.as_ref().unwrap().ready_replicas.unwrap_or(0).eq(&1) {
                            info!("Hoprd node {} deployment in namespace {:?} is ready", self.name_any(), &self.namespace().unwrap());
                            return Ok(())
                        }
                    }
                    WatchEvent::Modified(deployment) => {
                        if deployment.status.as_ref().unwrap().ready_replicas.unwrap_or(0).eq(&1) {
                            info!("Hoprd node {} deployment in namespace {:?} is ready", self.name_any(), &self.namespace().unwrap());
                            return Ok(())
                        }
                    }
                    WatchEvent::Deleted(_) => {
                        return Err(Error::ClusterHoprdSynchError("Deleted operation not expected".to_owned()))
                    }
                    WatchEvent::Bookmark(_) => {
                         warn!("Hoprd node {} deployment in namespace {:?} is bookmarked", self.name_any(), &self.namespace().unwrap());
                        return Ok(())
                    }
                    WatchEvent::Error(_) => {
                        return Err(Error::ClusterHoprdSynchError("Error operation not expected".to_owned()))
                    }
                }
            }
            Err(Error::ClusterHoprdSynchError("Timeout waiting for Hoprd node to be created".to_owned()))
        } else {
            Ok(())
        }
    }
}
