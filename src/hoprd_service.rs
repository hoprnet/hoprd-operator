use k8s_openapi::{api::core::v1::{ Service, ServicePort, ServiceSpec }, apimachinery::pkg::{util::intstr::IntOrString, apis::meta::v1::OwnerReference}};
use kube::{Api, Client, Error, core::ObjectMeta, api::{PostParams, DeleteParams}, runtime::wait::{await_condition, conditions}};
use std::collections::{BTreeMap};


use crate::{utils};

/// Creates a new service for accessing the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the service to be created
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
pub async fn create_service(client: Client, name: &str, namespace: &str, owner_references: Option<Vec<OwnerReference>>) -> Result<Service, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());

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
            ports: Some(vec![ServicePort {
                name: Some("api".to_owned()),
                port: 3001,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(3001)),
                ..ServicePort::default()
            }]),
            ..ServiceSpec::default()
        }),
        ..Service::default()
    };

    // Create the service defined above
    let service_api: Api<Service> = Api::namespaced(client, namespace);
    service_api.create(&PostParams::default(), &service).await
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
        await_condition(api, &name.to_owned(), conditions::is_deleted(&uid)).await.unwrap();
        Ok(println!("[INFO] Service successfully deleted"))
    } else {
        Ok(println!("[INFO] Service {name} in namespace {namespace} about to delete not found"))
    }
}
