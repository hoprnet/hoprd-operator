use json_patch::{PatchOperation, ReplaceOperation};
use json_patch::jsonptr::PointerBuf;
use k8s_openapi::{
    api::{
        core::v1::ConfigMap,
        networking::v1::{HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule, IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort},
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference
};
use kube::{
    api::{Patch, PatchParams, PostParams},
    core::ObjectMeta,
    Api, Client,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tracing::{debug, error, info};

use crate::{context_data::ContextData, operator_config::IngressConfig, utils};
use crate::{hoprd::hoprd_ingress, model::Error as HoprError};

use super::hoprd_service::ServiceTypeEnum;

/// Creates a new Ingress for accessing the hoprd node,
pub async fn create_ingress(
    context_data: Arc<ContextData>,
    service_type: &ServiceTypeEnum,
    service_name: &str,
    identity_pool_name: &str,
    namespace: &str,
    dns_name: &str,
    session_port_allocation: u16,
    ingress_config: &IngressConfig,
    owner_references: Option<Vec<OwnerReference>>,
) -> Result<u16, HoprError> {
    let labels: Option<BTreeMap<String, String>> = Some(utils::common_lables(identity_pool_name.to_owned(), Some(service_name.to_owned()), None));
    let stating_port = if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        hoprd_ingress::open_port(context_data.client.clone(), &namespace, &service_name, session_port_allocation, &context_data.config.ingress)
            .await?
    } else {
        9091
    };
    let backend_service_name = format!("{}-api", service_name);
    let annotations: BTreeMap<String, String> = ingress_config.annotations.as_ref().unwrap_or(&BTreeMap::new()).clone();

    // Try to strip the longest DNS-derived prefix from the service name to keep hostnames short.
    // Example: dns=piz-palu.dev.hoprnet.link, service=piz-palu-dev-node-1 -> node-1
    let dns_labels: Vec<&str> = dns_name.split('.').collect();
    let mut shortened_service_name = service_name.to_owned();
    for i in (1..=dns_labels.len()).rev() { // Iterate over the DNS labels in reverse order to find the longest matching prefix
        let candidate_prefix = dns_labels[..i].join("-"); // Join the first i DNS labels with a dash to form the candidate prefix
        let prefix_with_dash = format!("{}-", candidate_prefix);
        if shortened_service_name.starts_with(&prefix_with_dash) {
            shortened_service_name = shortened_service_name[prefix_with_dash.len()..].to_owned();
            break;
        }
    }

    let hostname = format!("{}.{}.{}", shortened_service_name, namespace, dns_name);

    // Definition of the ingress
    let ingress: Ingress = Ingress {
        metadata: ObjectMeta {
            name: Some(service_name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels,
            annotations: Some(annotations),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: Some(ingress_config.ingress_class_name.to_string()),
            rules: Some(vec![IngressRule {
                host: Some(hostname.to_owned()),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        backend: IngressBackend {
                            service: Some(IngressServiceBackend {
                                name: backend_service_name.to_owned(),
                                port: Some(ServiceBackendPort {
                                    name: Some("api".to_owned()),
                                    ..ServiceBackendPort::default()
                                }),
                            }),
                            ..IngressBackend::default()
                        },
                        path_type: "ImplementationSpecific".to_string(),
                        ..HTTPIngressPath::default()
                    }],
                }),
            }]),
            tls: Some(vec![IngressTLS {
                hosts: Some(vec![hostname.to_owned()]),
                secret_name: Some(format!("{}-tls", service_name)),
            }]),
            ..IngressSpec::default()
        }),
        ..Ingress::default()
    };

    // Create the Ingress defined above
    let api: Api<Ingress> = Api::namespaced(context_data.client.clone(), namespace);
    api.create(&PostParams::default(), &ingress).await?;
    info!("Ingress {} created successfully", service_name.to_owned());
    Ok(stating_port)
}

/// Deletes an existing ingress.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_ingress(context_data: Arc<ContextData>, name: &str, namespace: &str, service_type: &ServiceTypeEnum) -> Result<(), HoprError> {
    if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        hoprd_ingress::close_port(context_data.client.clone(), &namespace, &name, &context_data.config.ingress).await?;
    }
    let api: Api<Ingress> = Api::namespaced(context_data.client.clone(), namespace);
    crate::utils::delete_and_wait(api, name).await?;
    Ok(())
}

/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn open_port(client: Client, service_namespace: &str, service_name: &str, session_port_allocation: u16, ingress_config: &IngressConfig) -> Result<u16, HoprError> {
    let namespace = ingress_config.namespace()?;
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let starting_port: u16 = get_available_ports(client.clone(), session_port_allocation, ingress_config).await?;
    // Create a BTreeMap to hold the new data entries
    let mut new_ports = BTreeMap::new();
    if starting_port + session_port_allocation > ingress_config.port_max {
        return Err(HoprError::HoprdConfigError(format!(
            "Cannot allocate {} ports starting from {}. Would exceed max_port {}",
            session_port_allocation, starting_port, ingress_config.port_max
        )));
    }
    if session_port_allocation == 0 {
        debug!("No ports to allocate, skipping opening nginx ports");
        return Ok(starting_port);
    }
    // Iterate over the session_port_allocation and insert entries starting from starting_port
    for i in 0..session_port_allocation.to_owned() {
        let current_port = starting_port + i;
        new_ports.insert(current_port.to_string(), format!("{}/{}:{}", service_namespace, service_name, current_port));
    }
    let patch = Patch::Merge(json!({
       "data": new_ports.clone()
    }));
    open_in_config_map(&api, "tcp", &patch).await?;
    open_in_config_map(&api, "udp", &patch).await?;
    info!("{session_port_allocation} nginx ports starting from {starting_port} opened for Hoprd node {service_name}");
    Ok(starting_port)
}

/// Adds the port entries to the `ingress-nginx-<protocol>` ConfigMap, if it exists
async fn open_in_config_map(api: &Api<ConfigMap>, protocol: &str, patch: &Patch<Value>) -> Result<(), HoprError> {
    let config_map_name = format!("ingress-nginx-{protocol}");
    if api.get_opt(&config_map_name).await?.is_some() {
        if let Err(error) = api.patch(&config_map_name, &PatchParams::default(), patch).await {
            error!("Could not open Nginx {protocol} port: {:?}", error);
            return Err(HoprError::HoprdConfigError(format!("Could not open Nginx {protocol} port")));
        }
    } else {
        debug!("ConfigMap {config_map_name} not found, skipping Nginx {protocol} port opening");
    }
    Ok(())
}

/// Removes the entries of the service from the `ingress-nginx-<protocol>` ConfigMap, if it exists
async fn close_in_config_map(api: &Api<ConfigMap>, protocol: &str, service_fqn: &str) -> Result<(), HoprError> {
    let config_map_name = format!("ingress-nginx-{protocol}");
    let config_map = match api.get_opt(&config_map_name).await {
        Ok(Some(config_map)) => config_map,
        Ok(None) => {
            debug!("ConfigMap {config_map_name} not found, nothing to close");
            return Ok(());
        }
        Err(error) => {
            error!("Could not get ConfigMap {config_map_name}: {:?}", error);
            return Err(HoprError::HoprdConfigError(format!("Could not get Nginx {protocol} ConfigMap")));
        }
    };
    let new_data = config_map
        .data
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| !entry.1.contains(service_fqn))
        .collect::<BTreeMap<String, String>>();
    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation {
        path: PointerBuf::parse("/data".to_owned()).map_err(|e| HoprError::HoprdConfigError(format!("Invalid JSON pointer: {e}")))?,
        value: json!(new_data),
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    if let Err(error) = api.patch(&config_map_name, &PatchParams::default(), &patch).await {
        error!("Could not close Nginx {protocol}-port: {:?}", error);
        return Err(HoprError::HoprdConfigError(format!("Could not close Nginx {protocol}-port")));
    }
    Ok(())
}

async fn get_available_ports(client: Client, session_port_allocation: u16, ingress_config: &IngressConfig) -> Result<u16, HoprError> {
    let api: Api<ConfigMap> = Api::namespaced(client, ingress_config.namespace()?);
    if let Some(config_map) = api.get_opt("ingress-nginx-tcp").await? {
        let data = config_map.data.unwrap_or_default();
        let mut ports: Vec<u16> = data
            .keys()
            .filter_map(|port| port.parse::<u16>().ok())
            .filter(|port| port >= &ingress_config.port_min)
            .filter(|port| port <= &ingress_config.port_max)
            .clone()
            .collect::<Vec<_>>();
        ports.sort();
        return Ok(find_next_port(ports, session_port_allocation, ingress_config.port_min));
    } else {
        debug!("ConfigMap ingress-nginx-tcp not found, using port_min as next free port");
        Ok(ingress_config.port_min)
    }
}

/// Find the next port available
fn find_next_port(ports: Vec<u16>, session_port_allocation: u16, min_port: u16) -> u16 {
    if ports.is_empty() {
        return min_port;
    }

    // If the first port used is greater than the min_port plus session_port_allocation, fill the gap
    if (ports[0] - min_port) >= session_port_allocation {
        if ports[0] > min_port { // Ensure there is a gap in case the session_port_allocation is 0
            return ports[0] - session_port_allocation;
        }
    }

    // Find a gap in the ports vector where the values between two consecutive ports are greater than the session_port_allocation
    for i in 1..ports.len() {
        if ports[i] - ports[i - 1] - 1 >= session_port_allocation && session_port_allocation > 0 {
            return ports[i - 1] + 1;
        }
    }
    // If no gap is found, return the last port + 1
    return ports[ports.len() - 1] + 1;
}

/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn close_port(client: Client, service_namespace: &str, service_name: &str, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace()?;
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let service_fqn = format!("{}/{}", service_namespace, service_name);
    close_in_config_map(&api, "tcp", &service_fqn).await?;
    close_in_config_map(&api, "udp", &service_fqn).await?;
    info!("Nginx p2p port for Hoprd node {service_name} have been closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const OPERATOR_MIN_PORT: u16 = 9000;

    #[test]
    fn test_find_next_port_empty() {
        let gap_in_middle = vec![];
        assert_eq!(find_next_port(gap_in_middle, 10, OPERATOR_MIN_PORT), OPERATOR_MIN_PORT);
    }

    #[test]
    fn test_find_next_port_first() {
        let first_port = vec![9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009];
        assert_eq!(find_next_port(first_port, 10, OPERATOR_MIN_PORT), 9010);
    }

    #[test]
    fn test_find_next_port_gap_in_middle() {
        let gap_in_middle = vec![9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009, 9020, 9021, 9022, 9023, 9024, 9025, 9026, 9027, 9028, 9029];
        assert_eq!(find_next_port(gap_in_middle, 10, OPERATOR_MIN_PORT), 9010);
    }

    #[test]
    fn test_find_next_port_last() {
        let last = vec![9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009, 9010, 9011, 9012, 9013, 9014, 9015, 9016, 9017, 9018, 9019];
        assert_eq!(find_next_port(last, 10, OPERATOR_MIN_PORT), 9020);
    }
}
