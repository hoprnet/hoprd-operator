use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{
    api::{
        core::v1::ConfigMap,
        networking::v1::{
            HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
            IngressServiceBackend, IngressSpec, IngressTLS, ServiceBackendPort,
        },
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

use crate::model::Error as HoprError;
use crate::{constants, context_data::ContextData, operator_config::IngressConfig, utils};

/// Creates a new Ingress for accessing the hoprd node,
pub async fn create_ingress(context: Arc<ContextData>, service_name: &str, namespace: &str, ingress_config: &IngressConfig, owner_references: Option<Vec<OwnerReference>>) -> Result<Ingress, Error> {
    let labels: Option<BTreeMap<String, String>> = Some(utils::common_lables(
        context.config.instance.name.to_owned(),
        Some(service_name.to_owned()),
        None,
    ));
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
                })
            }]),
            tls: Some(vec![IngressTLS {
                hosts: Some(vec![hostname.to_owned()]),
                secret_name: Some(format!("{}-tls", service_name))
            }]),
            ..IngressSpec::default()
        }),
        ..Ingress::default()
    };

    // Create the Ingress defined above
    let api: Api<Ingress> = Api::namespaced(context.client.clone(), namespace);
    let ingress = api.create(&PostParams::default(), &ingress).await?;
    info!("Ingress {} created successfully", service_name.to_owned());
    Ok(ingress)
}

/// Deletes an existing ingress.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_ingress(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Ingress> = Api::namespaced(client, namespace);
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
pub async fn open_port(
    client: Client,
    service_namespace: &str,
    service_name: &str,
    ingress_config: &IngressConfig,
) -> Result<i32, HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let port: i32 = get_port(client.clone(), ingress_config).await.unwrap();
    let pp = PatchParams::default();
    let patch = Patch::Merge(json!({
       "data": {
            port.to_string().to_owned() : format!("{}/{}:{}", service_namespace, service_name, port.to_owned())
        }
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
    info!("Nginx p2p port {port} for Hoprd node {service_name} opened");
    Ok(port)
}

async fn get_port(client: Client, ingress_config: &IngressConfig) -> Result<i32, HoprError> {
    let api: Api<ConfigMap> = Api::namespaced(client, ingress_config.namespace.as_ref().unwrap());
    if let Some(config_map) = api.get_opt("ingress-nginx-tcp").await? {
        let data = config_map.data.unwrap_or_default();
        let min_port = ingress_config
            .p2p_port_min
            .as_ref()
            .unwrap()
            .parse::<i32>()
            .unwrap_or(constants::OPERATOR_P2P_MIN_PORT.parse::<i32>().unwrap());
        let max_port = ingress_config
            .p2p_port_max
            .as_ref()
            .unwrap()
            .parse::<i32>()
            .unwrap_or(constants::OPERATOR_P2P_MAX_PORT.parse::<i32>().unwrap());
        let ports: Vec<&str> = data
            .keys()
            .filter(|port| port.parse::<i32>().unwrap() >= min_port)
            .filter(|port| port.parse::<i32>().unwrap() <= max_port)
            .map(|x| x.as_str())
            .clone()
            .collect::<Vec<_>>();
        match find_next_port(ports, ingress_config.p2p_port_min.as_ref()).parse::<i32>() {
            Ok(port) => Ok(port),
            Err(error) => {
                error!("Could not parse port number: {:?}", error);
                Err(HoprError::HoprdConfigError("Could not parse port number".to_string()))
            }
        }
    } else {
        Err(HoprError::HoprdConfigError("Could not get new free port".to_string()))
    }
}

/// Find the next port available
fn find_next_port(ports: Vec<&str>, min_port: Option<&String>) -> String {
    if ports.is_empty() {
        return min_port
            .unwrap_or(&constants::OPERATOR_P2P_MIN_PORT.to_owned())
            .to_owned();
    }
    if ports.len() == 1 {
        return (ports[0].parse::<i32>().unwrap() + 1).to_string();
    }
    for i in 1..ports.len() {
        if ports[i].parse::<i32>().unwrap() - ports[i - 1].parse::<i32>().unwrap() > 1 {
            return (ports[i - 1].parse::<i32>().unwrap() + 1).to_string();
        }
    }
    (ports[ports.len() - 1].parse::<i32>().unwrap() + 1).to_string()
}

/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn close_port(
    client: Client,
    service_namespace: &str,
    service_name: &str,
    ingress_config: &IngressConfig,
) -> Result<(), HoprError> {
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
    match api
        .patch(&tcp_config_map.metadata.name.unwrap(), pp, &patch)
        .await
    {
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
    match api
        .patch(&udp_config_map.metadata.name.unwrap(), pp, &patch)
        .await
    {
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

    #[test]
    fn test_find_next_port_empty() {
        let gap_in_middle = vec![];
        assert_eq!(
            find_next_port(gap_in_middle, None),
            constants::OPERATOR_P2P_MIN_PORT.to_string()
        );
    }

    #[test]
    fn test_find_next_port_first() {
        let min_port = constants::OPERATOR_P2P_MIN_PORT.to_string();
        let first_port = vec![min_port.as_str()];
        assert_eq!(
            find_next_port(first_port, None),
            (constants::OPERATOR_P2P_MIN_PORT.parse::<i32>().unwrap() + 1).to_string()
        );
    }

    #[test]
    fn test_find_next_port_gap_in_middle() {
        let gap_in_middle = vec!["9000", "9001", "9003", "9004"];
        assert_eq!(find_next_port(gap_in_middle, None), "9002");
    }

    #[test]
    fn test_find_next_port_last() {
        let last = vec!["9000", "9001", "9002", "9003"];
        assert_eq!(find_next_port(last, None), "9004");
    }
}
