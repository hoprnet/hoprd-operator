use crate::model::Error as HoprError;
use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        core::v1::{ContainerPort, Service, ServicePort},
    },
    apimachinery::pkg::util::intstr::IntOrString,
    serde_value::Value,
};
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, Result,
};
use serde_json::json;
use std::sync::Arc;
use tracing::error;

use crate::{context_data::ContextData, operator_config::IngressConfig};

// Modifies Nginx deployment to add a range of ports that will be used by the operator while creating new nodes
async fn open_nginx_deployment_ports(
    client: Client,
    ingress_config: &IngressConfig,
) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let min_port = ingress_config.p2p_port_min.as_ref().unwrap().parse::<i32>().unwrap();
    let max_port = ingress_config.p2p_port_max.as_ref().unwrap().parse::<i32>().unwrap();
    let label_selector = ingress_config.selector_labels.as_ref().unwrap().iter().map(|entry| format!("{}={}", entry.0, entry.1)).collect::<Vec<String>>().join(",");
    let api_deployment: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployments = api_deployment.list(&ListParams::default().labels(&label_selector)).await?;
    let mut deployment = deployments.items.first().map(|deployment| deployment.to_owned()).unwrap();
    let pod_spec = deployment.to_owned().spec.unwrap().template.spec.unwrap().to_owned();
    let deployment_ports: Vec<ContainerPort> = pod_spec.containers.first().unwrap().ports.as_ref().unwrap().to_owned();
    let mut new_deployment_ports: Vec<ContainerPort> = deployment_ports.iter().map(|e| e.to_owned())
        .filter(|port_spec| {
            port_spec.container_port < min_port || port_spec.container_port > max_port
        }).collect();
    for port_number in min_port..max_port {
        new_deployment_ports.push(ContainerPort {
            container_port: port_number,
            name: Some(format!("{}-tcp", port_number.to_string())),
            protocol: Some("TCP".to_string()),
            ..ContainerPort::default()
        });
        new_deployment_ports.push(ContainerPort {
            container_port: port_number,
            name: Some(format!("{}-udp", port_number.to_string())),
            protocol: Some("UDP".to_string()),
            ..ContainerPort::default()
        });
    }
    deployment.spec.as_mut().unwrap().template.spec.as_mut().unwrap().containers[0].ports = Some(new_deployment_ports.to_owned());

    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation {
        path: "/spec/template/spec/containers".to_owned(),
        value: json!([deployment.spec.as_mut().unwrap().template.spec.as_mut().unwrap().containers[0]]),
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);

    match api_deployment.patch(&deployment.metadata.name.as_ref().unwrap().to_owned(), &PatchParams::default(), &patch).await {
        Ok(_) => Ok(()),
        Err(error) => Ok(error!("Could not open Nginx default ports on deployment: {:?}", error)),
    }
}

// Modifies Nginx service to add a range of ports that will be used by the operator while creating new nodes
async fn open_nginx_service_ports(
    client: Client,
    ingress_config: &IngressConfig,
) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let min_port = ingress_config.p2p_port_min.as_ref().unwrap().parse::<i32>().unwrap();
    let max_port = ingress_config.p2p_port_max.as_ref().unwrap().parse::<i32>().unwrap();
    let label_selector = ingress_config.selector_labels.as_ref().unwrap().iter()
        .map(|entry| format!("{}={}", entry.0, entry.1)).collect::<Vec<String>>().join(",");

    let api_service: Api<Service> = Api::namespaced(client.clone(), namespace);
    let services = api_service.list(&ListParams::default().labels(&label_selector)).await?;
    let service = services.items.first().map(|deployment| deployment.to_owned()).unwrap();
    let service_ports = service.to_owned().spec.unwrap().ports.unwrap().to_owned();
    let mut new_service_ports: Vec<ServicePort> = service_ports.iter().map(|e| e.to_owned())
        .filter(|service_port| service_port.port < min_port || service_port.port > max_port).collect();
    for port_number in min_port..max_port {
        new_service_ports.push(ServicePort {
            port: port_number,
            name: Some(format!("{}-tcp", port_number.to_string())),
            protocol: Some("TCP".to_string()),
            target_port: Some(IntOrString::Int(port_number)),
            ..ServicePort::default()
        });
        new_service_ports.push(ServicePort {
            port: port_number,
            name: Some(format!("{}-udp", port_number.to_string())),
            protocol: Some("UDP".to_string()),
            target_port: Some(IntOrString::Int(port_number)),
            ..ServicePort::default()
        });
    }
    let json_patch = json_patch::Patch(vec![PatchOperation::Replace(ReplaceOperation {
        path: "/spec/ports".to_owned(),
        value: json!(new_service_ports.to_owned()),
    })]);
    let patch: Patch<&Value> = Patch::Json::<&Value>(json_patch);
    match api_service.patch(&service.metadata.name.as_ref().unwrap().to_owned(), &PatchParams::default(), &patch).await
    {
        Ok(_) => Ok(()),
        Err(error) => Ok(error!("Could not open Nginx default ports on service: {:?}", error)),
    }
}

/// Boot operator
pub async fn start(client: Client, context_data: Arc<ContextData>) -> () {
    // Open Nginx Ports
    if context_data.config.ingress.ingress_class_name == "nginx" {
        open_nginx_deployment_ports(client.clone(), &context_data.config.ingress).await.unwrap();
        open_nginx_service_ports(client, &context_data.config.ingress).await.unwrap();
    }
}
