use crate::events::IdentityPoolEventEnum;
use crate::identity_hoprd::identity_hoprd_resource::{IdentityHoprd, IdentityHoprdPhaseEnum, IdentityHoprdStatus};
use crate::model::Error;
use crate::{constants, context_data::ContextData};
use crate::{
    identity_pool::{identity_pool_cronjob_faucet, identity_pool_service_account, identity_pool_service_monitor},
    resource_generics,
};
use chrono::Utc;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{ListParams, Patch, PatchParams};
use kube::core::ObjectMeta;
use kube::{client::Client, runtime::controller::Action, CustomResource, Result};
use kube::{Api, Resource, ResourceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[kube(group = "hoprnet.org", version = "v1alpha2", kind = "IdentityPool", plural = "identitypools", derive = "PartialEq", namespaced)]
#[kube(status = "IdentityPoolStatus", shortname = "identitypool")]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolSpec {
    pub network: String,
    pub secret_name: String,
    pub funding: Option<IdentityPoolFunding>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolFunding {
    pub schedule: String,
    pub native_amount: String,
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolStatus {
    pub update_timestamp: String,
    pub phase: IdentityPoolPhaseEnum,
    pub size: i32,
    pub locked: i32,
    pub checksum: String,
}

impl Default for IdentityPoolStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            phase: IdentityPoolPhaseEnum::Initialized,
            size: 0,
            locked: 0,
            checksum: "init".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum IdentityPoolPhaseEnum {
    // Status that represent when the IdentityPool is initialized after creation by creating the serviceMonitor
    Initialized,
    /// Status that represent when the IdentityPool initialization validation has failed
    Failed,
    // Status that represent when the IdentityPool is out of synchronization. It requires to create new identities
    OutOfSync,
    // Status that represent when the IdentityPool is ready to be used
    Ready,
    /// Status that represent when the IdentityPool is being deleted
    Deleting,
    // Event that represent when the IdentityPool has locked an identity
    Locked,
    // Event that represent when the IdentityPool has unlocked an identity
    Unlocked,
    // Event that represents when the IdentityPool has created a new identity
    IdentityCreated,
    // Event that represents when the IdentityPool has created a new identity
    IdentityDeleted,
}

impl Display for IdentityPoolPhaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            IdentityPoolPhaseEnum::Initialized => write!(f, "Initialized"),
            IdentityPoolPhaseEnum::Failed => write!(f, "Failed"),
            IdentityPoolPhaseEnum::OutOfSync => write!(f, "OutOfSync"),
            IdentityPoolPhaseEnum::Ready => write!(f, "Ready"),
            IdentityPoolPhaseEnum::Deleting => write!(f, "Deleting"),
            IdentityPoolPhaseEnum::Locked => write!(f, "Locked"),
            IdentityPoolPhaseEnum::Unlocked => write!(f, "Unlocked"),
            IdentityPoolPhaseEnum::IdentityCreated => write!(f, "IdentityCreated"),
            IdentityPoolPhaseEnum::IdentityDeleted => write!(f, "IdentityDeleted"),
        }
    }
}

impl Default for IdentityPool {
    fn default() -> Self {
        Self {
            metadata: ObjectMeta::default(),
            spec: IdentityPoolSpec::default(),
            status: Some(IdentityPoolStatus::default()),
        }
    }
}

impl IdentityPool {
    /// Handle the creation of IdentityPool resource
    pub async fn create(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        if !self.check_wallet(client.clone()).await.unwrap() {
            context_data.send_event(self, IdentityPoolEventEnum::Failed, None).await;
            return Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_LONG_FREQUENCY)));
        }
        info!("Starting to create IdentityPool {identity_pool_name} in namespace {identity_pool_namespace}");
        resource_generics::add_finalizer(client.clone(), self).await;
        identity_pool_service_monitor::create_service_monitor(context_data.clone(), &identity_pool_name, &identity_pool_namespace, &self.spec.secret_name, owner_references.to_owned()).await?;
        identity_pool_service_account::create_rbac(context_data.clone(), &identity_pool_namespace, &identity_pool_name, owner_references.to_owned()).await?;
        if self.spec.funding.is_some() {
            identity_pool_cronjob_faucet::create_cron_job(context_data.clone(), self).await.expect("Could not create Cronjob");
        }
        context_data.send_event(self, IdentityPoolEventEnum::Initialized, None).await;
        self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Initialized).await?;
        context_data.send_event(self, IdentityPoolEventEnum::Ready, None).await;
        self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Ready).await?;
        info!("IdentityPool {identity_pool_name} in namespace {identity_pool_namespace} successfully created");
        context_data.state.write().await.add_identity_pool(self.clone());
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    /// Handle the modification of IdentityPool resource
    pub async fn modify(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();
        if let Some(status) = self.status.as_ref() {
            if status.phase.eq(&IdentityPoolPhaseEnum::Ready) {
                if self.annotations().contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
                    let previous_text: String = self.annotations().get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
                    match serde_json::from_str::<IdentityPool>(&previous_text) {
                        Ok(previous_identity_pool) => {
                            self.apply_modification(context_data, &previous_identity_pool).await?;
                        }
                        Err(err) => {
                            error!("Could not parse the last applied configuration from {identity_pool_name}: {}", err);
                        }
                    }
                } else {
                    error!(
                        "The IdentityPool {} in namespace {} cannot be modified without the last configuration annotation",
                        identity_pool_name, identity_pool_namespace
                    );
                }
            } else if status.phase.eq(&IdentityPoolPhaseEnum::Failed) {
                context_data.send_event(self, IdentityPoolEventEnum::Ready, None).await;
                self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Ready).await?;
                warn!("Detected a change in IdentityPool {identity_pool_name} while being in Failed pahse. Assuming that is manually modified to recover the state. Automatically recovering to a Ready phase");
            } else {
                error!(
                    "The IdentityPool {} in namespace {} cannot be modified in status {}",
                    identity_pool_name, identity_pool_namespace, status.phase
                );
            }
        } else {
            error!("The IdentityPool {} in namespace {} cannot be modified unknown status.", identity_pool_name, identity_pool_namespace);
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    async fn apply_modification(&mut self, context_data: Arc<ContextData>, previous_identity_pool: &IdentityPool) -> Result<(), Error> {
        let client: Client = context_data.client.clone();
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();
        if self.changed_inmutable_fields(&previous_identity_pool.spec) {
            context_data.send_event(self, IdentityPoolEventEnum::Failed, None).await;
            self.update_status(client.clone(), IdentityPoolPhaseEnum::Failed).await?;
        } else {
            let api: Api<CronJob> = Api::namespaced(context_data.client.clone(), &identity_pool_namespace);
            let cron_job_name = format!("auto-funding-{}", identity_pool_name);
            match self.spec.funding.as_ref() {
                Some(_) => {
                    if api.get_opt(&cron_job_name).await?.is_none() {
                        identity_pool_cronjob_faucet::create_cron_job(context_data.clone(), self).await.expect("Could not create Cronjob");
                    } else {
                        identity_pool_cronjob_faucet::modify_cron_job(context_data.client.clone(), self)
                            .await
                            .expect("Could not modify Cronjob");
                    }
                }
                None => {
                    if api.get_opt(&cron_job_name).await?.is_some() {
                        identity_pool_cronjob_faucet::delete_cron_job(context_data.client.clone(), self)
                            .await
                            .expect("Could not delete cronjob");
                    }
                }
            }
            context_data.send_event(self, IdentityPoolEventEnum::Ready, None).await;
            self.update_status(client.clone(), IdentityPoolPhaseEnum::Ready).await?;
            info!("Identity pool {identity_pool_name} in namespace {identity_pool_namespace} has been successfully modified");
        }
        Ok(())
    }

    // Handle the deletion of IdentityPool resource
    pub async fn delete(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let identity_pool_namespace = self.namespace().unwrap();
        let identity_pool_name = self.name_any();
        let status = self.status.as_ref().unwrap();
        if status.locked == 0 && status.size == 0 {
            let client: Client = context_data.client.clone();
            self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Deleting).await?;
            context_data.send_event(self, IdentityPoolEventEnum::Deleting, None).await;
            info!("Starting to delete identity {identity_pool_name} from namespace {identity_pool_namespace}");
            identity_pool_service_monitor::delete_service_monitor(client.clone(), &identity_pool_name, &identity_pool_namespace).await?;
            identity_pool_service_account::delete_rbac(client.clone(), &identity_pool_namespace, &identity_pool_name).await?;
            if self.spec.funding.is_some() {
                identity_pool_cronjob_faucet::delete_cron_job(client.clone(), self).await?;
            }
            resource_generics::delete_finalizer(client.clone(), self).await;
            context_data.state.write().await.remove_identity_pool(&identity_pool_namespace, &identity_pool_name);
            info!("Identity {identity_pool_name} in namespace {identity_pool_namespace} has been successfully deleted");
            Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
        } else {
            warn!("Cannot delete an identity pool with identities");
            Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_LONG_FREQUENCY)))
        }
    }

    pub async fn sync(&mut self, _context_data: Arc<ContextData>) -> Result<Action, Error> {
        warn!("IdentityPool {} in namespace {} requires more identities", self.name_any(), self.namespace().unwrap());
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_LONG_FREQUENCY)))
    }

    fn changed_inmutable_fields(&self, previous_identity: &IdentityPoolSpec) -> bool {
        if !self.spec.network.eq(&previous_identity.network) {
            error!("Configuration is invalid, 'network' field cannot be changed on {}.", self.name_any());
            true
        } else if !self.spec.secret_name.eq(&previous_identity.secret_name) {
            error!("Configuration is invalid, 'secret_name' field cannot be changed on {}.", self.name_any());
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

    /// Updates the status of IdentityPool
    pub async fn update_status(&mut self, client: Client, phase: IdentityPoolPhaseEnum) -> Result<(), Error> {
        let identity_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
        let mut identity_pool_status = self.status.as_ref().unwrap_or(&IdentityPoolStatus::default()).to_owned();

        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        if phase.eq(&IdentityPoolPhaseEnum::Deleting) {
            Ok(())
        } else {
            identity_pool_status.update_timestamp = Utc::now().to_rfc3339();
            identity_pool_status.checksum = self.get_checksum();
            identity_pool_status.phase = phase;
            if phase.eq(&IdentityPoolPhaseEnum::IdentityCreated) {
                identity_pool_status.size += 1;
                if identity_pool_status.size >= identity_pool_status.locked {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
            } else if phase.eq(&IdentityPoolPhaseEnum::IdentityDeleted) {
                identity_pool_status.size -= 1;
                if identity_pool_status.size >= identity_pool_status.locked {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
            };

            if phase.eq(&IdentityPoolPhaseEnum::Locked) {
                identity_pool_status.locked += 1;
                if identity_pool_status.size >= identity_pool_status.locked {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
            } else if phase.eq(&IdentityPoolPhaseEnum::Unlocked) {
                identity_pool_status.locked -= 1;
                if identity_pool_status.size >= identity_pool_status.locked {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
            };
            let patch = Patch::Merge(json!({
                    "status": identity_pool_status
            }));
            match api.patch(&identity_hoprd_name, &PatchParams::default(), &patch).await {
                Ok(_identity) => {
                    self.status = Some(identity_pool_status.clone());
                    Ok(debug!("IdentityPool current status: {:?}", identity_pool_status))
                }
                Err(error) => Ok(error!("Could not update status on {identity_hoprd_name}: {:?}", error)),
            }
        }
    }

    pub async fn get_pool_identities(&self, client: Client) -> Vec<IdentityHoprd> {
        let api: Api<IdentityHoprd> = Api::namespaced(client, &self.namespace().unwrap().to_owned());
        let namespace_identities = api.list(&ListParams::default()).await.expect("Could not list namespace identities");
        let pool_identities: Vec<IdentityHoprd> = namespace_identities
            .iter()
            .filter(|&identity| {
                if let Some(owner_references) = identity.metadata.owner_references.as_ref() {
                    owner_references.first().unwrap().name.eq(&self.name_any())
                } else {
                    false
                }
            })
            .cloned()
            .collect();
        pool_identities
    }

    /// Gets the first identity in ready status
    pub async fn get_ready_identity(&mut self, client: Client, identity_name: Option<String>) -> Result<Option<IdentityHoprd>, Error> {
        let pool_identities: Vec<IdentityHoprd> = self.get_pool_identities(client).await;

        let identity: Option<IdentityHoprd> = match identity_name.clone() {
            Some(provided_identity_name) => {
                let identity_hoprd = pool_identities.iter().find(|&identity| identity.metadata.name.clone().unwrap().eq(&provided_identity_name)).cloned();
                if identity_hoprd.is_none() {
                    warn!("The identity provided {} does not exist", provided_identity_name);
                    None
                } else if identity_hoprd
                    .as_ref()
                    .unwrap()
                    .status
                    .as_ref()
                    .unwrap_or(&IdentityHoprdStatus::default())
                    .phase
                    .eq(&IdentityHoprdPhaseEnum::Ready)
                {
                    identity_hoprd
                } else {
                    if let Some(status) = identity_hoprd.as_ref().unwrap().status.as_ref() {
                        warn!(
                            "The identity {} is in phase {} and might be used by {}",
                            provided_identity_name,
                            status.phase,
                            status.hoprd_node_name.as_ref().unwrap_or(&"unknown".to_owned())
                        );
                    } else {
                        warn!("The identity {} has not status reported yet", provided_identity_name);
                    }
                    None
                }
            }
            None => {
                // No identity has been provided
                let ready_pool_identity: Option<IdentityHoprd> = pool_identities
                    .iter()
                    .find(|&identity| identity.status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::Ready))
                    .cloned();
                if ready_pool_identity.is_none() {
                    warn!("There are no identities ready to be used in this pool {}", self.name_any());
                }
                ready_pool_identity
            }
        };
        Ok(identity)
    }

    async fn check_wallet(&self, client: Client) -> Result<bool, Error> {
        let api: Api<Secret> = Api::namespaced(client, &self.namespace().unwrap());
        if let Some(wallet) = api.get_opt(&self.spec.secret_name).await? {
            if let Some(wallet_data) = wallet.data {
                if wallet_data.contains_key(constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY)
                    && wallet_data.contains_key(constants::IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY)
                    && wallet_data.contains_key(constants::IDENTITY_POOL_API_TOKEN_REF_KEY)
                {
                    Ok(true)
                } else {
                    error!("IdentityPool {} has a secret {} with some missing data", self.name_any(), self.spec.secret_name);
                    Ok(false)
                }
            } else {
                error!("IdentityPool {} has a secret {} with empty data", self.name_any(), self.spec.secret_name);
                Ok(false)
            }
        } else {
            error!(
                "IdentityPool {} cannot find secret {} in namespace {}",
                self.name_any(),
                self.spec.secret_name,
                self.namespace().unwrap()
            );
            Ok(false)
        }
    }
}
