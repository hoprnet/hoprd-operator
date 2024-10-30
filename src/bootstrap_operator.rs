use crate::model::Error as HoprError;
use json_patch::{PatchOperation, ReplaceOperation};
use k8s_openapi::{
    api::{apps::v1::Deployment, core::v1::ContainerPort},
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
async fn open_nginx_deployment_ports(client: Client, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let min_port = ingress_config.port_min.parse::<i32>().map_err(|e| HoprError::UserInputError(format!("Invalid port_min: {}", e)))?;
    let max_port = ingress_config.port_max.parse::<i32>().map_err(|e| HoprError::UserInputError(format!("Invalid port_max: {}", e)))? + 1;
    if !(1024..=65535).contains(&min_port) || !(1024..=65535).contains(&max_port) {
        return Err(HoprError::UserInputError("Ports must be between 1024 and 65535".into()));
    }
    if min_port >= max_port {
        return Err(HoprError::UserInputError("min_port must be less than max_port".into()));
    }
    let field_selector = format!("metadata.name={}", ingress_config.deployment_name.as_ref().unwrap());
    let api_deployment: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployments = api_deployment.list(&ListParams::default().fields(&field_selector)).await?;
    let mut deployment = deployments.items.first().map(|deployment| deployment.to_owned()).unwrap();
    let pod_spec = deployment.to_owned().spec.unwrap().template.spec.unwrap().to_owned();
    let deployment_ports: Vec<ContainerPort> = pod_spec.containers.first().unwrap().ports.as_ref().unwrap().to_owned();
    let mut new_deployment_ports: Vec<ContainerPort> = deployment_ports
        .iter()
        .map(|e| e.to_owned())
        .filter(|port_spec| port_spec.container_port < min_port || port_spec.container_port > max_port)
        .collect();
    for port_number in min_port..max_port {
        new_deployment_ports.push(ContainerPort {
            container_port: port_number,
            name: Some(format!("tcp-{}", port_number)),
            protocol: Some("TCP".to_string()),
            ..ContainerPort::default()
        });
        new_deployment_ports.push(ContainerPort {
            container_port: port_number,
            name: Some(format!("udp-{}", port_number)),
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

/// Boot operator
pub async fn start(client: Client, context_data: Arc<ContextData>) {
    // Open Nginx Ports
    if context_data.config.ingress.ingress_class_name == "nginx" {
        open_nginx_deployment_ports(client.clone(), &context_data.config.ingress).await.unwrap();
    }
}
