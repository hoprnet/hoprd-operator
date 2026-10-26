use crate::{model::Error as HoprdError, operator_config::IngressConfig};
use k8s_openapi::{
    api::core::v1::{Service, ServicePort, ServiceSpec},
    apimachinery::pkg::{apis::meta::v1::OwnerReference, util::intstr::IntOrString},
};
use kube::{
    api::{PostParams},
    core::ObjectMeta,
    Api, Client,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    sync::Arc,
};
use tracing::info;

use crate::{constants, context_data::ContextData, utils};

#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdServiceSpec {
    pub r#type: ServiceTypeEnum,
    pub ports_allocation: u16,
}

impl Default for HoprdServiceSpec {
    fn default() -> Self {
        Self { 
            r#type: ServiceTypeEnum::ClusterIP,
            ports_allocation: 4,
        }
    }
}

#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
pub enum ServiceTypeEnum {
    // The hoprd service is of type ClusterIP
    ClusterIP,
    /// The hoprd service is of type LoadBalancer
    LoadBalancer,
}

impl Display for ServiceTypeEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            ServiceTypeEnum::ClusterIP => write!(f, "ClusterIP"),
            ServiceTypeEnum::LoadBalancer => write!(f, "LoadBalancer"),
        }
    }
}

/// Creates a new service for accessing the hoprd node,
pub async fn create_service(
    context_data: Arc<ContextData>,
    name: &str,
    namespace: &str,
    ingress_config: &IngressConfig,
    identity_pool_name: &str,
    service_type: ServiceTypeEnum,
    starting_port: u16,
    last_port: u16,
    owner_references: Option<Vec<OwnerReference>>,
) -> Result<String, HoprdError> {
    let mut labels: BTreeMap<String, String> = utils::common_lables(identity_pool_name.to_owned(), Some(name.to_owned()), None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool_name.to_owned());

    create_single_port_cluster_ip_service(context_data.clone(), &format!("{}-metrics", name), namespace, "metrics", 8080, labels.clone(), owner_references.clone()).await?;
    info!("Metrics Service {} created successfully", name.to_owned());

    create_single_port_cluster_ip_service(context_data.clone(), &format!("{}-api", name), namespace, "api", 3001, labels.clone(), owner_references.clone()).await?;
    info!("Api Service {} created successfully", name.to_owned());

    if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        create_cluster_ip_service(context_data.clone(), name, namespace, labels, owner_references, starting_port, last_port).await?;
        info!("ClusterIP Service {} created successfully", name.to_owned());
        Ok(context_data.config.ingress.loadbalancer_ip.to_string())
    } else {
        let public_ip = create_load_balancer_service(context_data.clone(), name, namespace, ingress_config, labels, owner_references, starting_port, last_port).await?;
        info!("LoadBalancer Service {} created successfully", name.to_owned());
        Ok(public_ip)
    }
}

/// Creates a ClusterIP-only Service exposing a single named container port, kept separate from
/// the p2p Service(s) so it is never reachable through the node's external LoadBalancer IP.
async fn create_single_port_cluster_ip_service(
    context_data: Arc<ContextData>,
    service_name: &str,
    namespace: &str,
    port_name: &str,
    port: i32,
    labels: BTreeMap<String, String>,
    owner_references: Option<Vec<OwnerReference>>,
) -> Result<(), HoprdError> {
    let service: Service = Service {
        metadata: ObjectMeta {
            name: Some(service_name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            type_: Some("ClusterIP".to_owned()),
            ports: Some(vec![ServicePort {
                name: Some(port_name.to_owned()),
                port,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::String(port_name.to_owned())),
                ..ServicePort::default()
            }]),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    let api_service: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    api_service.create(&PostParams::default(), &service).await?;
    Ok(())
}

async fn create_load_balancer_service(
    context_data: Arc<ContextData>,
    name: &str,
    namespace: &str,
    ingress_config: &IngressConfig,
    labels: BTreeMap<String, String>,
    owner_references: Option<Vec<OwnerReference>>,
    starting_port: u16,
    last_port: u16,
) -> Result<String, HoprdError> {
    let api_service: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    let hostname = format!("{}-p2p.{}.{}", name.to_owned(), namespace, ingress_config.dns_domain);
    let mut annotations: BTreeMap<String, String> = BTreeMap::new();
    annotations.insert(constants::ANNOTATION_EXTERNAL_DNS_HOSTNAME.to_owned(), hostname.to_owned());

    let service_tcp: Service = Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-tcp", name.to_owned())),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references: owner_references.clone(),
            annotations: Some(annotations.clone()),
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels.clone()),
            type_: Some("LoadBalancer".to_owned()),
            allocate_load_balancer_node_ports: Some(false),
            ports: Some(build_ports(starting_port, last_port, Some("tcp"))),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    api_service.create(&PostParams::default(), &service_tcp).await?;
    let mut load_balancer_ip = None;
    let mut retries = 0;
    let max_retries = 24; // e.g., 24 retries with 10-second intervals
    while load_balancer_ip.is_none() && retries < max_retries {
        // Wait for a short period before checking the service status again
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        retries += 1;

        // Fetch the latest version of the service
        let service = api_service.get(&format!("{}-tcp", name.to_owned())).await?;

        // Try to get the IP address from the service status
        load_balancer_ip = service
            .status
            .clone()
            .and_then(|s| s.load_balancer)
            .and_then(|lb| lb.ingress.clone())
            .and_then(|ingress| ingress.get(0).cloned())
            .and_then(|first_ingress| first_ingress.ip.clone());
    }

    let Some(load_balancer_ip) = load_balancer_ip else {
        return Err(HoprdError::HoprdStatusError("Failed to obtain load balancer IP within the expected time frame".to_string()));
    };

    let service_udp: Service = Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-udp", name.to_owned())),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            load_balancer_ip: Some(load_balancer_ip.clone()),
            selector: Some(labels.clone()),
            type_: Some("LoadBalancer".to_owned()),
            ports: Some(build_ports(starting_port, last_port, Some("udp"))),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };
    api_service.create(&PostParams::default(), &service_udp).await?;
    Ok(load_balancer_ip)
}

async fn create_cluster_ip_service(
    context_data: Arc<ContextData>,
    name: &str,
    namespace: &str,
    labels: BTreeMap<String, String>,
    owner_references: Option<Vec<OwnerReference>>,
    starting_port: u16,
    last_port: u16,
) -> Result<Service, HoprdError> {
    // Definition of the service. Alternatively, a YAML representation could be used as well.
    let service: Service = Service {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels.clone()),
            type_: Some("ClusterIP".to_owned()),
            ports: Some(build_ports(starting_port, last_port, None)),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    // Create the service defined above
    let service_api: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    let service = service_api.create(&PostParams::default(), &service).await?;
    Ok(service)
}

fn build_ports(starting_port: u16, last_port: u16, port_name: Option<&str>) -> Vec<ServicePort> {
    let mut ports = Vec::new();
    let protocols = if let Some(name) = port_name {
        // If a specific protocol is provided
        let protocol = if name.contains("udp") { "UDP" } else { "TCP" };
        vec![protocol.to_owned()]
    } else {
        // If no specific protocol, include both TCP and UDP
        vec!["TCP".to_owned(), "UDP".to_owned()]
    };

    for protocol in protocols {
        ports.push(ServicePort {
            name: Some(format!("p2p-{}", protocol.to_lowercase())),
            port: starting_port.into(),
            protocol: Some(protocol.clone()),
            target_port: Some(IntOrString::Int(starting_port.into())),
            ..ServicePort::default()
        });
        for port_number in starting_port + 1..last_port {
            ports.push(ServicePort {
                name: Some(format!("session{}-{}", protocol.chars().next().map(|c| c.to_lowercase().to_string()).unwrap_or_default(), port_number)),
                port: port_number.into(),
                protocol: Some(protocol.clone()),
                target_port: Some(IntOrString::Int(port_number.into())),
                ..ServicePort::default()
            });
        }
    }
    ports
}

/// Deletes an existing service.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_service(client: Client, name: &str, namespace: &str, service_type: &ServiceTypeEnum) -> Result<(), HoprdError> {
    let api: Api<Service> = Api::namespaced(client, namespace);
    utils::delete_and_wait(api.clone(), name).await?;
    utils::delete_and_wait(api.clone(), &format!("{}-metrics", name)).await?;
    utils::delete_and_wait(api.clone(), &format!("{}-api", name)).await?;

    if service_type.eq(&ServiceTypeEnum::LoadBalancer) {
        utils::delete_and_wait(api.clone(), &format!("{}-tcp", name)).await?;
        utils::delete_and_wait(api.clone(), &format!("{}-udp", name)).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ports_for_a_single_protocol() {
        let ports = build_ports(9000, 9003, Some("tcp"));
        let numbers: Vec<i32> = ports.iter().map(|port| port.port).collect();
        assert_eq!(numbers, vec![9000, 9001, 9002]);
        assert!(ports.iter().all(|port| port.protocol.as_deref() == Some("TCP")));
        assert_eq!(ports[0].name.as_deref(), Some("p2p-tcp"));
        assert_eq!(ports[1].name.as_deref(), Some("sessiont-9001"));
    }

    #[test]
    fn build_ports_selects_udp_by_name() {
        let ports = build_ports(9000, 9002, Some("udp"));
        assert!(ports.iter().all(|port| port.protocol.as_deref() == Some("UDP")));
        assert_eq!(ports[0].name.as_deref(), Some("p2p-udp"));
    }

    #[test]
    fn build_ports_without_name_includes_both_protocols() {
        let ports = build_ports(9000, 9002, None);
        assert_eq!(ports.len(), 4);
        assert_eq!(ports.iter().filter(|port| port.protocol.as_deref() == Some("TCP")).count(), 2);
        assert_eq!(ports.iter().filter(|port| port.protocol.as_deref() == Some("UDP")).count(), 2);
    }

    #[test]
    fn build_ports_with_an_empty_range_only_has_the_p2p_port() {
        assert_eq!(build_ports(9000, 9000, Some("tcp")).len(), 1);
    }

    #[test]
    fn service_defaults_to_cluster_ip() {
        let spec = HoprdServiceSpec::default();
        assert_eq!(spec.r#type, ServiceTypeEnum::ClusterIP);
        assert_eq!(spec.ports_allocation, 4);
    }
}
