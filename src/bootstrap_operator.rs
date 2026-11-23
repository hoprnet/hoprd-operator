use crate::model::Error as HoprError;
use crate::utils::OptionExt;
use k8s_openapi::{
    api::{apps::v1::Deployment, core::v1::ContainerPort}
};
use kube::{
    api::{ListParams, Patch, PatchParams},
    client::Client,
    Api, Result,
};
use serde_json::json;
use serde_json::Value;
use std::sync::Arc;
use tracing::error;

use crate::{context_data::ContextData, operator_config::IngressConfig};

// Modifies Nginx deployment to add a range of ports that will be used by the operator while creating new nodes
async fn open_nginx_deployment_ports(client: Client, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace()?;
    let min_port: i32 = ingress_config.port_min.into();
    let max_port: i32 = ingress_config.port_max as i32 + 1;
    if !(1024..=65535).contains(&min_port) || !(1024..=65535).contains(&max_port) {
        return Err(HoprError::UserInputError("Ports must be between 1024 and 65535".into()));
    }
    if min_port >= max_port {
        return Err(HoprError::UserInputError("min_port must be less than max_port".into()));
    }
    let field_selector = format!("metadata.name={}", ingress_config.deployment_name()?);
    let api_deployment: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployments = api_deployment.list(&ListParams::default().fields(&field_selector)).await?;
    let mut deployment = deployments.items.first().map(|deployment| deployment.to_owned()).or_missing("nginx deployment")?;
    let deployment_name = deployment.metadata.name.clone().or_missing("nginx deployment name")?;
    let container = deployment
        .spec
        .as_mut()
        .and_then(|spec| spec.template.spec.as_mut())
        .and_then(|pod_spec| pod_spec.containers.first_mut())
        .or_missing("nginx deployment container")?;
    let deployment_ports: Vec<ContainerPort> = container.ports.clone().unwrap_or_default();
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
    container.ports = Some(new_deployment_ports);

    // A JSON merge patch replaces the whole `containers` array, which is the same
    // result as the JSON patch replace operation it replaces. It keeps kube's own
    // json-patch version as the only one in the dependency graph.
    let patch: Patch<Value> = Patch::Merge(json!({
        "spec": { "template": { "spec": { "containers": [container] } } }
    }));

    api_deployment.patch(&deployment_name, &PatchParams::default(), &patch).await.inspect_err(|error| {
        error!("Could not open Nginx default ports on deployment: {:?}", error);
    })?;
    Ok(())
}

/// Boot operator
pub async fn start(client: Client, context_data: Arc<ContextData>) -> Result<(), HoprError> {
    // Open Nginx Ports
    if context_data.config.ingress.ingress_class_name == "nginx" {
        open_nginx_deployment_ports(client.clone(), &context_data.config.ingress).await?;
    }
    Ok(())
}
