

use std::collections::BTreeMap;

use kube::api::{DeleteParams, PostParams};
use kube::core::ObjectMeta;
use kube::Error;
use kube::{Client, Api};

use crate::servicemonitor::{ServiceMonitorSpec, ServiceMonitorEndpoints, ServiceMonitorEndpointsBasicAuth, ServiceMonitorEndpointsBasicAuthUsername, ServiceMonitorNamespaceSelector, ServiceMonitorSelector, ServiceMonitorEndpointsBasicAuthPassword};
use crate::{
    constants,
    hoprd::HoprdSpec,
    servicemonitor::ServiceMonitor,
    utils,
};


/// Creates a new serviceMonitor to enable the monitoring with Prometheus of the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the deployment to be created
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
/// - `hoprd_spec` - Details about the hoprd configuration node
///
pub async fn create_service_monitor(client: Client, name: &str, namespace: &str, hoprd_spec: &HoprdSpec) -> Result<ServiceMonitor, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());
    let api: Api<ServiceMonitor> = Api::namespaced(client, namespace);


    let service_monitor: ServiceMonitor = ServiceMonitor {
        metadata: ObjectMeta { 
            labels: Some(labels.clone()),
             name: Some(name.to_owned()), 
             namespace: Some(namespace.to_owned()),
             ..ObjectMeta::default()
            },
        spec: ServiceMonitorSpec { 
            endpoints: vec![ServiceMonitorEndpoints {
                interval:Some("15s".to_owned()),
                path: Some("/api/v2/node/metrics".to_owned()),
                port:Some("api".to_owned()),
                basic_auth: Some(ServiceMonitorEndpointsBasicAuth{
                    username:Some(ServiceMonitorEndpointsBasicAuthUsername{
                        key: hoprd_spec
                        .secret.as_ref().unwrap()
                        .api_token_ref_key.as_ref().unwrap_or(&constants::HOPRD_API_TOKEN.to_owned())
                        .to_string(),
                        name: Some(hoprd_spec.secret.as_ref().unwrap().secret_name.to_owned()),
                        optional:Some(false)
                    }),
                    password:Some(ServiceMonitorEndpointsBasicAuthPassword {
                        key: hoprd_spec
                        .secret.as_ref().unwrap()
                        .metrics_password_ref_key.as_ref().unwrap_or(&constants::HOPRD_METRICS_PASSWORD.to_owned())
                        .to_string(),
                        name: Some(hoprd_spec.secret.as_ref().unwrap().secret_name.to_owned()),
                        optional:Some(false)
                    }),
                }), 
                authorization: None,
                bearer_token_file: None,
                bearer_token_secret: None,
                follow_redirects: None,
                honor_labels: None,
                honor_timestamps: None,
                metric_relabelings: None,
                oauth2: None,
                params: None,
                proxy_url: None,
                relabelings: None,
                scheme: None,
                scrape_timeout: None,
                target_port: None,
                tls_config: None }],
            job_label: Some(name.to_owned()),
            namespace_selector: Some(ServiceMonitorNamespaceSelector {
                match_names: Some(vec![ namespace.to_owned() ]),
                any: Some(false)
            }),
            selector: ServiceMonitorSelector {
                match_labels: Some(labels),
                match_expressions: None
            },
            label_limit: None,
            label_name_length_limit: None,
            label_value_length_limit: None,
            pod_target_labels: None,
            sample_limit: None,
            target_labels: None,
            target_limit: None,
        }
    };

    // Create the serviceMonitor defined above
    api.create(&PostParams::default(), &service_monitor).await


}


/// Deletes an existing serviceMonitor.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the ServiceMonitor with
/// - `name` - Name of the ServiceMonitor to delete
/// - `namespace` - Namespace the existing ServiceMonitor resides in
///
pub async fn delete_service_monitor(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<ServiceMonitor> = Api::namespaced(client, namespace);
    if let Some(_secret) = api.get_opt(&name).await? {
        api.delete(name, &DeleteParams::default()).await?;
        Ok(println!("[INFO] ServiceMonitor successfully deleted"))
    } else {
        Ok(println!("[INFO] ServiceMonitor {name} in namespace {namespace} about to delete not found"))
    }
}
