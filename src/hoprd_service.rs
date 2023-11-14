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
use std::{collections::BTreeMap, sync::Arc};
use tracing::info;

use crate::{constants, context_data::ContextData, utils};

/// Creates a new service for accessing the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the service to be created
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
pub async fn create_service(context_data: Arc<ContextData>, name: &str, namespace: &str, identity_pool_name: &str, p2p_port: i32, owner_references: Option<Vec<OwnerReference>>) -> Result<Service, Error> {
    let mut labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(),Some(name.to_owned()),None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(),identity_pool_name.to_owned());

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
            ports: Some(service_ports(p2p_port)),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    // Create the service defined above
    let service_api: Api<Service> = Api::namespaced(context_data.client.clone(), namespace);
    service_api.create(&PostParams::default(), &service).await
}

fn service_ports(p2p_port: i32) -> Vec<ServicePort> {
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
    ]
}

/// Deletes an existing service.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
pub async fn delete_service(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Service> = Api::namespaced(client, namespace);
    if let Some(service) = api.get_opt(&name).await? {
        let uid = service.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, &name.to_owned(), conditions::is_deleted(&uid))
            .await
            .unwrap();
        Ok(info!("Service {name} successfully deleted"))
    } else {
        Ok(info!(
            "Service {name} in namespace {namespace} about to delete not found"
        ))
    }
}
