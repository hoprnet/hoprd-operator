use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use crate::hoprd::{HoprdSpec, HoprdConfig};
use crate::utils;
use crate::{constants, context_data::ContextData, hoprd::Hoprd};
use crate::model::{EnablingFlag, DeploymentResource, Error, ClusterHoprdStatusEnum};
use chrono::Utc;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::Resource;
use kube::api::{PostParams, ListParams, DeleteParams};
use kube::core::{ObjectMeta, WatchEvent};
use kube::runtime::events::Recorder;
use kube::runtime::wait::{conditions, await_condition};
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::{json};
use kube::{
    api::{Api, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action}, events::{Event, EventType}
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
    kind = "ClusterHoprd",
    plural = "clusterhoprds",
    derive = "PartialEq",
    namespaced
)]
#[kube(status = "ClusterHoprdStatus", shortname = "clusterhoprd")]
#[serde(rename_all = "camelCase")]
pub struct ClusterHoprdSpec {
    pub network: String,    
    pub nodes: Vec<Node>,
    pub ingress: Option<EnablingFlag>,
    pub monitoring: Option<EnablingFlag>

}

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone, JsonSchema, Hash)]
pub struct Node {
    pub name: String,
    pub replicas: i32,
    pub config: Option<HoprdConfig>,
    pub enabled: Option<bool>,
    pub resources: Option<DeploymentResource>,
    pub version: String
}


/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
pub struct ClusterHoprdStatus {
    pub update_timestamp: i64,
    pub status: ClusterHoprdStatusEnum,
    pub checksum: String,
}

impl ClusterHoprd {

    /// Creates the hoprd nodes related with ClusterHoprd
    pub async fn create(&self, context: Arc<ContextData>) -> Result<Action> {
        let client: Client = context.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String= self.name_any();
        self.create_event(context.clone(), ClusterHoprdStatusEnum::Initializing, None).await.unwrap();
        self.update_status(context.clone(), ClusterHoprdStatusEnum::Initializing).await.unwrap();
        println!("[INFO] Starting to create ClusterHoprd  {cluster_hoprd_name} in namespace {hoprd_namespace}");
        self.add_finalizer(client.clone(), &cluster_hoprd_name, &hoprd_namespace).await.unwrap();
        let mut out_of_sync = false;
        for node_set in self.spec.nodes.to_owned() {
            self.create_node_set(context.clone(), node_set, &mut out_of_sync).await.unwrap();
        }
        if ! out_of_sync {
            self.create_event(context.clone(), ClusterHoprdStatusEnum::InSync, None).await.unwrap();
            self.update_status(context.clone(), ClusterHoprdStatusEnum::InSync).await.unwrap();
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    // Modifies the hoprd nodes related with ClusterHoprd
    pub async fn modify(&self, context: Arc<ContextData>) -> Result<Action> {
        let client: Client = context.client.clone();
        let hoprd_namespace: String = self.namespace().unwrap();
        let cluster_hoprd_name: String= self.name_any();
        println!("[INFO] ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} has been successfully modified");
        self.create_event(context.clone(), ClusterHoprdStatusEnum::Synching, None).await.unwrap();
        self.update_status(context.clone(), ClusterHoprdStatusEnum::Synching).await.unwrap();
        let annotations = utils::get_resource_kinds(client.clone(), utils::ResourceType::ClusterHoprd, utils::ResourceKind::Annotations, &cluster_hoprd_name, &hoprd_namespace).await;
        if annotations.contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
            let previous_config_text: String = annotations.get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
            match serde_json::from_str::<ClusterHoprd>(&previous_config_text) {
                Ok(modified_cluster_hoprd) => {
                    self.check_inmutable_fields(&modified_cluster_hoprd.spec).unwrap();
                },
                Err(_err) => {
                    println!("[ERROR] Could not parse the last applied configuration of Hoprd node {cluster_hoprd_name}.");
                }
            }
        } else {
            println!("[WARN] The ClusterHoprd {cluster_hoprd_name} resource did not have previous configuration")
        }

        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    // Deletes the hoprd nodes related with ClusterHoprd
    pub async fn delete(&self, context: Arc<ContextData>) -> Result<Action> {
        let cluster_hoprd_name = self.name_any();
        let hoprd_namespace = self.namespace().unwrap();
        let client: Client = context.client.clone();
        self.create_event(context.clone(),  ClusterHoprdStatusEnum::Deleting, None).await.unwrap();
        println!("[INFO] Starting to delete ClusterHoprd {cluster_hoprd_name} from namespace {hoprd_namespace}");
        self.delete_nodes(client.clone()).await.unwrap();
        self.delete_finalizer(client.clone(), &cluster_hoprd_name, &hoprd_namespace).await.unwrap();
        println!("[INFO] ClusterHoprd {cluster_hoprd_name} in namespace {hoprd_namespace} has been successfully deleted");
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }

    /// Adds a finalizer in ClusterHoprd to prevent deletion of the resource by Kubernetes API and allow the controller to safely manage its deletion 
    async fn add_finalizer(&self, client: Client, hoprd_name: &str, hoprd_namespace: &str) -> Result<ClusterHoprd, Error> {
        let api: Api<ClusterHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        let pp = PatchParams::default();
        let patch = json!({
           "metadata": {
                "finalizers": [constants::OPERATOR_FINALIZER]
            }
        });
        match api.patch(&hoprd_name, &pp, &Patch::Merge(patch)).await {
            Ok(hopr) => Ok(hopr),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(Error::HoprdStatusError(format!("Could not add finalizer on {hoprd_name}.").to_owned()));
            }
        }
    }

    /// Deletes the finalizer of ClusterHoprd resource, so the resource can be freely deleted by Kubernetes API
    async fn delete_finalizer(&self, client: Client, cluster_name: &str, hoprd_namespace: &str) -> Result<(), Error> {
        let api: Api<ClusterHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        let pp = PatchParams::default();
        let patch = json!({
           "metadata": {
                "finalizers": null
            }
        });
        if let Some(_) = api.get_opt(&cluster_name).await? {
            match api.patch(&cluster_name, &pp, &Patch::Merge(patch)).await {
                Ok(_hopr) => Ok(()),
                Err(error) => {
                    println!("[ERROR]: {:?}", error);
                    return Err(Error::HoprdStatusError(format!("Could not delete finalizer on {cluster_name}.").to_owned()));
                }
            }
        } else {
            println!("[INFO] ClusterHoprd {cluster_name} has already been deleted");
            Ok(())
        }
    }

    /// Check the fileds that cannot be modifed
    fn check_inmutable_fields(&self, spec: &ClusterHoprdSpec) -> Result<(),Error> {
        if ! self.spec.network.eq(&spec.network) {
            return Err(Error::HoprdConfigError(format!("Hoprd configuration is invalid, network field cannot be changed on {}.", self.name_any())));
        }
        Ok(())
    }

    /// Creates an event for ClusterHoprd given the new ClusterHoprdStatusEnum
    async fn create_event(&self, context: Arc<ContextData>, status: ClusterHoprdStatusEnum, node_name: Option<String>) -> Result<(), Error> {
        let client: Client = context.client.clone();   
        let ev: Event = match status {
            ClusterHoprdStatusEnum::Initializing => Event {
                        type_: EventType::Normal,
                        reason: "Initializing".to_string(),
                        note: Some("Initializing ClusterHoprd node".to_owned()),
                        action: "Starting the process of creating a new cluster of hoprd".to_string(),
                        secondary: None,
                    },
            ClusterHoprdStatusEnum::Synching => Event {
                        type_: EventType::Normal,
                        reason: "Synching".to_string(),
                        note: Some(format!("ClusterHoprd synchronized with node {}", node_name.unwrap_or("".to_owned()))),
                        action: "Node secrets are being created".to_string(),
                        secondary: None,
                    },
            ClusterHoprdStatusEnum::InSync => Event {
                        type_: EventType::Normal,
                        reason: "RegisteringInNetwork".to_string(),
                        note: Some("ClusterHoprd is sinchronized".to_owned()),
                        action: "Node is registering into the Network registry".to_string(),
                        secondary: None,
                    },
        
            ClusterHoprdStatusEnum::Deleting => Event {
                        type_: EventType::Normal,
                        reason: "Deleting".to_string(),
                        note: Some("ClusterHoprd is being deleted".to_owned()),
                        action: "Node deletion started".to_string(),
                        secondary: None,
                    },
            ClusterHoprdStatusEnum::OutOfSync => Event {
                        type_: EventType::Warning,
                        reason: "Out of sync".to_string(),
                        note: Some(format!("ClusterHoprd is not sync with node {}", node_name.unwrap_or("unknown".to_owned()))),
                        action: "Node sync failed".to_string(),
                        secondary: None,
                    }

        };
        let recorder: Recorder = context.state.read().await.generate_cluster_hoprd_event(client.clone(), self);
        Ok(recorder.publish(ev).await?)
    }

    /// Updates the status of ClusterHoprd
    async fn update_status(&self, context: Arc<ContextData>, status: ClusterHoprdStatusEnum) -> Result<(), Error> {
    let client: Client = context.client.clone();
    let cluster_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();    
    let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();

    let api: Api<ClusterHoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
    if status.eq(&ClusterHoprdStatusEnum::Deleting) {
        Ok(())
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        let hash: u64 = hasher.finish();
        let status = ClusterHoprdStatus {
                update_timestamp: Utc::now().timestamp(),
                status: status,
                checksum: format!("checksum-{}",hash.to_string())
        };
        let pp = PatchParams::default();
        let patch = json!({
                "status": status
        });
        match api.patch(&cluster_hoprd_name, &pp, &Patch::Merge(patch)).await {
            Ok(_cluster_hopr) => Ok(()),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(Error::HoprdStatusError(format!("Could not update status on cluster {cluster_hoprd_name}.")));
            }
        }
    }

    
}

    /// Creates a set of hoprd resources with similar configuration
    async fn create_node_set(&self,  context: Arc<ContextData>, node_set: Node, out_of_sync: &mut bool) -> Result<(), Error> {
        let client: Client = context.client.clone();
        for node_instance in 0..node_set.replicas.to_owned() {
            let name = format!("{}-{}-{}", self.name_any(), node_set.name.to_owned(), node_instance.to_owned()).to_owned();
            let hoprd_spec: HoprdSpec = HoprdSpec {
                config: node_set.config.to_owned(),
                enabled: node_set.enabled,
                network: self.spec.network.to_owned(),
                ingress: self.spec.ingress.to_owned(),
                monitoring: self.spec.monitoring.to_owned(),
                version: node_set.version.to_owned(),
                resources: node_set.resources.to_owned(),
                secret: None
            };
            if self.create_hoprd_resource(client.clone(), name.to_owned(), hoprd_spec).await.is_ok() {
                self.create_event(context.clone(), ClusterHoprdStatusEnum::Synching, Some(name)).await.unwrap();
            } else {
                *out_of_sync = true;
                self.create_event(context.clone(), ClusterHoprdStatusEnum::OutOfSync, Some(name)).await.unwrap();
            }
        }
        Ok(())
    }

    /// Creates a hoprd resource
    async fn create_hoprd_resource(&self, client: Client, name: String, hoprd_spec: HoprdSpec) -> Result<Hoprd, Error> {
        let mut labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());
        labels.insert(constants::LABEL_NODE_CLUSTER.to_owned(), self.name_any());
        let api: Api<Hoprd> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        let hoprd: Hoprd = Hoprd {
            metadata: ObjectMeta { 
                labels: Some(labels.clone()),
                name: Some(name.to_owned()), 
                namespace: self.namespace().to_owned(),
                owner_references,
                ..ObjectMeta::default()
                },
            spec: hoprd_spec.to_owned(),
            status: None
        };
        // Create the Hoprd resource defined above
        let hoprd_created = api.create(&PostParams::default(), &hoprd).await.unwrap();
        // Wait for the Hoprd deployment to be created
        let lp = ListParams::default()
            .fields(&format!("metadata.name={name}"))
            .timeout(constants::OPERATOR_NODE_SYNC_TIMEOUT);
        let deployment_api: Api<Deployment> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        let mut stream = deployment_api.watch(&lp, "0").await?.boxed();
        while let Some(deployment) = stream.try_next().await.unwrap() {
            match deployment {
                WatchEvent::Added(deployment) => {
                    println!("[DEBUG] Deployment uid {:?} is created", deployment.uid().unwrap());
                    println!("[INFO] Hoprd {name} has been added to the cluster");
                    return Ok(hoprd_created);
                },
                WatchEvent::Modified(_) => return Err(Error::ClusterHoprdSynchError("Modified operation not expected".to_owned())),
                WatchEvent::Deleted(_) => return Err(Error::ClusterHoprdSynchError("Deleted operation not expected".to_owned())),
                WatchEvent::Bookmark(_) => return Err(Error::ClusterHoprdSynchError("Bookmark operation not expected".to_owned())),
                WatchEvent::Error(_) => return Err(Error::ClusterHoprdSynchError("Error operation not expected".to_owned()))
            }
        }
        return Err(Error::ClusterHoprdSynchError("Timeout waiting for Hoprd node to be created".to_owned()))
    }

    /// Get the hoprd nodes owned by the ClusterHoprd
    async fn get_my_nodes(&self, api: Api<Hoprd>) -> Result<Vec<Hoprd>, Error> {
        let label_selector: String = format!("{}={}", constants::LABEL_NODE_CLUSTER, self.name_any());
        let lp = ListParams::default().labels(&label_selector);
        let nodes = api.list(&lp).await?;
        Ok(nodes.items)
    }

    // Delete hoprd nodes related to the cluster
    async fn delete_nodes(&self, client: Client) -> Result<(), Error> {
        let api: Api<Hoprd> = Api::namespaced(client.clone(), &self.namespace().unwrap());
        let nodes = self.get_my_nodes(api.clone()).await.unwrap();
        for node in nodes {
            let node_name = &node.name_any();
            let uid = &node.uid().unwrap();
            api.delete(node_name,  &DeleteParams::default()).await?;
            let hoprd_deleted = await_condition(api.clone(), node_name, conditions::is_deleted(uid));
            match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_NODE_SYNC_TIMEOUT.into()), hoprd_deleted).await {
                Ok(_) => {},
                Err(_error) => {
                    println!("The Hoprd node {:?} deletion failed", node.name_any())
                }
            }
        }
        Ok(())
    }
}


