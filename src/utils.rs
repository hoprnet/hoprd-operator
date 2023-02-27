use std::collections::BTreeMap;
use k8s_openapi::{api::core::v1::{ResourceRequirements, Secret}, apimachinery::pkg::api::resource::Quantity};
use kube::{Api, api::Patch};
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
/// - `name` - Hoprd node name
/// - `label_name` - Label name
///
pub async fn get_secret_label(api_secret: &Api<Secret>, name: &str, label_name: &str) -> Option<String> {
    match api_secret.get_opt(&name).await.unwrap() {
        Some(secret) => {
            let empty_annotations = &BTreeMap::new();
            let hoprd_labels: &BTreeMap<String, String> = secret.metadata.labels.as_ref().unwrap_or_else(|| empty_annotations);
            if hoprd_labels.contains_key(label_name) {
                return Some(hoprd_labels.get_key_value(label_name).unwrap().1.parse().unwrap());
            } else {
                println!("The secret {name} does not contain the label {label_name}.");
                None
            }
        }
        None => { 
            println!("The secret {name} does not exist.");
            None }
    }
}

pub async fn update_secret_annotations(api: &Api<Secret>, resource_name: &str, annotation_name: &str, annotation_value: &str) -> Result<(), Error> {
    let annotations_added: Value = json!({
        "metadata": {
            "annotations":  
                { annotation_name : annotation_value }
        }
    });
    if let Some(_secret) = api.get_opt(&resource_name).await? {
            let patch: Patch<&Value> = Patch::Merge(&annotations_added);
            api.patch(&resource_name, &kube::api::PatchParams::default(), &patch).await?;
    }
    Ok(())
}

pub async fn update_secret_label(api: &Api<Secret>, resource_name: &str, label_name: &str, label_value: &str) -> Result<Secret, Error> {
    let annotations_added: Value = json!({
        "metadata": {
            "labels": 
                { label_name : label_value }
        }
    });
    let patch: Patch<&Value> = Patch::Merge(&annotations_added);
    Ok(api.patch(&resource_name, &kube::api::PatchParams::default(), &patch).await?)
}