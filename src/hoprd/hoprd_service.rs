use k8s_openapi::{
    api::core::v1::{Service, ServicePort, ServiceSpec},
    apimachinery::pkg::{apis::meta::v1::OwnerReference, util::intstr::IntOrString},
};
use kube::{
    api::{DeleteParams, PostParams},
    core::ObjectMeta,
    runtime::wait::{await_condition, conditions},
    Api, Client, Error,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::{Display, Formatter}, sync::Arc};
use tracing::info;

use crate::{constants, context_data::ContextData, utils};

#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdServiceSpec {
    pub r#type: ServiceTypeEnum
}

impl Default for HoprdServiceSpec {
    fn default() -> Self {
        Self {
            r#type: ServiceTypeEnum::ClusterIP
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
            ServiceTypeEnum::LoadBalancer => write!(f, "LoadBalancer")
        }
    }
}


/// Creates a new service for accessing the hoprd node,
pub async fn create_service(context_data: Arc<ContextData>, name: &str, namespace: &str, identity_pool_name: &str, service_type: ServiceTypeEnum, p2p_port: i32, owner_references: Option<Vec<OwnerReference>>) -> Result<String, Error> {
    let mut labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(),Some(name.to_owned()),None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(),identity_pool_name.to_owned());

    if service_type.eq(&ServiceTypeEnum::ClusterIP) {
        let public_ip = context_data.config.ingress.loadbalancer_ip.clone().unwrap();
        create_cluster_ip_service(context_data.clone(), name, namespace, labels, owner_references, p2p_port).await?;
        info!("ClusterIP Service {} created successfully", name.to_owned());
        Ok(public_ip)
    } else {
        let p2p_port= 9091;
        let public_ip = create_load_balancer_service(context_data.clone(), name, namespace, labels, owner_references, p2p_port).await.unwrap();
        info!("LoadBalancer Service {} created successfully", name.to_owned());
        Ok(public_ip)
    }
}

async fn create_load_balancer_service(context_data: Arc<ContextData>, name: &str, namespace: &str, labels: BTreeMap<String, String>, owner_references: Option<Vec<OwnerReference>>, p2p_port: i32) -> Result<String, Error> {
    let api_service: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    let service_api: Service = Service {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references: owner_references.clone(),
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels.clone()),
            type_: Some("ClusterIP".to_owned()),
            ports: Some(build_ports(p2p_port, Some("api"))),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };
    api_service.create(&PostParams::default(), &service_api).await.unwrap();

    let service_tcp: Service = Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-p2p-tcp", name.to_owned())),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references: owner_references.clone(),
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels.clone()),
            type_: Some("LoadBalancer".to_owned()),
            ports: Some(build_ports(p2p_port, Some("p2p-tcp"))),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    api_service.create(&PostParams::default(), &service_tcp).await.unwrap();
    let mut load_balancer_ip = None;
    while load_balancer_ip.is_none() {
        // Wait for a short period before checking the service status again
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Fetch the latest version of the service
        let service = api_service.get(&format!("{}-p2p-tcp", name.to_owned())).await.unwrap();

        // Try to get the IP address from the service status
        load_balancer_ip = service.status
            .clone()
            .and_then(|s| s.load_balancer)
            .and_then(|lb| lb.ingress.clone())
            .and_then(|ingress| ingress.get(0).cloned())
            .and_then(|first_ingress| first_ingress.ip.clone());
    }

    let service_udp: Service = Service {
        metadata: ObjectMeta {
            name: Some(format!("{}-p2p-udp", name.to_owned())),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(ServiceSpec {
            load_balancer_ip: load_balancer_ip.clone(),
            selector: Some(labels.clone()),
            type_: Some("LoadBalancer".to_owned()),
            ports: Some(build_ports(p2p_port, Some("p2p-udp"))),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };
    api_service.create(&PostParams::default(), &service_udp).await.unwrap();
    Ok(load_balancer_ip.unwrap())
}

async fn create_cluster_ip_service(context_data: Arc<ContextData>, name: &str, namespace: &str, labels: BTreeMap<String, String>, owner_references: Option<Vec<OwnerReference>>, p2p_port: i32) -> Result<Service, Error> {
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
            ports: Some(build_ports(p2p_port, None)),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    // Create the service defined above
    let service_api: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    let service = service_api.create(&PostParams::default(), &service).await.unwrap();
    Ok(service)
}

fn build_ports(p2p_port: i32, port_name: Option<&str>) -> Vec<ServicePort> {
    if port_name.is_none() {
            vec![
        ServicePort {
            name: Some("api".to_owned()),
            port: 3001,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::String("api".to_owned())),
            ..ServicePort::default()
        },
        ServicePort {
            name: Some("p2p-tcp".to_owned()),
            port: p2p_port,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(p2p_port)),
            ..ServicePort::default()
        },
        ServicePort {
            name: Some("p2p-udp".to_owned()),
            port: p2p_port,
            protocol: Some("UDP".to_owned()),
            target_port: Some(IntOrString::Int(p2p_port)),
            ..ServicePort::default()
        },
    ]} else {
        let protocol = if port_name.as_ref().unwrap().contains("udp") {
            "UDP"
        } else {
            "TCP"
        };
        let port = if port_name.as_ref().unwrap().contains("api") {
            3001
        } else {
            p2p_port
        };
        vec![
            ServicePort {
                name: Some(port_name.unwrap().to_owned()),
                port: port,
                protocol: Some(protocol.to_string()),
                target_port: Some(IntOrString::Int(port)),
                ..ServicePort::default()
            }
        ]
    }

}

/// Deletes an existing service.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_service(client: Client, name: &str, namespace: &str, service_type: &ServiceTypeEnum) -> Result<(), Error> {
    let api: Api<Service> = Api::namespaced(client, namespace);
    if let Some(service) = api.get_opt(name).await? {
        let uid = service.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api.clone(), name, conditions::is_deleted(&uid)).await.unwrap();
        info!("Service {name} successfully deleted")
    } else {
        info!("Service {name} in namespace {namespace} about to delete not found")
    }
    if service_type.eq(&ServiceTypeEnum::LoadBalancer) {
        let service_p2p_tcp_name = format!("{}-p2p-tcp", name.to_owned());
        if let Some(service) = api.get_opt(&service_p2p_tcp_name).await? {
            let uid = service.metadata.uid.unwrap();
            api.clone().delete(&service_p2p_tcp_name, &DeleteParams::default()).await?;
            await_condition(api.clone(), &service_p2p_tcp_name, conditions::is_deleted(&uid)).await.unwrap();
            info!("Service {service_p2p_tcp_name} successfully deleted")
        } else {
            info!("Service {service_p2p_tcp_name} in namespace {namespace} about to delete not found")
        }
        let service_p2p_udp_name = format!("{}-p2p-udp", name.to_owned());
        if let Some(service) = api.get_opt(&service_p2p_udp_name).await? {
            let uid = service.metadata.uid.unwrap();
            api.clone().delete(&service_p2p_udp_name, &DeleteParams::default()).await?;
            await_condition(api.clone(), &service_p2p_udp_name, conditions::is_deleted(&uid)).await.unwrap();
            info!("Service {service_p2p_udp_name} successfully deleted")
        } else {
            info!("Service {service_p2p_udp_name} in namespace {namespace} about to delete not found")
        }
    }


    Ok(())
}
