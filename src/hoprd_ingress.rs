use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{api::{networking::v1::{Ingress, IngressSpec, IngressRule, HTTPIngressRuleValue, HTTPIngressPath, IngressBackend, IngressServiceBackend, ServiceBackendPort, IngressTLS}, core::v1::ConfigMap}, apimachinery::pkg::apis::meta::v1::OwnerReference, serde_value::Value};
use kube::{Api, Client, Error, core::ObjectMeta, api::{PostParams, DeleteParams, PatchParams, Patch}, runtime::wait::{conditions, await_condition}};
use serde_json::json;
use std::{collections::{BTreeMap}};


use crate::{utils, operator_config::IngressConfig, constants};
use crate::model::{Error as HoprError};

/// Creates a new Ingress for accessing the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `service_name` - Name of the service which will be exposed externally in the Ingress
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
/// - `ingress` - Ingress Details
///
pub async fn create_ingress(client: Client, service_name: &str, namespace: &str, ingress_config: &IngressConfig, owner_references: Option<Vec<OwnerReference>>) -> Result<Ingress, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&service_name.to_owned());
    let annotations: BTreeMap<String, String> = ingress_config.annotations.as_ref().unwrap_or(&BTreeMap::new()).clone();

    let hostname = format!("{}.{}.{}", service_name, namespace, ingress_config.dns_domain);

    // Definition of the ingress
    let ingress: Ingress = Ingress {
        metadata: ObjectMeta {
            name: Some(service_name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            annotations: Some(annotations),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: Some(ingress_config.ingress_class_name.to_string()),
            rules: Some(vec![IngressRule{
                host: Some(hostname.to_owned()),
                http: Some(HTTPIngressRuleValue{
                    paths: vec![HTTPIngressPath {
                        backend: IngressBackend {
                            service: Some(IngressServiceBackend {
                                name: service_name.to_owned(),
                                port: Some(ServiceBackendPort{
                                    name: Some("api".to_owned()),
                                    ..ServiceBackendPort::default()
                                })
                            }),
                            ..IngressBackend::default()
                        },
                        path_type: "ImplementationSpecific".to_string(),
                        ..HTTPIngressPath::default()
                    }]
                }),
            ..IngressRule::default()
            }]),
            tls: Some(vec![IngressTLS {
                hosts: Some(vec![hostname.to_owned()]),
                secret_name: Some(format!("{}-tls", service_name)),
                ..IngressTLS::default()
            }]),
            ..IngressSpec::default()
        }),
        ..Ingress::default()
    };

    // Create the Ingress defined above
    let api: Api<Ingress> = Api::namespaced(client, namespace);
    api.create(&PostParams::default(), &ingress).await
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
    if let Some(ingress) = api.get_opt(&name).await? {
        let uid = ingress.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, &name.to_owned(), conditions::is_deleted(&uid)).await.unwrap();
        Ok(println!("[INFO] Ingress {name} successfully deleted"))
    } else {
        Ok(println!("[INFO] Ingress {name} in namespace {namespace} about to delete not found"))
    }
}


/// Creates a new Ingress for accessing the hoprd node,
///
pub async fn open_port(client: Client, service_namespace: &str, service_name: &str, ingress_config: &IngressConfig) -> Result<String, HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let port: String = get_port(client.clone(), ingress_config).await.unwrap();
    let pp = PatchParams::default();
    let patch = json!({
           "data": {
                port.to_owned() : format!("{}/{}:{}", service_namespace, service_name, port.to_owned())
            }
        });
    match api.patch("ingress-nginx-tcp", &pp, &Patch::Merge(patch.clone())).await {
            Ok(_) => {},
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not open Nginx tcp port").to_owned()));
            }
    };
    match api.patch("ingress-nginx-udp", &pp, &Patch::Merge(patch.clone())).await {
            Ok(_) => {},
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not open Nginx tcp port").to_owned()));
            }
    };
    Ok(port)
}

async fn get_port(client: Client, ingress_config: &IngressConfig) -> Result<String, HoprError>  {
    let api: Api<ConfigMap> = Api::namespaced(client, &ingress_config.namespace.as_ref().unwrap());
    if let Some(config_map) = api.get_opt("ingress-nginx-tcp").await? {
        let data = config_map.data.unwrap();
        let min_port = ingress_config.p2p_port_min.as_ref().unwrap().parse::<i32>().unwrap();
        let max_port = ingress_config.p2p_port_max.as_ref().unwrap().parse::<i32>().unwrap();
        let ports: Vec<&str> = data.keys()
            .filter(|port| port.parse::<i32>().unwrap() >= min_port )
            .filter(|port| port.parse::<i32>().unwrap() <= max_port )
            .map(|x| x.as_str())
            .clone().collect::<Vec<_>>();
        return Ok(find_next_port(ports, ingress_config.p2p_port_min.as_ref()))
    } else {
        Err(HoprError::HoprdConfigError(format!("Could not get new free port").to_owned()))
    }

}

/// Find the next port available
fn find_next_port(ports: Vec<&str>, min_port: Option<&String>) -> String  {
    if ports.is_empty() {
        return min_port.unwrap_or(&constants::OPERATOR_P2P_MIN_PORT.to_owned()).to_owned();
    }
    if ports.len() == 1 {
        return (ports[0].parse::<i32>().unwrap()+1).to_string()
    }
    for i in 1..ports.len() {
        if ports[i].parse::<i32>().unwrap() - ports[i-1].parse::<i32>().unwrap() > 1 {
            return (ports[i-1].parse::<i32>().unwrap()+1).to_string()
        }
    }
    return (ports[ports.len()-1].parse::<i32>().unwrap()+1).to_string()
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
    let new_data = tcp_config_map.to_owned().data.unwrap().into_iter()
        .filter(|entry| ! entry.1.contains(&service_fqn))
        .collect::<BTreeMap<String, String>>();
    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation{
        path: "/data".to_owned(),
        value: json!(new_data)
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    match api.patch(&tcp_config_map.metadata.name.unwrap(), pp, &patch).await {
            Ok(_) => println!("[INFO] Closed p2p-tcp port on Nginx"),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not close Nginx tcp-port").to_owned()));
            }
    };

    // UDP
    let udp_config_map = api.get("ingress-nginx-udp").await.unwrap();
    let new_data = udp_config_map.to_owned().data.unwrap().into_iter()
        .filter(|entry| ! entry.1.contains(&service_fqn))
        .collect::<BTreeMap<String, String>>();

    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation{
        path: "/data".to_owned(),
        value: json!(new_data)
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    match api.patch(&udp_config_map.metadata.name.unwrap(), pp, &patch).await {
            Ok(_) => println!("[INFO] Closed p2p-udp port on Nginx"),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not close Nginx udp-port").to_owned()));
            }
    };
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_next_port_empty() {
        let gap_in_middle= vec![];
        assert_eq!(find_next_port(gap_in_middle, None), constants::OPERATOR_P2P_MIN_PORT.to_string());
    }

    #[test]
    fn test_find_next_port_first() {
        let min_port = constants::OPERATOR_P2P_MIN_PORT.to_string();
        let first_port= vec![min_port.as_str()];
        assert_eq!(find_next_port(first_port, None), (constants::OPERATOR_P2P_MIN_PORT.parse::<i32>().unwrap() + 1).to_string());
    }

    #[test]
    fn test_find_next_port_gap_in_middle() {
        let gap_in_middle= vec!["9000","9001","9003","9004"];
        assert_eq!(find_next_port(gap_in_middle, None), "9002");
    }

    #[test]
    fn test_find_next_port_last() {
        let last= vec!["9000","9001","9002","9003"];
        assert_eq!(find_next_port(last, None), "9004");
    }


}