use std::{collections::{BTreeMap, hash_map::DefaultHasher}, sync::Arc, hash::{Hash, Hasher}, fmt::{Display, Formatter, self}};
use chrono::Utc;
use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{api::{core::v1::{ResourceRequirements, Secret}}, apimachinery::pkg::{api::resource::Quantity}};
use kube::{Api, api::{ Patch, PatchParams}, Client, runtime::events::{Recorder, Event, EventType}};
use serde_json::{Value, json};


use crate::{constants, model::{DeploymentResource, HoprdStatusEnum, Error}, controller::ContextData, hoprd::{Hoprd, HoprdStatus}};

pub fn common_lables(instance_name: &String) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), "hoprd".to_owned());
    labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance_name.to_owned());
    return labels;
}

/// Builds the struct ResourceRequirement from Resource specified in the node
///
/// # Arguments
/// - `resources` - The resources object on the Hoprd record
pub fn build_resource_requirements(resources: &Option<DeploymentResource>) -> Option<ResourceRequirements> {
    let mut value_limits: BTreeMap<String, Quantity> = BTreeMap::new();
    let mut value_requests: BTreeMap<String, Quantity> = BTreeMap::new();
    if resources.is_some() {
        let resource = resources.as_ref().unwrap();
        value_limits.insert("cpu".to_owned(), Quantity(resource.limits.cpu.to_owned()));
        value_limits.insert(
            "memory".to_owned(),
            Quantity(resource.limits.memory.to_owned()),
        );
        value_requests.insert("cpu".to_owned(), Quantity(resource.requests.cpu.to_owned()));
        value_requests.insert(
            "memory".to_owned(),
            Quantity(resource.requests.memory.to_owned()),
        );
    } else {
        value_limits.insert("cpu".to_owned(), Quantity("1500m".to_owned()));
        value_limits.insert("memory".to_owned(), Quantity("2Gi".to_owned()));
        value_requests.insert("cpu".to_owned(), Quantity("750m".to_owned()));
        value_requests.insert("memory".to_owned(), Quantity("256Mi".to_owned()));
    }
    return Some(ResourceRequirements {
        limits: Some(value_limits),
        requests: Some(value_requests),
    });
}

pub enum ResourceType {
    Secret,
    Hoprd
}

impl Display for ResourceType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ResourceType::Secret => write!(f, "Secret"),
            ResourceType::Hoprd => write!(f, "Hoprd")
        }
    }
}
#[derive( PartialEq, Clone)]
pub enum ResourceKind {
    Labels,
    Annotations
}

impl Display for ResourceKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ResourceKind::Labels => write!(f, "Labels"),
            ResourceKind::Annotations => write!(f, "Annotations")
        }
    }
}


pub async fn get_resource_kinds(client: Client, resource_type: ResourceType, resource_kind: ResourceKind, resource_name: &str, resource_namespace: &str) -> BTreeMap<String, String> {
    let empty_map: &BTreeMap<String, String> = &BTreeMap::new();
    match resource_type {
        ResourceType::Secret => { 
            let api_secret: Api<Secret> = Api::namespaced(client.clone(), &resource_namespace);
            match api_secret.get_opt(&resource_name).await.unwrap() {
                Some(secret) => { 
                    if resource_kind.eq(&ResourceKind::Labels) {
                        secret.metadata.labels.as_ref().unwrap_or_else(|| empty_map).clone()
                    } else {
                        secret.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone()
                    }
                }
                None => {
                    println!("The secret {resource_name} does not exist.");
                    empty_map.clone()
                }
            }
        } 
        ResourceType::Hoprd => { 
            let api_hoprd: Api<Hoprd> = Api::namespaced(client.clone(), &resource_namespace);
            match api_hoprd.get_opt(&resource_name).await.unwrap() {
                Some(hoprd) => { 
                    if resource_kind.eq(&ResourceKind::Labels) {
                        hoprd.metadata.labels.as_ref().unwrap_or_else(|| empty_map).clone()
                    } else {
                        hoprd.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone()
                    }
                }
                None => {
                    println!("The hoprd {resource_name} does not exist.");
                    empty_map.clone()
                }
            }
        }
    }
}


pub async fn update_secret_annotations(api_secret: &Api<Secret>, secret_name: &str, annotation_name: &str, annotation_value: &str) -> Result<Secret, Error> {
    match api_secret.get_opt(&secret_name).await.unwrap() {
        Some(secret) => {
            let empty_map = &mut BTreeMap::new();
            let mut hoprd_annotations: BTreeMap<String,String> = secret.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone();
            if hoprd_annotations.contains_key(annotation_name) {
                *hoprd_annotations.get_mut(annotation_name).unwrap() = annotation_value.to_string();
            } else {
                hoprd_annotations.insert(annotation_name.to_string(), annotation_value.to_string());
            }
            let secret_patch_object: Value = json!({
                "metadata": {
                    "annotations": hoprd_annotations
                }
            });
            let patch: Patch<&Value> = Patch::Merge(&secret_patch_object);
            Ok(api_secret.patch(&secret_name, &PatchParams::default(), &patch).await?)
        }
        None => { 
            return Err(Error::SecretStatusError(format!("[ERROR] The secret specified {secret_name} does not exist").to_owned()
            ));
        }
    }
}

pub async fn delete_secret_annotations(api_secret: &Api<Secret>, secret_name: &str, annotation_name: &str) -> Result<Secret, Error> {
    match api_secret.get_opt(&secret_name).await.unwrap() {
        Some(secret) => {
            let empty_map = &mut BTreeMap::new();
            let mut hoprd_annotations: BTreeMap<String,String> = secret.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone();
            if hoprd_annotations.contains_key(annotation_name) {
                hoprd_annotations.remove(annotation_name);
            } else {
              println!("[WARN] The secret {secret_name} does not contain the annotation {annotation_name}");
            }
            let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation{
                path: "/metadata/annotations".to_owned(),
                value: json!(hoprd_annotations)
            })]);
            let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
            api_secret.patch(&secret_name, &PatchParams::default(), &patch).await?;
            Ok(secret)
        }
        None => { 
            return Err(Error::SecretStatusError(format!("[ERROR] The secret specified {secret_name} does not exist").to_owned()
            ));
        }
    }
}

pub async fn update_secret_label(api_secret: &Api<Secret>, secret_name: &str, label_name: &str, label_value: &String) -> Result<Secret, Error> {
    match api_secret.get_opt(&secret_name).await.unwrap() {
        Some(secret) => {
            let empty_map = &mut BTreeMap::new();
            let mut hoprd_labels: BTreeMap<String,String> = secret.metadata.labels.as_ref().unwrap_or_else(|| empty_map).clone();
            if hoprd_labels.contains_key(label_name) {
                *hoprd_labels.get_mut(label_name).unwrap() = label_value.to_string();
            } else {
                hoprd_labels.insert(label_name.to_string(), label_value.to_string());
            }

            let secret_patch_object: Value = json!({
                "metadata": {
                    "labels": hoprd_labels
                }
            });
            let patch: Patch<&Value> = Patch::Merge(&secret_patch_object);
            Ok(api_secret.patch(&secret_name, &PatchParams::default(), &patch).await?)
        }
        None => { 
            return Err(Error::SecretStatusError(format!("[ERROR] The secret specified {secret_name} does not exist").to_owned()
            ));
        }
    }
}

pub async fn update_hoprd_status(context: Arc<ContextData>, hoprd: &Hoprd, status: HoprdStatusEnum) -> Result<Hoprd, Error> {
    let client: Client = context.client.clone();
    let hoprd_name = hoprd.metadata.name.as_ref().unwrap().to_owned();    
    let ev: Event = match status {
        HoprdStatusEnum::Initializing => Event {
                    type_: EventType::Normal,
                    reason: "Initializing".to_string(),
                    note: Some(format!("Initializing Hoprd node `{hoprd_name}`")),
                    action: "Starting the process of creating a new node".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Creating => Event {
                    type_: EventType::Normal,
                    reason: "Creating".to_string(),
                    note: Some(format!("Creating Hoprd node repository and secrets`{hoprd_name}`")),
                    action: "Node secrets are being created".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::RegisteringInNetwork => Event {
                    type_: EventType::Normal,
                    reason: "RegisteringInNetwork".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` created but not registered yet")),
                    action: "Node is registering into the Network registry".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Funding => Event {
                    type_: EventType::Normal,
                    reason: "Funding".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` created and registered but not funded yet")),
                    action: "Node is being funded with mHopr and xDAI".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Stopped => Event {
                    type_: EventType::Normal,
                    reason: "Stopped".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` is stopped")),
                    action: "Node has stopped".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Running => Event {
                    type_: EventType::Normal,
                    reason: "Running".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` is running")),
                    action: "Node has started".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Reloading => Event {
                    type_: EventType::Normal,
                    reason: "Reloading".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` configuration change detected")),
                    action: "Node reconfigured".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Deleting => Event {
                    type_: EventType::Normal,
                    reason: "Deleting".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` is being deleted")),
                    action: "Node deletion started".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Deleted => Event {
                    type_: EventType::Normal,
                    reason: "Deleted".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` is deleted")),
                    action: "Node deletion finished".to_string(),
                    secondary: None,
                },
        HoprdStatusEnum::Unsync => Event {
                    type_: EventType::Normal,
                    reason: "Unsync".to_string(),
                    note: Some(format!("Hoprd node `{hoprd_name}` is not sync")),
                    action: "Node sync failed".to_string(),
                    secondary: None,
                }

    };
    let recorder: Recorder = context.state.read().await.recorder(client.clone(), hoprd);
    recorder.publish(ev).await?;
    let hoprd_namespace = hoprd.metadata.namespace.as_ref().unwrap().to_owned();

    let api: Api<Hoprd> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
    if status.eq(&HoprdStatusEnum::Deleting) || status.eq(&HoprdStatusEnum::Deleted) {
        Ok(api.get(&hoprd_name).await?)
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        hoprd.spec.clone().hash(&mut hasher);
        let hash: u64 = hasher.finish();
        let status = HoprdStatus {
                update_timestamp: Utc::now().timestamp(),
                status: status,
                checksum: format!("checksum-{}",hash.to_string())
        };
        let pp = PatchParams::default();
        let patch = json!({
                "status": status
        });
        match api.patch(&hoprd_name, &pp, &Patch::Merge(patch)).await {
            Ok(hopr) => Ok(hopr),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(Error::HoprdStatusError(format!("Could not update status on {hoprd_name}.").to_owned()));
            }
        }
    }

    
}