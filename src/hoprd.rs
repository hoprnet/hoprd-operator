use crate::cluster::ClusterHoprdStatusEnum;
use crate::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_hoprd::IdentityHoprd;
use crate::identity_pool::IdentityPool;
use crate::model::Error;
use crate::{
    cluster::ClusterHoprd, constants, context_data::ContextData, hoprd_deployment, hoprd_ingress,
    hoprd_service, utils,
};
use chrono::Utc;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::runtime::events::{Event, EventType, Recorder};
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
use tracing::{debug, error, info, warn};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha",
    kind = "Hoprd",
    plural = "hoprds",
    derive = "PartialEq",
    namespaced
)]
#[kube(status = "HoprdStatus", shortname = "hoprd")]
#[serde(rename_all = "camelCase")]
pub struct HoprdSpec {
    pub identity_pool_name: String,
    pub identity_name: String,
    pub version: String,
    pub config: String,
    pub enabled: Option<bool>,
    pub deployment: Option<HoprdDeploymentSpec>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdConfig {
    pub announce: Option<bool>,
    pub provider: Option<String>,
    pub default_strategy: Option<String>,
    pub max_auto_channels: Option<i32>,
    pub auto_redeem_tickets: Option<bool>,
    pub check_unrealized_balance: Option<bool>,
    pub allow_private_node_connections: Option<bool>,
    pub test_announce_local_address: Option<bool>,
    pub heartbeat_interval: Option<i32>,
    pub heartbeat_threshold: Option<i32>,
    pub heartbeat_variance: Option<i32>,
    pub on_chain_confirmations: Option<i32>,
    pub network_quality_threshold: Option<u32>,
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HoprdStatus {
    pub update_timestamp: String,
    pub status: HoprdStatusEnum,
    pub checksum: String,
    pub identity_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum HoprdStatusEnum {
    // The node is not yet created
    Initializing,
    /// The node is running
    Running,
    /// The node is stopped
    Stopped,
    /// The node is reconfigured
    Synching,
    /// The node is not sync
    OutOfSync,
    /// The node is being deleted
    Deleting,
    /// The node is deleted
    Deleted,
}

impl Display for HoprdStatusEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            HoprdStatusEnum::Initializing => write!(f, "Initializing"),
            HoprdStatusEnum::Running => write!(f, "Running"),
            HoprdStatusEnum::Stopped => write!(f, "Stopped"),
            HoprdStatusEnum::Synching => write!(f, "Synching"),
            HoprdStatusEnum::OutOfSync => write!(f, "OutOfSync"),
            HoprdStatusEnum::Deleting => write!(f, "Deleting"),
            HoprdStatusEnum::Deleted => write!(f, "Deleted"),
        }
    }
}

impl Hoprd {
    // Creates all the related resources
    pub async fn create(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let hoprd_name: String = self.name_any();
        self.create_event(context.clone(), HoprdStatusEnum::Initializing)
            .await?;
        self.update_status(context.clone(), HoprdStatusEnum::Initializing, None)
            .await?;
        info!("Starting to create Hoprd node {hoprd_name} in namespace {hoprd_namespace}");
        let owner_reference: Option<Vec<OwnerReference>> =
            Some(vec![self.controller_owner_ref(&()).unwrap()]);
        self.add_finalizer(client.clone(), &hoprd_name, &hoprd_namespace.to_owned())
            .await
            .unwrap();
        // Invoke creation of a Kubernetes resources
        let identity_pool = self
            .get_identity_pool(client.clone())
            .await
            .unwrap()
            .unwrap();
        if let Some(identity) = identity_pool
            .lock_identity(context.clone(), self.spec.identity_name.as_ref())
            .await?
        {
            let p2p_port = hoprd_ingress::open_port(
                client.clone(),
                &hoprd_namespace,
                &hoprd_name,
                &context.config.ingress,
            )
            .await
            .unwrap();
            hoprd_deployment::create_deployment(
                context.clone(),
                &self,
                &identity,
                p2p_port,
                context.config.ingress.to_owned(),
            )
            .await?;
            hoprd_service::create_service(
                client.clone(),
                &hoprd_name,
                &hoprd_namespace,
                &identity_pool.name_any().to_owned(),
                p2p_port,
                owner_reference.to_owned(),
            )
            .await?;
            hoprd_ingress::create_ingress(
                context.clone(),
                &hoprd_name,
                &hoprd_namespace,
                &context.config.ingress,
                owner_reference.to_owned(),
            )
            .await?;
            info!("Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully created");
            self.update_status(
                context.clone(),
                HoprdStatusEnum::Running,
                Some(identity.name_any()),
            )
            .await?;
            Ok(Action::requeue(Duration::from_secs(
                constants::RECONCILE_FREQUENCY,
            )))
        } else {
            // TODO: Inform to update status and create event
            error!("[Hoprd] Error locking the identity");
            Ok(Action::requeue(Duration::from_secs(
                constants::RECONCILE_FREQUENCY,
            )))
        }
    }

    // Creates all the related resources
    pub async fn modify(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let hoprd_name: String = self.name_any();
        info!(
            "Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully modified"
        );
        self.create_event(context.clone(), HoprdStatusEnum::Synching).await?;
        self.update_status(
            context.clone(),
            HoprdStatusEnum::Synching,
            self.status.as_ref().unwrap().to_owned().identity_name,
        )
        .await?;
        let annotations = utils::get_resource_kinds(
            client.clone(),
            utils::ResourceType::Hoprd,
            utils::ResourceKind::Annotations,
            &hoprd_name,
            &hoprd_namespace,
        )
        .await;
        if annotations.contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
            let previous_hoprd_text: String = annotations
                .get_key_value(constants::ANNOTATION_LAST_CONFIGURATION)
                .unwrap()
                .1
                .parse()
                .unwrap();
            match serde_json::from_str::<Hoprd>(&previous_hoprd_text) {
                Ok(previous_hoprd) => {
                    self.check_inmutable_fields(&previous_hoprd.spec).unwrap();
                    let identity = self.get_identity(client.clone()).await?;
                    if identity.is_some() {
                        hoprd_deployment::modify_deployment(
                            context.clone(),
                            &hoprd_name.to_owned(),
                            &hoprd_namespace.to_owned(),
                            &self.spec.to_owned(),
                            &identity.unwrap(),
                        )
                        .await?;
                    } else {
                        warn!("Hoprd node {hoprd_name} does not have a linked secret and is inconsistent");
                    }
                }
                Err(_err) => {
                    error!("[Hoprd] Could not parse the last applied configuration of Hoprd node {hoprd_name}.");
                }
            }
        }
        Ok(Action::requeue(Duration::from_secs(
            constants::RECONCILE_FREQUENCY,
        )))
    }

    // Deletes all the related resources
    pub async fn delete(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let hoprd_name = self.name_any();
        let hoprd_namespace = self.namespace().unwrap();
        let client: Client = context.client.clone();
        self.create_event(context.clone(), HoprdStatusEnum::Deleting)
            .await?;
        info!("Starting to delete Hoprd node {hoprd_name} from namespace {hoprd_namespace}");
        // Deletes any subresources related to this `Hoprd` resources. If and only if all subresources
        // are deleted, the finalizer is removed and Kubernetes is free to remove the `Hoprd` resource.
        hoprd_ingress::close_port(
            client.clone(),
            &hoprd_namespace,
            &hoprd_name,
            &context.config.ingress,
        )
        .await
        .unwrap();
        hoprd_ingress::delete_ingress(client.clone(), &hoprd_name, &hoprd_namespace).await?;
        hoprd_service::delete_service(client.clone(), &hoprd_name, &hoprd_namespace).await?;
        hoprd_deployment::delete_depoyment(client.clone(), &hoprd_name, &hoprd_namespace)
            .await
            .unwrap();
        if let Some(identity) = self.get_identity(client.clone()).await.unwrap() {
            identity.unlock(context.clone()).await?;
        }
        // Once all the resources are successfully removed, remove the finalizer to make it possible
        // for Kubernetes to delete the `Hoprd` resource.
        self.create_event(context.clone(), HoprdStatusEnum::Deleted)
            .await?;
        self.delete_finalizer(client.clone(), &hoprd_name, &hoprd_namespace)
            .await?;
        info!(
            "Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully deleted"
        );
        self.notify_cluster(context.clone()).await.unwrap();
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }

    /// Adds a finalizer record into an `Hoprd` kind of resource. If the finalizer already exists,
    /// this action has no effect.
    ///
    /// # Arguments:
    /// - `client` - Kubernetes client to modify the `Hoprd` resource with.
    /// - `hoprd_name` - Name of the `Hoprd` resource to modify. Existence is not verified
    /// - `hoprd_namespace` - Namespace where the `Hoprd` resource with given `name` resides.
    ///
    async fn add_finalizer(
        &self,
        client: Client,
        hoprd_name: &str,
        hoprd_namespace: &str,
    ) -> Result<Hoprd, Error> {
        let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": [constants::OPERATOR_FINALIZER]
            }
        }));
        match api
            .patch(&hoprd_name, &PatchParams::default(), &patch)
            .await
        {
            Ok(hopr) => Ok(hopr),
            Err(error) => {
                error!(
                    "[Hoprd] Could not add finalizer on Hoprd node {hoprd_name}: {:?}",
                    error
                );
                return Err(Error::HoprdStatusError(
                    format!("Could not add finalizer on {hoprd_name}.").to_owned(),
                ));
            }
        }
    }

    /// Removes all finalizers from an `Hoprd` resource. If there are no finalizers already, this
    /// action has no effect.
    ///
    /// # Arguments:
    /// - `client` - Kubernetes client to modify the `Hoprd` resource with.
    /// - `hoprd_name` - Name of the `Hoprd` resource to modify. Existence is not verified
    /// - `hoprd_namespace` - Namespace where the `Hoprd` resource with given `name` resides.
    ///
    async fn delete_finalizer(
        &self,
        client: Client,
        hoprd_name: &str,
        hoprd_namespace: &str,
    ) -> Result<(), Error> {
        let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": null
            }
        }));
        if let Some(_) = api.get_opt(&hoprd_name).await? {
            match api
                .patch(&hoprd_name, &PatchParams::default(), &patch)
                .await
            {
                Ok(_hopr) => Ok(()),
                Err(error) => Ok(error!(
                    "[Hoprd] Could not delete finalizer on Hoprd node {hoprd_name}: {:?}",
                    error
                )),
            }
        } else {
            Ok(debug!("Hoprd node {hoprd_name} already deleted"))
        }
    }

    fn check_inmutable_fields(&self, previous_hoprd: &HoprdSpec) -> Result<(), Error> {
        if !self
            .spec
            .identity_pool_name
            .eq(&previous_hoprd.identity_pool_name)
        {
            return Err(Error::HoprdConfigError(format!("Hoprd configuration is invalid, 'identity_pool_name' field cannot be changed on {}.", self.name_any())));
        }
        if !self.spec.identity_name.eq(&previous_hoprd.identity_name) {
            return Err(Error::HoprdConfigError(format!(
                "Hoprd configuration is invalid, 'identity_name' field cannot be changed on {}.",
                self.name_any()
            )));
        }
        Ok(())
    }

    /// Creates an event for ClusterHoprd given the new HoprdStatusEnum
    pub async fn create_event(
        &self,
        context: Arc<ContextData>,
        status: HoprdStatusEnum,
    ) -> Result<(), Error> {
        let client: Client = context.client.clone();

        let ev: Event = match status {
            HoprdStatusEnum::Initializing => Event {
                type_: EventType::Normal,
                reason: "Initializing".to_string(),
                note: Some("Initializing Hoprd node".to_owned()),
                action: "Starting the process of creating a new node".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::Running => Event {
                type_: EventType::Normal,
                reason: "Running".to_string(),
                note: Some("Hoprd node is running".to_owned()),
                action: "Node has started".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::Stopped => Event {
                type_: EventType::Normal,
                reason: "Stopped".to_string(),
                note: Some("Hoprd node is stopped".to_owned()),
                action: "Node has stopped".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::Synching => Event {
                type_: EventType::Normal,
                reason: "Synching".to_string(),
                note: Some("Hoprd node configuration change detected".to_owned()),
                action: "Node reconfigured".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::Deleting => Event {
                type_: EventType::Normal,
                reason: "Deleting".to_string(),
                note: Some("Hoprd node is being deleted".to_owned()),
                action: "Node deletion started".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::Deleted => Event {
                type_: EventType::Normal,
                reason: "Deleted".to_string(),
                note: Some("Hoprd node is deleted".to_owned()),
                action: "Node deletion finished".to_string(),
                secondary: None,
            },
            HoprdStatusEnum::OutOfSync => Event {
                type_: EventType::Warning,
                reason: "Out of sync".to_string(),
                note: Some("Hoprd node is not sync".to_owned()),
                action: "Node sync failed".to_string(),
                secondary: None,
            },
        };
        let recorder: Recorder = context
            .state
            .read()
            .await
            .generate_hoprd_event(client.clone(), self);
        Ok(recorder.publish(ev).await?)
    }

    pub async fn update_status(
        &self,
        context: Arc<ContextData>,
        status: HoprdStatusEnum,
        identity_name: Option<String>,
    ) -> Result<(), Error> {
        let client: Client = context.client.clone();
        let hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();

        let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        if status.eq(&HoprdStatusEnum::Deleting) || status.eq(&HoprdStatusEnum::Deleted) {
            api.get(&hoprd_name).await?;
            Ok(())
        } else {
            let mut hasher: DefaultHasher = DefaultHasher::new();
            self.spec.clone().hash(&mut hasher);
            let checksum: String = hasher.finish().to_string();
            let status = HoprdStatus {
                update_timestamp: Utc::now().to_rfc3339(),
                status,
                checksum,
                identity_name: identity_name,
            };
            let patch = Patch::Merge(json!({ "status": status }));
            match api
                .patch(&hoprd_name, &PatchParams::default(), &patch)
                .await
            {
                Ok(_) => Ok(()),
                Err(error) => Ok(error!(
                    "[Hoprd] Could not update status on node {hoprd_name}: {:?}",
                    error
                )),
            }
        }
    }

    pub async fn notify_cluster(&self, context: Arc<ContextData>) -> Result<(), Error> {
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
        let api: Api<ClusterHoprd> =
            Api::namespaced(context.client.clone(), &hoprd_namespace.to_owned());
        if let Some(owner_reference) = self.owner_references().to_owned().first() {
            if let Some(cluster) = api.get_opt(&owner_reference.name).await? {
                if cluster.to_owned().status.unwrap().status != ClusterHoprdStatusEnum::Deleting {
                    cluster
                        .create_event(
                            context.clone(),
                            ClusterHoprdStatusEnum::OutOfSync,
                            Some(self.name_any().to_owned()),
                        )
                        .await
                        .unwrap();
                    cluster
                        .update_status(context.clone(), ClusterHoprdStatusEnum::OutOfSync)
                        .await
                        .unwrap();
                    info!(
                        "Notifying ClusterHoprd {} that hoprd node {} is being deleted",
                        &owner_reference.name,
                        self.name_any().to_owned()
                    )
                }
            } else {
                println!("[WARN] ClusterHoprd {} not found", &owner_reference.name);
            }
        };

        Ok(())
    }

    pub async fn get_identity_pool(&self, client: Client) -> Result<Option<IdentityPool>> {
        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        return Ok(api
            .get_opt(self.spec.identity_pool_name.as_str())
            .await
            .unwrap());
    }

    pub async fn get_identity(&self, client: Client) -> Result<Option<IdentityHoprd>, Error> {
        match &self.status {
            Some(status) => {
                let api: Api<IdentityHoprd> =
                    Api::namespaced(client.clone(), &self.namespace().unwrap());
                return match status.to_owned().identity_name {
                    Some(identity_name) => Ok(api.get_opt(identity_name.as_str()).await.unwrap()),
                    None => Ok(None),
                };
            }
            None => Ok(None),
        }
    }
}
