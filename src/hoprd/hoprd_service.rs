use crate::model::Error as HoprdError;
use k8s_openapi::{
    api::core::v1::{Service, ServicePort, ServiceSpec},
    apimachinery::pkg::{apis::meta::v1::OwnerReference, util::intstr::IntOrString},
};
use kube::{core::ObjectMeta, Api, Client};
use std::{collections::BTreeMap, sync::Arc};
use tracing::info;

use crate::{constants, context_data::ContextData, utils};

/// Name of the Service that exposes the p2p ports of a node
pub fn p2p_service_name(name: &str) -> String {
    format!("{name}-p2p")
}

/// Name of the Service that exposes the API of a node
pub fn api_service_name(name: &str) -> String {
    format!("{name}-api")
}

/// Creates the Services of a hoprd node: the one that exposes its API and the one with its p2p port.
/// All of them are `ClusterIP`: the node is reached from outside through the routes of the Gateway.
/// The creation is idempotent, so a repeated reconciliation converges to the same state.
pub async fn create_service(
    context_data: Arc<ContextData>,
    name: &str,
    namespace: &str,
    identity_pool_name: &str,
    p2p_port: u16,
    owner_references: Option<Vec<OwnerReference>>,
) -> Result<(), HoprdError> {
    let mut labels: BTreeMap<String, String> = utils::common_lables(identity_pool_name.to_owned(), Some(name.to_owned()), None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool_name.to_owned());

    apply_service(
        context_data.client.clone(),
        &api_service_name(name),
        namespace,
        labels.clone(),
        owner_references.clone(),
        vec![ServicePort {
            name: Some("api".to_owned()),
            port: constants::API_PORT,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::String("api".to_owned())),
            ..ServicePort::default()
        }],
    )
    .await?;
    info!("Api Service {} created successfully", name.to_owned());

    apply_service(context_data.client.clone(), &p2p_service_name(name), namespace, labels, owner_references, build_ports(p2p_port)).await?;
    info!("Service {} created successfully", name.to_owned());
    Ok(())
}

async fn apply_service(client: Client, service_name: &str, namespace: &str, labels: BTreeMap<String, String>, owner_references: Option<Vec<OwnerReference>>, ports: Vec<ServicePort>) -> Result<(), HoprdError> {
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
            ports: Some(ports),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };
    let api: Api<Service> = Api::namespaced(client, namespace);
    utils::apply(&api, service_name, &service).await?;
    Ok(())
}

/// Ports of the p2p Service: the p2p port for TCP and for UDP
fn build_ports(p2p_port: u16) -> Vec<ServicePort> {
    ["TCP", "UDP"]
        .into_iter()
        .map(|protocol| ServicePort {
            name: Some(format!("p2p-{}", protocol.to_lowercase())),
            port: p2p_port.into(),
            protocol: Some(protocol.to_owned()),
            target_port: Some(IntOrString::Int(p2p_port.into())),
            ..ServicePort::default()
        })
        .collect()
}

/// Deletes the Services of a node.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_service(client: Client, name: &str, namespace: &str) -> Result<(), HoprdError> {
    let api: Api<Service> = Api::namespaced(client, namespace);
    utils::delete_and_wait(api.clone(), &p2p_service_name(name)).await?;
    utils::delete_and_wait(api.clone(), &api_service_name(name)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ports_lists_the_p2p_port_for_both_protocols() {
        let ports = build_ports(9000);
        let named: Vec<(String, i32, String)> = ports.iter().map(|port| (port.name.clone().unwrap_or_default(), port.port, port.protocol.clone().unwrap_or_default())).collect();
        assert_eq!(named, vec![("p2p-tcp".to_owned(), 9000, "TCP".to_owned()), ("p2p-udp".to_owned(), 9000, "UDP".to_owned())]);
    }
}
