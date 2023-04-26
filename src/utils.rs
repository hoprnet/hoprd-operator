use std::{collections::{BTreeMap}, fmt::{Display, Formatter, self}};
use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{api::{core::v1::{ Secret}}};
use kube::{Api, api::{ Patch, PatchParams}, Client};
use serde_json::{Value, json};
use crate::{constants, model::{Error}, hoprd::{Hoprd}, cluster::ClusterHoprd};

pub fn common_lables(instance_name: &String) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), "hoprd".to_owned());
    labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance_name.to_owned());
    return labels;
}

pub enum ResourceType {
    Secret,
    Hoprd,
    ClusterHoprd
}

impl Display for ResourceType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ResourceType::Secret => write!(f, "Secret"),
            ResourceType::Hoprd => write!(f, "Hoprd"),
            ResourceType::ClusterHoprd => write!(f, "ClusterHoprd")
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
        ResourceType::ClusterHoprd => { 
            let api_cluster_hoprd: Api<ClusterHoprd> = Api::namespaced(client.clone(), &resource_namespace);
            match api_cluster_hoprd.get_opt(&resource_name).await.unwrap() {
                Some(hoprd) => { 
                    if resource_kind.eq(&ResourceKind::Labels) {
                        hoprd.metadata.labels.as_ref().unwrap_or_else(|| empty_map).clone()
                    } else {
                        hoprd.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone()
                    }
                }
                None => {
                    println!("The cluster hoprd {resource_name} does not exist.");
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

pub async fn delete_secret_annotations(api: &Api<Secret>, secret_name: &str, annotation_name: &str) -> Result<Secret, Error> {
    match api.get_opt(&secret_name).await.unwrap() {
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
            api.patch(&secret_name, &PatchParams::default(), &patch).await?;
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

