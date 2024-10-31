use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{
    api::{
        core::v1::ConfigMap,
        networking::v1::{HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule, IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort},
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference,
    serde_value::Value,
};
use kube::{
    api::{DeleteParams, Patch, PatchParams, PostParams},
    core::ObjectMeta,
    runtime::wait::{await_condition, conditions},
    Api, Client, Error,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tracing::{error, info};

use crate::{context_data::ContextData, operator_config::IngressConfig, utils};
use crate::{hoprd::hoprd_ingress, model::Error as HoprError};

use super::hoprd_service::ServiceTypeEnum;

/// Creates a new Ingress for accessing the hoprd node,
pub async fn create_ingress(
    context_data: Arc<ContextData>,
    service_type: &ServiceTypeEnum,
    service_name: &str,
    namespace: &str,
    session_port_allocation: u16,
    ingress_config: &IngressConfig,
    owner_references: Option<Vec<OwnerReference>>,
) -> Result<u16, Error> {
    let labels: Option<BTreeMap<String, String>> = Some(utils::common_lables(context_data.config.instance.name.to_owned(), Some(service_name.to_owned()), None));
    let stating_port = if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        hoprd_ingress::open_port(context_data.client.clone(), &namespace, &service_name, session_port_allocation, &context_data.config.ingress)
            .await
            .unwrap()
    } else {
        9091
    };
    let annotations: BTreeMap<String, String> = ingress_config.annotations.as_ref().unwrap_or(&BTreeMap::new()).clone();

    let hostname = format!("{}.{}.{}", service_name, namespace, ingress_config.dns_domain);

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
                                name: service_name.to_owned(),
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
pub async fn delete_ingress(context_data: Arc<ContextData>, name: &str, namespace: &str, service_type: &ServiceTypeEnum) -> Result<(), Error> {
    if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        hoprd_ingress::close_port(context_data.client.clone(), &namespace, &name, &context_data.config.ingress).await.unwrap();
    }
    let api: Api<Ingress> = Api::namespaced(context_data.client.clone(), namespace);
    if let Some(ingress) = api.get_opt(name).await? {
        let uid = ingress.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("Ingress {name} successfully deleted"))
    } else {
        Ok(info!("Ingress {name} in namespace {namespace} about to delete not found"))
    }
}

/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn open_port(client: Client, service_namespace: &str, service_name: &str, session_port_allocation: u16, ingress_config: &IngressConfig) -> Result<u16, HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let starting_port: u16 = get_available_ports(client.clone(), session_port_allocation, ingress_config).await?;
    let pp = PatchParams::default();

    // Create a BTreeMap to hold the new data entries
    let mut new_ports = BTreeMap::new();
    if starting_port + session_port_allocation > ingress_config.port_max {
        return Err(HoprError::HoprdConfigError(format!(
            "Cannot allocate {} ports starting from {}. Would exceed max_port {}",
            session_port_allocation, starting_port, ingress_config.port_max
        )));
    }
    // Iterate over the session_port_allocation and insert entries starting from starting_port
    for i in 0..session_port_allocation.to_owned() {
        let current_port = starting_port + i;
        new_ports.insert(current_port.to_string(), format!("{}/{}:{}", service_namespace, service_name, current_port));
    }
    let patch = Patch::Merge(json!({
       "data": new_ports.clone()
    }));
    match api.patch("ingress-nginx-tcp", &pp, &patch.clone()).await {
        Ok(_) => {}
        Err(error) => {
            error!("Could not open Nginx tcp port: {:?}", error);
            return Err(HoprError::HoprdConfigError("Could not open Nginx tcp port".to_string()));
        }
    };
    match api.patch("ingress-nginx-udp", &pp, &patch.clone()).await {
        Ok(_) => {}
        Err(error) => {
            error!("Could not open Nginx udp port: {:?}", error);
            return Err(HoprError::HoprdConfigError("Could not open Nginx udp port".to_string()));
        }
    };
    info!("{session_port_allocation} nginx ports starting from {starting_port} opened for Hoprd node {service_name}");
    Ok(starting_port)
}

async fn get_available_ports(client: Client, session_port_allocation: u16, ingress_config: &IngressConfig) -> Result<u16, HoprError> {
    let api: Api<ConfigMap> = Api::namespaced(client, ingress_config.namespace.as_ref().unwrap());
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
        Err(HoprError::HoprdConfigError("Could not get new free port".to_string()))
    }
}

/// Find the next port available
fn find_next_port(ports: Vec<u16>, session_port_allocation: u16, min_port: u16) -> u16 {
    if ports.is_empty() {
        return min_port;
    }

    // If the first port used is greater than the min_port plus session_port_allocation, fill the gap
    if (ports[0] - min_port) >= session_port_allocation {
        return ports[0] - session_port_allocation;
    }

    // Find a gap in the ports vector where the values between two consecutive ports are greater than the session_port_allocation
    for i in 1..ports.len() {
        if ports[i] - ports[i - 1] >= session_port_allocation {
            return ports[i - 1] + 1;
        }
    }
    // If no gap is found, return the last port + 1
    return ports[ports.len() - 1] + 1;
}

/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn close_port(client: Client, service_namespace: &str, service_name: &str, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let service_fqn = format!("{}/{}", service_namespace, service_name);
    let pp = &PatchParams::default();

    // TCP
    let tcp_config_map = api.get("ingress-nginx-tcp").await.unwrap();
    let new_data = tcp_config_map
        .to_owned()
        .data
        .unwrap_or(BTreeMap::new())
        .into_iter()
        .filter(|entry| !entry.1.contains(&service_fqn))
        .collect::<BTreeMap<String, String>>();
    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation {
        path: "/data".to_owned(),
        value: json!(new_data),
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    match api.patch(&tcp_config_map.metadata.name.unwrap(), pp, &patch).await {
        Ok(_) => {}
        Err(error) => {
            error!("Could not close Nginx tcp-port: {:?}", error);
            return Err(HoprError::HoprdConfigError("Could not close Nginx tcp-port".to_string()));
        }
    };

    // UDP
    let udp_config_map = api.get("ingress-nginx-udp").await.unwrap();
    let new_data = udp_config_map
        .to_owned()
        .data
        .unwrap_or(BTreeMap::new())
        .into_iter()
        .filter(|entry| !entry.1.contains(&service_fqn))
        .collect::<BTreeMap<String, String>>();

    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation {
        path: "/data".to_owned(),
        value: json!(new_data),
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    match api.patch(&udp_config_map.metadata.name.unwrap(), pp, &patch).await {
        Ok(_) => {}
        Err(error) => {
            error!("Could not close Nginx udp-port: {:?}", error);
            return Err(HoprError::HoprdConfigError("Could not close Nginx udp-port".to_string()));
        }
    };
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
