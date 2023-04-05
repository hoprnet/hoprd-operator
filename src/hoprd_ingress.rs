use k8s_openapi::{api::{networking::v1::{Ingress, IngressSpec, IngressRule, HTTPIngressRuleValue, HTTPIngressPath, IngressBackend, IngressServiceBackend, ServiceBackendPort, IngressTLS}}, apimachinery::pkg::apis::meta::v1::OwnerReference};
use kube::{Api, Client, Error, core::ObjectMeta, api::{PostParams, DeleteParams}, runtime::wait::{conditions, await_condition}};
use std::collections::{BTreeMap};


use crate::{utils, model::IngressConfig};

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
