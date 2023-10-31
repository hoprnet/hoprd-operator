use std::collections::hash_map::DefaultHasher;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::Resource;
use tracing::{debug, info, error, warn};
use crate::identity_hoprd_persistence;
use crate::identity_pool::{IdentityPool, IdentityPoolStatusEnum};
use crate::{constants, context_data::ContextData};
use crate::model::Error;
use chrono::Utc;
use kube::runtime::events::Recorder;
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::json;
use kube::{
    api::{Api, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{controller::Action, events::{Event, EventType}
    },
    CustomResource, Result
};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha",
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
    pub module_address: String
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityHoprdStatus {
    pub update_timestamp: String,
    pub status: IdentityHoprdStatusEnum,
    pub checksum: String,
    pub hoprd_node_name: Option<String>
}


#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum IdentityHoprdStatusEnum {
    // The IdentityHoprd is initializing after first creationg
    Initialized,
    /// The IdentityHoprd is failed
    Failed,
    // The IdentityHoprd is synching
    Synching,
    // The IdentityHoprd is ready to be used
    Ready,
    // The IdentityHoprd is being used
    InUse,
    /// The IdentityHoprd is being deleted
    Deleting
}

impl Display for IdentityHoprdStatusEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            IdentityHoprdStatusEnum::Initialized => write!(f, "Initialized"),
            IdentityHoprdStatusEnum::Failed => write!(f, "Failed"),
            IdentityHoprdStatusEnum::Synching => write!(f, "Synching"),
            IdentityHoprdStatusEnum::Ready => write!(f, "Ready"),
            IdentityHoprdStatusEnum::InUse => write!(f, "InUse"),
            IdentityHoprdStatusEnum::Deleting => write!(f, "Deleting")
        }
    }
}

impl IdentityHoprd {

    /// Handle the creation of IdentityHoprd resource
    pub async fn create(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context.client.clone();
        let identity_namespace: String = self.namespace().unwrap();
        let identity_name: String= self.name_any();

        info!("[IdentityHoprd] Starting to create identity {identity_name} in namespace {identity_namespace}");
        self.add_finalizer(client.clone(), &identity_name, &identity_namespace).await.unwrap();
        self.add_owner_reference(client.clone()).await?;
        identity_hoprd_persistence::create_pvc(context.clone(), &self).await?;
        self.create_event(context.clone(), IdentityHoprdStatusEnum::Initialized, None).await?;
        self.update_status(context.clone(), IdentityHoprdStatusEnum::Initialized, None).await?;
        // TODO: Validate data
        // - Is registered in network
        // - Is funded (safe and node)
        // - SafeAddress is correct
        // - ModuleAddress is correct
        self.create_event(context.clone(), IdentityHoprdStatusEnum::Ready, None).await?;
        self.update_status(context.clone(), IdentityHoprdStatusEnum::Ready, None).await?;
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    /// Handle the modification of IdentityHoprd resource
    pub async fn modify(&self) -> Result<Action, Error> {
        error!("[IdentityPool] The resource cannot be modified");
        Err(Error::OperationNotSupported(format!("[IdentityPool] The resource cannot be modified").to_owned()))
    }

    // Handle the deletion of IdentityHoprd resource
    pub async fn delete(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let identity_name = self.name_any();
        let identity_namespace = self.namespace().unwrap();
        let client: Client = context.client.clone();
        if ! self.status.as_ref().unwrap().status.eq(&IdentityHoprdStatusEnum::InUse) {
            self.create_event(context.clone(),  IdentityHoprdStatusEnum::Deleting, None).await?;
            self.update_status(context.clone(), IdentityHoprdStatusEnum::Deleting, None).await?;
            info!("[IdentityHoprd] Starting to delete identity {identity_name} from namespace {identity_namespace}");
            // TODO Drain funds
            self.delete_finalizer(client.clone(), &identity_name, &identity_namespace).await?;
            info!("[IdentityHoprd] Identity {identity_name} in namespace {identity_namespace} has been successfully deleted");
            Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
        } else {
            Err(Error::IdentityStatusError(format!("[IdentityHoprd] Cannot delete a identity in state {}", self.status.as_ref().unwrap().status)))
        }
    }

    /// Adds a finalizer in IdentityHoprd to prevent deletion of the resource by Kubernetes API and allow the controller to safely manage its deletion 
    async fn add_finalizer(&self, client: Client, identity_name: &str, identity_namespace: &str) -> Result<(), Error> {
        let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &identity_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": [constants::OPERATOR_FINALIZER]
            }
        }));
        match api.patch(&identity_name, &PatchParams::default(), &patch).await {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("[IdentityHoprd] Could not add finalizer on {identity_name}: {:?}", error);
                return Err(Error::HoprdStatusError(format!("[IdentityHoprd] Could not add finalizer on {identity_name}.").to_owned()));
            }
        }
    }

    /// Deletes the finalizer of IdentityHoprd resource, so the resource can be freely deleted by Kubernetes API
    async fn delete_finalizer(&self, client: Client, identity_name: &str, identity_namespace: &str) -> Result<(), Error> {
        let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &identity_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": null
            }
        }));
        if let Some(_) = api.get_opt(&identity_name).await? {
            match api.patch(&identity_name, &PatchParams::default(), &patch).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    Ok(error!("[IdentityHoprd] Could not delete finalizer on {identity_name}: {:?}", error))
                }
            }
        } else {
            Ok(debug!("[IdentityHoprd] Identity {identity_name} already deleted"))
        }
    }

    /// Creates an event for IdentityHoprd given the new IdentityHoprdStatusEnum
    pub async fn create_event(&self, context: Arc<ContextData>, status: IdentityHoprdStatusEnum, hoprd_name: Option<String>) -> Result<(), Error> {
        let client: Client = context.client.clone();   
        let ev: Event = match status {
            IdentityHoprdStatusEnum::Initialized => Event {
                        type_: EventType::Normal,
                        reason: "Initialized".to_string(),
                        note: Some("Initialized node identity".to_owned()),
                        action: "Starting the process of creating a new identity".to_string(),
                        secondary: None,
                    },
            IdentityHoprdStatusEnum::Failed => Event {
                        type_: EventType::Warning,
                        reason: "Failed".to_string(),
                        note: Some("Failed to bootstrap identity".to_owned()),
                        action: "Identity bootstrapping failed".to_string(),
                        secondary: None,
                    },
            IdentityHoprdStatusEnum::Synching => Event {
                        type_: EventType::Normal,
                        reason: "Synching".to_string(),
                        note: Some("Starting to sync an identity".to_owned()),
                        action: "Synching failed identity".to_string(),
                        secondary: None,
                    },
            IdentityHoprdStatusEnum::Ready => Event {
                        type_: EventType::Normal,
                        reason: "Ready".to_string(),
                        note: Some("Identity ready to be used".to_owned()),
                        action: "Identity is ready to be used by a Hoprd node".to_string(),
                        secondary: None,
                    },
            IdentityHoprdStatusEnum::InUse => Event {
                        type_: EventType::Normal,
                        reason: "InUse".to_string(),
                        note: Some(format!("Identity being used by Hoprd node {}", hoprd_name.unwrap_or("unknown".to_owned()))),
                        action: "Identity is being used".to_string(),
                        secondary: None,
                    },
            IdentityHoprdStatusEnum::Deleting => Event {
                        type_: EventType::Normal,
                        reason: "Deleting".to_string(),
                        note: Some("Identity is being deleted".to_owned()),
                        action: "Identity deletion started".to_string(),
                        secondary: None,
                    }

        };
        let recorder: Recorder = context.state.read().await.generate_identity_hoprd_event(client.clone(), self);
        Ok(recorder.publish(ev).await?)
    }

    /// Updates the status of IdentityHoprd
    pub async fn update_status(&self, context: Arc<ContextData>, status: IdentityHoprdStatusEnum, hoprd_name: Option<String>) -> Result<(), Error> {
    let client: Client = context.client.clone();
    let identity_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();    
    let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();

    let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
    if status.eq(&IdentityHoprdStatusEnum::Deleting) {
        Ok(())
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        let checksum: String = hasher.finish().to_string();
        let status = IdentityHoprdStatus {
                update_timestamp: Utc::now().to_rfc3339(),
                status,
                checksum,
                hoprd_node_name: hoprd_name
        };
        let patch = Patch::Merge(json!({ "status": status }));

        match api.patch(&identity_hoprd_name, &PatchParams::default(), &patch).await {
            Ok(_identity) => Ok(()),
            Err(error) => {
                Ok(error!("[IdentityHoprd] Could not update status on {identity_hoprd_name}: {:?}", error))
            }
        }
    }

}

    pub async fn get_identity_pool(&self, client: Client) -> Result<IdentityPool, Error> {
        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        return Ok(api.get(&self.spec.identity_pool_name).await.unwrap())
    }

    // // Unlocks a given identity from a Hoprd node
    pub async fn unlock(&self, context: Arc<ContextData>) -> Result<(), Error> {
        if self.status.as_ref().unwrap().status.eq(&IdentityHoprdStatusEnum::InUse) {
            self.create_event(context.clone(), IdentityHoprdStatusEnum::Ready, None).await?;
            self.update_status(context.clone(), IdentityHoprdStatusEnum::Ready, None).await?;
            let api: Api<IdentityPool> = Api::namespaced(context.client.clone(), &self.namespace().unwrap());
            let identity_pool = api.get(self.spec.identity_pool_name.as_str()).await.unwrap();
            identity_pool.update_status(context.clone(), IdentityPoolStatusEnum::Unlocked).await?;
            Ok(())
        } else {
            Ok(warn!("The identity cannot be unlock because it is in status {:?}", &self.status))
        }
    }

    pub async fn add_owner_reference(&self, client: Client) -> Result<(),Error> {
        let identity_pool = self.get_identity_pool(client.clone()).await.unwrap();
        let identity_name = self.name_any();
        let identity_pool_owner_reference: Option<Vec<OwnerReference>> = Some(vec![identity_pool.controller_owner_ref(&()).unwrap()]);
        let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &identity_pool.namespace().unwrap());
        let patch = Patch::Merge(json!({
                    "metadata": {
                        "ownerReferences": identity_pool_owner_reference 
                    }
        }));
        let _updated = match api.patch(&identity_name, &PatchParams::default(), &patch).await {
            Ok(secret) => Ok(secret),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                Err(Error::HoprdStatusError(format!("Could not update secret owned references for '{identity_name}'.").to_owned()))
            }
        };
        Ok(())

    }

}
