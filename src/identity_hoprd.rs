use crate::events::{ IdentityHoprdEventEnum, IdentityPoolEventEnum};
use crate::{identity_hoprd_persistence, resource_generics};
use crate::identity_pool::{IdentityPool, IdentityPoolPhaseEnum, IdentityPoolStatus};
use crate::model::Error;
use crate::{constants, context_data::ContextData};
use chrono::Utc;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::core::ObjectMeta;
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
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha2",
    kind = "IdentityHoprd",
    plural = "identityhoprds",
    derive = "PartialEq",
    namespaced
)]
#[kube(status = "IdentityHoprdStatus", shortname = "identityhoprd")]
#[serde(rename_all = "camelCase")]
pub struct IdentityHoprdSpec {
    pub identity_pool_name: String,
    pub identity_file: String,
    pub peer_id: String,
    pub native_address: String,
    pub safe_address: String,
    pub module_address: String,
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityHoprdStatus {
    pub update_timestamp: String,
    pub phase: IdentityHoprdPhaseEnum,
    pub checksum: String,
    pub hoprd_node_name: Option<String>,
}

impl Default for IdentityHoprdStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            phase: IdentityHoprdPhaseEnum::Initialized,
            checksum: "init".to_owned(),
            hoprd_node_name: None
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum IdentityHoprdPhaseEnum {
    // The IdentityHoprd is initializing after first creationg
    Initialized,
    /// The IdentityHoprd is failed
    Failed,
    // The IdentityHoprd is ready to be used
    Ready,
    // The IdentityHoprd is being used
    InUse,
    /// The IdentityHoprd is being deleted
    Deleting,
}

impl Display for IdentityHoprdPhaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            IdentityHoprdPhaseEnum::Initialized => write!(f, "Initialized"),
            IdentityHoprdPhaseEnum::Failed => write!(f, "Failed"),
            IdentityHoprdPhaseEnum::Ready => write!(f, "Ready"),
            IdentityHoprdPhaseEnum::InUse => write!(f, "InUse"),
            IdentityHoprdPhaseEnum::Deleting => write!(f, "Deleting"),
        }
    }
}

impl Default for IdentityHoprd {
    fn default() -> Self {
        Self {
            metadata: ObjectMeta::default(),
            spec: IdentityHoprdSpec::default(),
            status: Some(IdentityHoprdStatus::default()),
        }
    }
}


impl IdentityHoprd {
    /// Handle the creation of IdentityHoprd resource
    pub async fn create(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let identity_namespace: String = self.namespace().unwrap();
        let identity_name: String = self.name_any();
        let identity_pool_name: String = self.spec.identity_pool_name.to_owned();
        if ! self.check_identity_pool(client.clone()).await.unwrap() {
            context_data.send_event(self, IdentityHoprdEventEnum::Failed, None).await;
            return Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY_ERROR)))
        }
        info!("Starting to create identity {identity_name} in namespace {identity_namespace}");
        resource_generics::add_finalizer(client.clone(), self).await;
        self.add_owner_reference(client.clone()).await?;
        identity_hoprd_persistence::create_pvc(context_data.clone(), self).await?;
        context_data.send_event(self, IdentityHoprdEventEnum::Initialized, None).await;
        self.update_phase(client.clone(), IdentityHoprdPhaseEnum::Initialized, None).await?;
        // TODO: Validate data
        // - Is registered in network
        // - Is funded (safe and node)
        // - SafeAddress is correct
        // - ModuleAddress is correct

        // Update pool to decrease identities
        let mut updated = false;
        {
            let mut context_state = context_data.state.write().await;
            let identity_pool_option = context_state.get_identity_pool(&self.namespace().unwrap(), &identity_pool_name);
            if identity_pool_option.is_some() {
                let mut identity_pool_arc = identity_pool_option.unwrap();
                let identity_pool:  &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_arc);
                identity_pool.update_phase(context_data.client.clone(), IdentityPoolPhaseEnum::IdentityCreated).await?;
                context_state.update_identity_pool(identity_pool.to_owned());
                updated = true;
            }
        }
        context_data.send_event(&self.get_identity_pool(client.clone()).await.unwrap(), IdentityPoolEventEnum::IdentityCreated, Some(identity_name.to_owned())).await;
        if updated {
            // These instructions need to be done out of the context_data lock
            context_data.send_event(self, IdentityHoprdEventEnum::Ready, None).await;
            self.update_phase(client.clone(), IdentityHoprdPhaseEnum::Ready, None).await?;
            info!("IdentityHoprd {identity_name} in namespace {identity_namespace} successfully created");
        } else {
            error!("Identity pool {} not exists in namespace {}", identity_pool_name, &self.namespace().unwrap());
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    /// Handle the modification of IdentityHoprd resource
    pub async fn modify(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let identity_name: String = self.name_any();
        if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::Ready) {
            if self.annotations().contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
                let previous_config_text: String = self.annotations().get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
                match serde_json::from_str::<IdentityHoprd>(&previous_config_text) {
                    Ok(previous_cluster_hoprd) => {
                        if self.changed_inmutable_fields(&previous_cluster_hoprd.spec) {
                            context_data.send_event(self,IdentityHoprdEventEnum::Failed,None).await;
                            self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Failed, None).await?;
                        } else {
                            info!("IdentityHoprd {} in namespace {} has been successfully modified", self.name_any(), self.namespace().unwrap());
                        }
                        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
                    }
                    Err(_err) => {
                        context_data.send_event(self,IdentityHoprdEventEnum::Failed,None).await;
                        self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Failed, None).await?;
                        Err(Error::HoprdConfigError(format!("Could not parse the last applied configuration of IdentityHoprd {identity_name}")))
                    }
                }
            } else {
                context_data.send_event(self,IdentityHoprdEventEnum::Failed,None).await;
                self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Failed, None).await?;
                Err(Error::HoprdConfigError(format!("Could not modify IdentityHoprd {identity_name} because cannot recover last configuration")))
            }
        } else if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::Failed) {
            // Assumes that the next modification of the resource is to recover to a good state
            context_data.send_event(self,IdentityHoprdEventEnum::Ready,None).await;
            self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Ready, None).await?;
            warn!("Detected a change in IdentityHoprd {identity_name}. Automatically recovering to a Ready phase");
            Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
        } else {
            error!("The IdentityHoprd {} in namespace {} cannot be modified", self.name_any(), self.namespace().unwrap());
            Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
        }
    }

    // Handle the deletion of IdentityHoprd resource
    pub async fn delete(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let identity_name = self.name_any();
        let identity_namespace = self.namespace().unwrap();
        let client: Client = context_data.client.clone();
        if let Some(status) = self.status.as_ref() {
            if !status.phase.eq(&IdentityHoprdPhaseEnum::InUse) {
                context_data.send_event(self, IdentityHoprdEventEnum::Deleting, None).await;
                self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Deleting, None).await?;
                info!("Starting to delete identity {identity_name} from namespace {identity_namespace}");

                {
                    let mut context_state = context_data.state.write().await;
                    let identity_pool_option = context_state.get_identity_pool(&self.namespace().unwrap(), &self.spec.identity_pool_name);
                    if identity_pool_option.is_some() {
                        let mut identity_pool_arc = identity_pool_option.unwrap();
                        let identity_pool:  &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_arc);
                        identity_pool.update_phase(context_data.client.clone(), IdentityPoolPhaseEnum::IdentityDeleted).await?;
                        context_state.update_identity_pool(identity_pool.to_owned());
                    } else {
                        warn!("Identity pool {} not exists in namespace {}", &self.spec.identity_pool_name, &self.namespace().unwrap());
                    }
                }
                context_data.send_event(&self.get_identity_pool(client.clone()).await.unwrap(), IdentityPoolEventEnum::IdentityDeleted, Some(identity_name.to_owned())).await;
                // TODO Drain funds
                resource_generics::delete_finalizer(client.clone(), self).await;
                info!("Identity {identity_name} in namespace {identity_namespace} has been successfully deleted");
                Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
            } else {
                error!("Cannot delete an identity in state {}", status.phase);
                Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
            }
        } else {
            error!("IdentityHoprd {} was not correctly initialized", &identity_name);
            resource_generics::delete_finalizer(client.clone(), self).await;
            Ok(Action::await_change())
        }

    }

    /// Check the fileds that cannot be modifed
    fn changed_inmutable_fields(&self, spec: &IdentityHoprdSpec) -> bool {
        if !self.spec.identity_pool_name.eq(&spec.identity_pool_name) {
            error!("IdentityHoprd configuration is invalid, identity_pool_name field cannot be changed on {}.", self.name_any());
            true
        } else {
            false
        }
    }


    pub fn get_checksum(&self) -> String {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        hasher.finish().to_string()
    }

    /// Updates the status of IdentityHoprd
    pub async fn update_phase(&self, client: Client, phase: IdentityHoprdPhaseEnum, hoprd_name: Option<String>) -> Result<(), Error> {
        let identity_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();

        let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        if phase.eq(&IdentityHoprdPhaseEnum::Deleting) {
            Ok(())
        } else {
            let status = IdentityHoprdStatus {
                update_timestamp: Utc::now().to_rfc3339(),
                phase,
                checksum: self.get_checksum(),
                hoprd_node_name: hoprd_name,
            };
            let patch = Patch::Merge(json!({ "status": status }));

            match api.patch(&identity_hoprd_name, &PatchParams::default(), &patch).await
            {
                Ok(_identity) => Ok(()),
                Err(error) => Ok(error!("Could not update status on {identity_hoprd_name}: {:?}",error))
            }
        }
    }

    pub async fn get_identity_pool(&self, client: Client) -> Result<IdentityPool, Error> {
        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        Ok(api.get(&self.spec.identity_pool_name).await.unwrap())
    }

    // Unlocks a given identity from a Hoprd node
    pub async fn unlock(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        if self.status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::InUse) {
            context_data.send_event(self, IdentityHoprdEventEnum::Ready, None).await;
            self.update_phase(context_data.client.clone(), IdentityHoprdPhaseEnum::Ready, None).await?;

            // Update pool to decrease locks
            {
                let mut context_state = context_data.state.write().await;
                let mut identity_pool_arc = context_state.get_identity_pool(&self.namespace().unwrap(), &self.spec.identity_pool_name).unwrap();
                let identity_pool:  &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_arc);
                identity_pool.update_phase(context_data.client.clone(), IdentityPoolPhaseEnum::Unlocked).await?;
                context_state.update_identity_pool(identity_pool.to_owned());
            }
            context_data.send_event(&self.get_identity_pool(context_data.client.clone()).await.unwrap(), IdentityPoolEventEnum::Unlocked, Some(self.name_any())).await;
            Ok(())
        } else {
            Ok(warn!("The identity cannot be unlock because it is in status {:?}", &self.status))
        }
    }

    pub async fn add_owner_reference(&self, client: Client) -> Result<(), Error> {
        let identity_pool = self.get_identity_pool(client.clone()).await.unwrap();
        let identity_name = self.name_any();
        let identity_pool_owner_reference: Option<Vec<OwnerReference>> = Some(vec![identity_pool.controller_owner_ref(&()).unwrap()]);
        let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &identity_pool.namespace().unwrap());
        let patch = Patch::Merge(json!({
                    "metadata": {
                        "ownerReferences": identity_pool_owner_reference
                    }
        }));
        let _updated = match api.patch(&identity_name, &PatchParams::default(), &patch).await
        {
            Ok(secret) => Ok(secret),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                Err(Error::HoprdStatusError(format!("Could not update secret owned references for '{identity_name}'.").to_owned()))
            }
        };
        Ok(())
    }

    async fn check_identity_pool(&self, client: Client) -> Result<bool,Error> {
        let api: Api<IdentityPool> = Api::namespaced(client, &self.namespace().unwrap());
        if let Some(identity_pool) = api.get_opt(&self.spec.identity_pool_name).await? {
            if identity_pool.status.is_some() && identity_pool.status.as_ref().unwrap().phase.eq(&IdentityPoolPhaseEnum::Ready) {
                Ok(true)
            } else {
                error!("IdentityHoprd {} has an IdentityPool {} in namespace {} with status {:?}", self.name_any(), self.spec.identity_pool_name, self.namespace().unwrap(), identity_pool.status.as_ref().unwrap_or(&IdentityPoolStatus::default()));
            Ok(false)
            }
        } else {
            error!("IdentityHoprd {} cannot find IdentityPool {} in namespace {}", self.name_any(), self.spec.identity_pool_name, self.namespace().unwrap());
            Ok(false)
        }
    }

}
