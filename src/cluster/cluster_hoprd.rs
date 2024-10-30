use crate::constants::SupportedReleaseEnum;
use crate::events::ClusterHoprdEventEnum;
use crate::hoprd::{
    hoprd_deployment_spec::HoprdDeploymentSpec,
    hoprd_resource::{Hoprd, HoprdSpec},
    hoprd_service::HoprdServiceSpec,
};
use crate::model::Error;
use crate::{constants, context_data::ContextData};
use crate::{resource_generics, utils};
use chrono::Utc;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{DeleteParams, ListParams, PostParams};
use kube::core::ObjectMeta;
use kube::runtime::conditions;
use kube::runtime::wait::await_condition;
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
#[kube(group = "hoprnet.org", version = "v1alpha2", kind = "ClusterHoprd", plural = "clusterhoprds", derive = "PartialEq", namespaced)]
#[kube(status = "ClusterHoprdStatus", shortname = "clusterhoprd")]
#[serde(rename_all = "camelCase")]
pub struct ClusterHoprdSpec {
    pub identity_pool_name: String,
    pub replicas: i32,
    pub config: String,
    pub version: String,
    pub enabled: Option<bool>,
    pub supported_release: SupportedReleaseEnum,
    pub force_identity_name: Option<bool>,
    pub service: Option<HoprdServiceSpec>,
    pub deployment: Option<HoprdDeploymentSpec>,
    pub ports_allocation: Option<i32>,
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterHoprdStatus {
    pub update_timestamp: String,
    pub phase: ClusterHoprdPhaseEnum,
    pub checksum: String,
    pub current_nodes: i32,
}

impl Default for ClusterHoprdStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            phase: ClusterHoprdPhaseEnum::Initialized,
            checksum: "init".to_owned(),
            current_nodes: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum ClusterHoprdPhaseEnum {
    // The HoprdCluster is initialized
    Initialized,
    /// The HoprdCluster is not synchronized with its nodes
    NotScaled,
    /// The HoprdCluster is performing a scaling action
    Scaling,
    /// The HoprdCluster is in failed state with its nodes
    Failed,
    // The HoprdCluster is synchronized with its nodes
    Ready,
    /// The HoprdCluster is being deleted
    Deleting,
    // Event that represents when the ClusterHoprd has created a new node
    NodeCreated,
    // Event that represents when the ClusterHoprd has created a new node
    NodeDeleted,
}

impl Display for ClusterHoprdPhaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            ClusterHoprdPhaseEnum::Initialized => write!(f, "Initialized"),
            ClusterHoprdPhaseEnum::NotScaled => write!(f, "NotScaled"),
            ClusterHoprdPhaseEnum::Scaling => write!(f, "Scaling"),
            ClusterHoprdPhaseEnum::Failed => write!(f, "Failed"),
            ClusterHoprdPhaseEnum::Ready => write!(f, "Ready"),
            ClusterHoprdPhaseEnum::Deleting => write!(f, "Deleting"),
            ClusterHoprdPhaseEnum::NodeCreated => write!(f, "NodeCreated"),
            ClusterHoprdPhaseEnum::NodeDeleted => write!(f, "NodeDeleted"),
        }
    }
}

impl Default for ClusterHoprd {
    fn default() -> Self {
        Self {
            metadata: ObjectMeta::default(),
            spec: ClusterHoprdSpec::default(),
            status: Some(ClusterHoprdStatus::default()),
        }
    }
}

impl ClusterHoprd {
    /// Creates the hoprd nodes related with ClusterHoprd
    pub async fn create(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String = self.name_any();
        info!("Starting to create ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace}");
        resource_generics::add_finalizer(client.clone(), self).await;
        context_data.send_event(self, ClusterHoprdEventEnum::Initialized, None).await;
        self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Initialized).await?;
        if self.spec.replicas > 0 {
            context_data.send_event(self, ClusterHoprdEventEnum::NotScaled, Some(self.spec.replicas.to_string())).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::NotScaled).await?;
        } else {
            context_data.send_event(self, ClusterHoprdEventEnum::Ready, None).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Ready).await?;
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    // Modifies the hoprd nodes related with ClusterHoprd
    pub async fn modify(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String = self.name_any();
        if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&ClusterHoprdPhaseEnum::Failed) {
            // Assumes that the next modification of the resource is to recover to a good state
            context_data.send_event(self, ClusterHoprdEventEnum::Ready, None).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Ready).await?;
            warn!("Detected a change in ClusterHoprd {cluster_hoprd_name} while was in Failed phase. Automatically recovering to a Ready phase");
        } else if self.annotations().contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
            let previous_config_text: String = self.annotations().get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
            match serde_json::from_str::<ClusterHoprd>(&previous_config_text) {
                Ok(previous_cluster_hoprd) => {
                    if self.changed_inmutable_fields(&previous_cluster_hoprd.spec) {
                        context_data.send_event(self, ClusterHoprdEventEnum::Failed, None).await;
                        self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Failed).await?;
                    } else {
                        self.appply_modification(context_data.clone()).await?;
                        self.check_needs_rescale(context_data.clone()).await?;
                        info!("ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} has been successfully modified");
                    }
                }
                Err(_err) => {
                    context_data.send_event(self, ClusterHoprdEventEnum::Failed, None).await;
                    self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Failed).await?;
                    error!("Could not parse the last applied configuration of ClusterHoprd {cluster_hoprd_name}");
                }
            }
            self.update_last_configuration(context_data.client.clone()).await?;
        } else {
            context_data.send_event(self, ClusterHoprdEventEnum::Failed, None).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Failed).await?;
            error!("Could not modify ClusterHoprd {cluster_hoprd_name} because cannot recover last configuration");
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
    }

    async fn update_last_configuration(&self, client: Client) -> Result<(), Error> {
        let api: Api<ClusterHoprd> = Api::namespaced(client, &self.namespace().unwrap());
        let mut cloned_cluster = self.clone();
        cloned_cluster.status = None;
        cloned_cluster.metadata.managed_fields = None;
        cloned_cluster.metadata.creation_timestamp = None;
        cloned_cluster.metadata.finalizers = None;
        cloned_cluster.metadata.annotations = None;
        cloned_cluster.metadata.generation = None;
        cloned_cluster.metadata.resource_version = None;
        cloned_cluster.metadata.uid = None;
        let cluster_last_configuration = serde_json::to_string(&cloned_cluster).unwrap();
        let mut annotations = BTreeMap::new();
        annotations.insert(constants::ANNOTATION_LAST_CONFIGURATION.to_string(), cluster_last_configuration);
        let patch = Patch::Merge(json!({
            "metadata": {
                "annotations": annotations
            }
        }));
        match api.patch(&self.name_any(), &PatchParams::default(), &patch).await {
            Ok(_cluster_hopr) => Ok(()),
            Err(error) => Ok(error!("Could not update last configuration annotation on ClusterHoprd {}: {:?}", self.name_any(), error)),
        }
    }

    // Handle rescaling
    async fn check_needs_rescale(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String = self.name_any();
        let unsynched_nodes: i32 = self.spec.replicas - self.status.as_ref().unwrap().current_nodes;
        if unsynched_nodes != 0 {
            if unsynched_nodes > 0 {
                info!("ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} requires to create {} new nodes", unsynched_nodes);
            } else {
                info!("ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} requires to delete {} nodes", unsynched_nodes.abs());
            }
            context_data.send_event(self, ClusterHoprdEventEnum::NotScaled, Some(unsynched_nodes.to_string())).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::NotScaled).await?;
        } else {
            context_data.send_event(self, ClusterHoprdEventEnum::Ready, None).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Ready).await?;
        }
        Ok(())
    }

    // Sync Cluster with its hoprd nodes
    pub async fn rescale(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String = self.name_any();
        let status = self.status.as_ref().unwrap();
        if status.phase.eq(&ClusterHoprdPhaseEnum::NotScaled) {
            context_data.send_event(self, ClusterHoprdEventEnum::Scaling, None).await;
            self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Scaling).await?;
            let current_unsynched_nodes = self.spec.replicas - status.current_nodes;
            info!("ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} is not scaled");
            match current_unsynched_nodes {
                1..=i32::MAX => {
                    self.create_node(context_data.clone()).await.expect("Could not scale up cluster");
                }
                i32::MIN..=-1 => {
                    self.delete_node(context_data.clone()).await.expect("Could not scale down cluster");
                }
                0 => {
                    context_data.send_event(self, ClusterHoprdEventEnum::Ready, None).await;
                    self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Ready).await?;
                }
            }
            Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY)))
        } else {
            debug!(
                "ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} is already being scaling, currently having {} nodes while needs {}",
                status.current_nodes, self.spec.replicas
            );
            Ok(Action::await_change())
        }
    }

    // Deletes the hoprd nodes related with ClusterHoprd
    pub async fn delete(&self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let cluster_hoprd_name = self.name_any();
        let hoprd_namespace = self.namespace().unwrap();
        let client: Client = context_data.client.clone();
        self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::Deleting).await?;
        context_data.send_event(self, ClusterHoprdEventEnum::Deleting, None).await;
        info!("Starting to delete ClusterHoprd {cluster_hoprd_name} from namespace {hoprd_namespace}");
        self.delete_nodes(context_data.clone()).await.unwrap_or(());
        resource_generics::delete_finalizer(client.clone(), self).await?;
        info!("ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} has been successfully deleted");
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }

    /// Check the fileds that cannot be modifed
    fn changed_inmutable_fields(&self, spec: &ClusterHoprdSpec) -> bool {
        if !self.spec.identity_pool_name.eq(&spec.identity_pool_name) {
            error!("Cluster configuration is invalid, identity_pool_name field cannot be changed on {}.", self.name_any());
            true
        } else if self.spec.service.is_some() && !self.spec.service.as_ref().unwrap().r#type.eq(&spec.service.as_ref().unwrap().r#type) {
            error!("Cluster configuration is invalid, service Type field cannot be changed on {}.", self.name_any());
            true
        } else {
            false
        }
    }

    /// Updates the status of ClusterHoprd
    pub async fn update_status(&self, context_data: Arc<ContextData>, phase: ClusterHoprdPhaseEnum) -> Result<(), Error> {
        let client: Client = context_data.client.clone();
        let cluster_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
        let api: Api<ClusterHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        // We need to get the latest state of the ClusterHoprd  as it may be updated by other thread and the values stored in self object might be obsolete
        let cluster_hoprd = api.get(&cluster_hoprd_name).await.unwrap();
        let mut cluster_hoprd_status = cluster_hoprd.status.as_ref().unwrap_or(&ClusterHoprdStatus::default()).to_owned();

        cluster_hoprd_status.update_timestamp = Utc::now().to_rfc3339();
        cluster_hoprd_status.checksum = cluster_hoprd.get_checksum();
        cluster_hoprd_status.phase = phase;
        if phase.eq(&ClusterHoprdPhaseEnum::NodeCreated) {
            cluster_hoprd_status.current_nodes += 1;
        } else if phase.eq(&ClusterHoprdPhaseEnum::NodeDeleted) {
            cluster_hoprd_status.current_nodes -= 1;
        };
        if phase.eq(&ClusterHoprdPhaseEnum::NodeCreated) || phase.eq(&ClusterHoprdPhaseEnum::NodeDeleted) {
            if cluster_hoprd_status.current_nodes == cluster_hoprd.spec.replicas {
                cluster_hoprd_status.phase = ClusterHoprdPhaseEnum::Ready;
            } else {
                cluster_hoprd_status.phase = ClusterHoprdPhaseEnum::NotScaled;
            }
        };
        let patch = Patch::Merge(json!({"status": cluster_hoprd_status }));
        match api.patch(&cluster_hoprd_name, &PatchParams::default(), &patch).await {
            Ok(_cluster_hopr) => Ok(debug!("ClusterHoprd current status {:?}", cluster_hoprd_status)),
            Err(error) => Ok(error!("Could not update phase {} on cluster {cluster_hoprd_name}: {:?}", cluster_hoprd_status.phase, error)),
        }
    }

    // Finds the next free node in the cluster. We use this function, because a node might be missing in the middle of the list of nodes
    async fn get_next_free_node(&self, client: Client) -> i32 {
        let api: Api<Hoprd> = Api::namespaced(client, &self.namespace().unwrap());
        let current_nodes = self.get_my_nodes(api).await.unwrap();
        debug!(
            "ClusterHoprd {} in namespace {} has currently {} nodes",
            self.name_any(),
            self.namespace().unwrap(),
            current_nodes.len()
        );
        let mut current_node_numbers = current_nodes
            .iter()
            .map(|n| {
                // Casting the node numbers and removing 1 unit to align them with the index of the array, so the nodes numbering starts from value 0 instead of 1
                n.name_any().replace(&format!("{}-", self.metadata.name.as_ref().unwrap()), "").parse::<i32>().unwrap() - 1
            })
            .collect::<Vec<i32>>();
        current_node_numbers.sort();
        let next = current_node_numbers
            .iter()
            .enumerate()
            .find_map(|(index, &value)| {
                let index_i32: i32 = index.try_into().unwrap();
                if index_i32 != value {
                    Some(index_i32 + 1)
                } else if index == current_node_numbers.len() - 1 {
                    Some(index_i32 + 2)
                } else {
                    None
                }
            })
            .unwrap_or(1);
        debug!("Next free node for ClusterHopr {} in namespace {} is {}", self.name_any(), self.namespace().unwrap(), next);
        next
    }

    /// Creates a set of hoprd resources with similar configuration
    async fn create_node(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let cluster_name = self.metadata.name.as_ref().unwrap().to_owned();
        let node_instance = self.get_next_free_node(context_data.client.clone()).await;
        let node_name = format!("{}-{}", cluster_name.to_owned(), node_instance).to_owned();
        context_data.send_event(self, ClusterHoprdEventEnum::CreatingNode, Some(node_name.to_owned())).await;
        let identity_name: Option<String> = match self.spec.force_identity_name {
            Some(force) => {
                if force {
                    Some(format!("{}-{}", self.spec.identity_pool_name, node_instance))
                } else {
                    None
                }
            }
            None => None,
        };
        info!("Creating node {} for cluster {}", node_name.to_owned(), cluster_name.to_owned());
        let hoprd_spec: HoprdSpec = HoprdSpec {
            config: self.spec.config.to_owned(),
            enabled: self.spec.enabled,
            version: self.spec.version.to_owned(),
            deployment: self.spec.deployment.to_owned(),
            identity_pool_name: self.spec.identity_pool_name.to_owned(),
            supported_release: self.spec.supported_release.to_owned(),
            delete_database: Some(false),
            service: Some(self.spec.service.as_ref().unwrap_or(&HoprdServiceSpec::default()).to_owned()),
            identity_name,
            ports_allocation: self.spec.ports_allocation.to_owned(),
        };
        match self.create_hoprd_resource(context_data.clone(), node_name.to_owned(), hoprd_spec).await {
            Ok(_) => {
                info!("Node {} successfully created for cluster {}", node_name.to_owned(), cluster_name.to_owned());
                context_data.send_event(self, ClusterHoprdEventEnum::NodeCreated, Some(node_name.to_owned())).await;
                self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::NodeCreated).await?;
            }
            Err(error) => {
                error!("{:?}", error);
            }
        }
        Ok(())
    }

    /// Creates a set of hoprd resources with similar configuration
    async fn delete_node(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let cluster_name = self.metadata.name.as_ref().unwrap().to_owned();
        let node_instance = self.status.clone().unwrap().current_nodes;
        let node_name = format!("{}-{}", cluster_name.to_owned(), node_instance).to_owned();
        context_data.send_event(self, ClusterHoprdEventEnum::DeletingNode, Some(node_name.to_owned())).await;
        info!("Deleting node {} from cluster {}", node_name, cluster_name.to_owned());
        let api: Api<Hoprd> = Api::namespaced(context_data.client.clone(), &self.namespace().unwrap());
        if let Some(hoprd_node) = api.get_opt(&node_name).await? {
            let uid = hoprd_node.metadata.uid.unwrap();
            api.delete(&node_name, &DeleteParams::default()).await?;
            await_condition(api.clone(), &node_name, conditions::is_deleted(&uid)).await.unwrap();
            info!("Node {} deleted from cluster {}", node_name, cluster_name.to_owned());
        };
        context_data.send_event(self, ClusterHoprdEventEnum::NodeDeleted, Some(node_name)).await;
        self.update_status(context_data.clone(), ClusterHoprdPhaseEnum::NodeDeleted).await?;
        Ok(())
    }

    /// Creates a hoprd resource
    async fn create_hoprd_resource(&self, context_data: Arc<ContextData>, name: String, hoprd_spec: HoprdSpec) -> Result<Hoprd, Error> {
        let mut labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(), Some(name.to_owned()), Some("node".to_owned()));
        labels.insert(constants::LABEL_NODE_CLUSTER.to_owned(), self.name_any());
        let api: Api<Hoprd> = Api::namespaced(context_data.client.clone(), &self.namespace().unwrap());
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        let mut hoprd: Hoprd = Hoprd {
            metadata: ObjectMeta {
                labels: Some(labels.clone()),
                name: Some(name.to_owned()),
                namespace: self.namespace().to_owned(),
                owner_references,
                ..ObjectMeta::default()
            },
            spec: hoprd_spec.to_owned(),
            status: None,
        };
        let hoprd_last_configuration = serde_json::to_string(&hoprd).unwrap();
        let mut hoprd_annotations: BTreeMap<String, String> = BTreeMap::new();
        hoprd_annotations.insert(constants::ANNOTATION_LAST_CONFIGURATION.to_string(), hoprd_last_configuration);
        hoprd.metadata.annotations = Some(hoprd_annotations);
        // Create the Hoprd resource defined above
        let hoprd_created = api.create(&PostParams::default(), &hoprd).await?;
        hoprd_created.wait_deployment(context_data.client.clone()).await?;
        Ok(hoprd_created)
    }

    /// Creates a hoprd resource
    async fn appply_modification(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let api: Api<Hoprd> = Api::namespaced(context_data.client.clone(), &self.namespace().unwrap());
        let mut hoprd_spec: HoprdSpec = HoprdSpec {
            config: self.spec.config.to_owned(),
            enabled: self.spec.enabled,
            version: self.spec.version.to_owned(),
            deployment: self.spec.deployment.to_owned(),
            delete_database: Some(false),
            identity_pool_name: self.spec.identity_pool_name.to_owned(),
            supported_release: self.spec.supported_release.to_owned(),
            service: self.spec.service.to_owned(),
            identity_name: None,
            ports_allocation: self.spec.ports_allocation.to_owned(),
        };

        for hoprd_node in self.get_my_nodes(api.clone()).await.unwrap() {
            hoprd_spec.identity_name = hoprd_node.spec.identity_name.clone();
            let patch = &Patch::Merge(json!({ "spec": hoprd_spec }));
            let hoprd_modified = api.patch(&hoprd_node.name_any(), &PatchParams::default(), patch).await.unwrap();
            hoprd_modified.wait_deployment(context_data.client.clone()).await?;
        }
        Ok(())
    }

    pub fn get_checksum(&self) -> String {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        hasher.finish().to_string()
    }

    /// Get the hoprd nodes owned by the ClusterHoprd
    async fn get_my_nodes(&self, api: Api<Hoprd>) -> Result<Vec<Hoprd>, Error> {
        let label_selector: String = format!("{}={}", constants::LABEL_NODE_CLUSTER, self.name_any());
        let lp = ListParams::default().labels(&label_selector);
        let nodes = api.list(&lp).await?;
        Ok(nodes.items)
    }

    // Delete hoprd nodes related to the cluster
    async fn delete_nodes(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let api: Api<Hoprd> = Api::namespaced(context_data.client.clone(), &self.namespace().unwrap());
        let nodes = self.get_my_nodes(api.clone()).await?;
        for node in nodes {
            let node_name = &node.name_any();
            let uid = node.metadata.uid.unwrap();
            api.delete(node_name, &DeleteParams::default()).await?;
            await_condition(api.clone(), node_name, conditions::is_deleted(&uid))
                .await
                .expect(&format!("Could not delete the node {}", node_name));
        }
        Ok(())
    }
}
