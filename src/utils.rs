use crate::{cluster::ClusterHoprd, constants, hoprd::Hoprd, identity_pool::IdentityPool};
use kube::{Api, Client};
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

pub fn common_lables(
    name: String,
    instance: Option<String>,
    component: Option<String>,
) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), name);
    match instance {
        Some(instance) => {
            labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance);
        }
        None => {}
    }
    match component {
        Some(component) => {
            labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), component);
        }
        None => {}
    }
    return labels;
}

pub enum ResourceType {
    IdentityPool,
    Hoprd,
    ClusterHoprd,
}

impl Display for ResourceType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ResourceType::IdentityPool => write!(f, "IdentityPool"),
            ResourceType::Hoprd => write!(f, "Hoprd"),
            ResourceType::ClusterHoprd => write!(f, "ClusterHoprd"),
        }
    }
}
#[derive(PartialEq, Clone)]
pub enum ResourceKind {
    Labels,
    Annotations,
}

impl Display for ResourceKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ResourceKind::Labels => write!(f, "Labels"),
            ResourceKind::Annotations => write!(f, "Annotations"),
        }
    }
}

pub async fn get_resource_kinds(
    client: Client,
    resource_type: ResourceType,
    resource_kind: ResourceKind,
    resource_name: &str,
    resource_namespace: &str,
) -> BTreeMap<String, String> {
    let empty_map: &BTreeMap<String, String> = &BTreeMap::new();
    match resource_type {
        ResourceType::IdentityPool => { 
            let api_identity: Api<IdentityPool> = Api::namespaced(client.clone(), &resource_namespace);
            match api_identity.get_opt(&resource_name).await.unwrap() {
                Some(identity) => { 
                    if resource_kind.eq(&ResourceKind::Labels) {
                        identity.metadata.labels.as_ref().unwrap_or_else(|| empty_map).clone()
                    } else {
                        identity.metadata.annotations.as_ref().unwrap_or_else(|| empty_map).clone()
                    }
                }
                None => {
                    println!("The identity pool {resource_name} does not exist.");
                    empty_map.clone()
                }
            }
        },
        ResourceType::Hoprd => {
            let api_hoprd: Api<Hoprd> = Api::namespaced(client.clone(), &resource_namespace);
            match api_hoprd.get_opt(&resource_name).await.unwrap() {
                Some(hoprd) => {
                    if resource_kind.eq(&ResourceKind::Labels) {
                        hoprd
                            .metadata
                            .labels
                            .as_ref()
                            .unwrap_or_else(|| empty_map)
                            .clone()
                    } else {
                        hoprd
                            .metadata
                            .annotations
                            .as_ref()
                            .unwrap_or_else(|| empty_map)
                            .clone()
                    }
                }
                None => {
                    println!("The hoprd {resource_name} does not exist.");
                    empty_map.clone()
                }
            }
        }
        ResourceType::ClusterHoprd => {
            let api_cluster_hoprd: Api<ClusterHoprd> =
                Api::namespaced(client.clone(), &resource_namespace);
            match api_cluster_hoprd.get_opt(&resource_name).await.unwrap() {
                Some(hoprd) => {
                    if resource_kind.eq(&ResourceKind::Labels) {
                        hoprd
                            .metadata
                            .labels
                            .as_ref()
                            .unwrap_or_else(|| empty_map)
                            .clone()
                    } else {
                        hoprd
                            .metadata
                            .annotations
                            .as_ref()
                            .unwrap_or_else(|| empty_map)
                            .clone()
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
