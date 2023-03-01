use std::collections::{BTreeMap};
use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{api::core::v1::{ResourceRequirements, Secret}, apimachinery::pkg::{api::resource::Quantity}};
use kube::{Api, api::{ Patch, PatchParams}};
use serde_json::{Value, json};

use crate::{constants, crd::{Resource as HoprdResource}, actions::Error};

pub fn common_lables(instance_name: &String) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), "hoprd".to_owned());
    labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance_name.to_owned());
    return labels;
}

pub fn get_hopr_image_tag(tag: &String) -> String {
    let mut image = String::from(constants::HOPR_DOCKER_REGISTRY.to_owned());
    image.push_str("/");
    image.push_str(constants::HOPR_DOCKER_IMAGE_NAME);
    image.push_str(":");
    image.push_str(&tag.to_owned());
    return image;
}

/// Builds the struct ResourceRequirement from Resource specified in the node
///
/// # Arguments
/// - `resources` - The resources object on the Hoprd record
pub async fn build_resource_requirements(resources: &Option<HoprdResource>) -> Option<ResourceRequirements> {
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

/// Get the value of a Secret
///
/// # Arguments
/// - `api_secret` - The namespaced API for querying Kubernetes
/// - `secret_name` - Hoprd node name
/// - `label_name` - Label name
///
pub async fn get_secret_label(api_secret: &Api<Secret>, secret_name: &str, label_name: &str) -> Option<String> {
    match api_secret.get_opt(&secret_name).await.unwrap() {
        Some(secret) => {
            let emempty_map = &BTreeMap::new();
            let hoprd_labels = secret.metadata.labels.as_ref().unwrap_or_else(|| emempty_map);
            if hoprd_labels.contains_key(label_name) {
                return Some(hoprd_labels.get_key_value(label_name).unwrap().1.parse().unwrap());
            } else {
                println!("The secret {secret_name} does not contain the label {label_name}.");
                None
            }
        }
        None => { 
            println!("The secret {secret_name} does not exist.");
            None }
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