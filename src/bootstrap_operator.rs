use k8s_openapi::{api::{apps::v1::Deployment, core::v1::{ContainerPort, Service, ServicePort}}, apimachinery::pkg::util::intstr::IntOrString};
use kube::{ client::Client, Result, api::{ListParams, PatchParams, Patch}, Api};
use crate::model::{Error as HoprError};

use std::{sync::Arc};

use crate::{ context_data::ContextData, operator_config::IngressConfig};

// Modifies Nginx deployment to add a range of ports that will be used by the operator while creating new nodes
async fn open_nginx_deployment_ports(client: Client, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let min_port = ingress_config.p2p_port_min.as_ref().unwrap().parse::<i32>().unwrap();
    let max_port = ingress_config.p2p_port_max.as_ref().unwrap().parse::<i32>().unwrap();
    let label_selector = ingress_config.selector_labels.as_ref().unwrap().iter().map(|entry| format!("{}={}", entry.0, entry.1)).collect::<Vec<String>>().join(",");
    let api_deployment: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployments = api_deployment.list(&ListParams::default().labels(&label_selector)).await?;
    let mut deployment = deployments.items.first().map(|deployment| deployment.to_owned()).unwrap();
    let pod_spec = deployment.to_owned().spec.unwrap().template.spec.unwrap().to_owned();
    let deployment_ports: Vec<ContainerPort> = pod_spec.containers.first().unwrap().ports.as_ref().unwrap().to_owned();
    let mut new_deployment_ports: Vec<ContainerPort> = deployment_ports.iter()
        .map(|e| e.to_owned())
        .filter(|port_spec| port_spec.container_port < min_port)
        .filter(|port_spec| port_spec.container_port > max_port)
        .collect();
    for port_number in min_port..max_port {
        new_deployment_ports.push(ContainerPort{ 
            container_port: port_number, 
            name: Some(format!("{}-tcp",port_number.to_string())),
            protocol: Some("TCP".to_string()),
            ..ContainerPort::default()});
    }
    deployment.spec.as_mut().unwrap().template.spec.as_mut().unwrap().containers[0].ports = Some(new_deployment_ports.to_owned());
    deployment.metadata.managed_fields = None;

    let pp = &PatchParams{field_manager: Some("application/apply-patch".to_owned()), ..PatchParams::default()};
    match api_deployment.patch(&deployment.metadata.name.as_ref().unwrap().to_owned(), pp, &Patch::Apply(&deployment)).await {
            Ok(_) => Ok(()),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not open Nginx default ports on deployment").to_owned()));
            }
    }
}

// Modifies Nginx service to add a range of ports that will be used by the operator while creating new nodes
async fn open_nginx_service_ports(client: Client, ingress_config: &IngressConfig) -> Result<(), HoprError> {
    let namespace = ingress_config.namespace.as_ref().unwrap();
    let min_port = ingress_config.p2p_port_min.as_ref().unwrap().parse::<i32>().unwrap();
    let max_port = ingress_config.p2p_port_max.as_ref().unwrap().parse::<i32>().unwrap();
    let label_selector = ingress_config.selector_labels.as_ref().unwrap().iter().map(|entry| format!("{}={}", entry.0, entry.1)).collect::<Vec<String>>().join(",");

    let api_service: Api<Service> = Api::namespaced(client.clone(), namespace);
    let services = api_service.list(&ListParams::default().labels(&label_selector)).await?;
    let mut service = services.items.first().map(|deployment| deployment.to_owned()).unwrap();
    let service_ports = service.to_owned().spec.unwrap().ports.unwrap().to_owned();
    let mut new_service_ports: Vec<ServicePort> = service_ports.iter()
        .map(|e| e.to_owned())
        .filter(|service_port| service_port.port < min_port)
        .filter(|service_port| service_port.port > max_port)
        .collect();
    for port_number in min_port..max_port {
        new_service_ports.push(ServicePort{ 
            port: port_number, 
            name: Some(format!("{}-tcp",port_number.to_string())),
            protocol: Some("TCP".to_string()),
            target_port: Some(IntOrString::Int(port_number)),
            ..ServicePort::default()});
    }
    service.spec.as_mut().unwrap().ports = Some(new_service_ports.to_owned());
    service.metadata.managed_fields = None;

    let pp = &PatchParams{field_manager: Some("application/apply-patch".to_owned()), ..PatchParams::default()};
    match api_service.patch(&service.metadata.name.as_ref().unwrap().to_owned(), pp, &Patch::Apply(&service)).await {
            Ok(_) => Ok(()),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                return Err(HoprError::HoprdConfigError(format!("Could not open Nginx default ports on service").to_owned()));
            }
    }
}


/// Boot operator
pub async fn start(client: Client, context_data: Arc<ContextData>) -> () {
    if context_data.config.ingress.ingress_class_name == "nginx" {
        open_nginx_deployment_ports(client.clone(), &context_data.config.ingress).await.unwrap();
        open_nginx_service_ports(client, &context_data.config.ingress).await.unwrap();
    }
}
