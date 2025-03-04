use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{DeleteParams, PostParams};
use kube::core::ObjectMeta;
use kube::runtime::wait::{await_condition, conditions};
use kube::Error;
use kube::{Api, Client};
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::info;

use crate::context_data::ContextData;
use crate::servicemonitor::{
    ServiceMonitorEndpoints, ServiceMonitorEndpointsRelabelings, ServiceMonitorEndpointsRelabelingsAction, ServiceMonitorNamespaceSelector,
    ServiceMonitorSelector, ServiceMonitorSpec,
};
use crate::{constants, servicemonitor::ServiceMonitor};

/// Creates a new serviceMonitor to enable the monitoring with Prometheus of the hoprd node,
pub async fn create_service_monitor(context_data: Arc<ContextData>, name: &str, namespace: &str, owner_references: Option<Vec<OwnerReference>>) -> Result<ServiceMonitor, Error> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), context_data.config.instance.name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), name.to_owned());

    let api: Api<ServiceMonitor> = Api::namespaced(context_data.client.clone(), namespace);

    let service_monitor: ServiceMonitor = ServiceMonitor {
        metadata: ObjectMeta {
            labels: Some(labels.clone()),
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: ServiceMonitorSpec {
            endpoints: vec![ServiceMonitorEndpoints {
                interval: Some("15s".to_owned()),
                path: Some("/metrics".to_owned()),
                port: Some("metrics".to_owned()),
                basic_auth: None,
                authorization: None,
                bearer_token_file: None,
                bearer_token_secret: None,
                // bearer_token_secret: Some(ServiceMonitorEndpointsBearerTokenSecret {
                //     key: constants::IDENTITY_POOL_API_TOKEN_REF_KEY.to_owned(),
                //     name: Some(secret_name.to_owned()),
                //     optional: Some(false),
                // }),
                follow_redirects: None,
                honor_labels: None,
                honor_timestamps: None,
                metric_relabelings: None,
                oauth2: None,
                params: None,
                proxy_url: None,
                relabelings: Some(build_metric_relabel()),
                scheme: None,
                scrape_timeout: None,
                target_port: None,
                tls_config: None,
            }],
            job_label: Some(name.to_owned()),
            namespace_selector: Some(ServiceMonitorNamespaceSelector {
                match_names: Some(vec![namespace.to_owned()]),
                any: Some(false),
            }),
            selector: ServiceMonitorSelector {
                match_labels: Some(labels),
                match_expressions: None,
            },
            label_limit: None,
            label_name_length_limit: None,
            label_value_length_limit: None,
            pod_target_labels: None,
            sample_limit: None,
            target_labels: None,
            target_limit: None,
        },
    };

    // Create the serviceMonitor defined above
    api.create(&PostParams::default(), &service_monitor).await
}

fn create_relabel_rule(source_suffix: &str, target_name: &str) -> ServiceMonitorEndpointsRelabelings {
    ServiceMonitorEndpointsRelabelings {
        action: Some(ServiceMonitorEndpointsRelabelingsAction::Replace),
        source_labels: Some(vec![format!("__meta_kubernetes_pod_label_hoprds_hoprnet_org_{}", source_suffix)]),
        target_label: Some(format!("hoprd_{}", target_name)),
        modulus: None,
        regex: None,
        replacement: None,
        separator: None,
    }
}

fn build_metric_relabel() -> Vec<ServiceMonitorEndpointsRelabelings> {
    vec![
        create_relabel_rule("network", "network"),
        create_relabel_rule("safeAddress", "safe_address"),
        create_relabel_rule("nativeAddress", "address"),
        create_relabel_rule("peerId", "peer_id"),
        create_relabel_rule("cluster", "cluster"),
    ]
}

/// Deletes an existing serviceMonitor.
pub async fn delete_service_monitor(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<ServiceMonitor> = Api::namespaced(client, namespace);
    if let Some(service_monitor) = api.get_opt(name).await? {
        let uid = service_monitor.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("ServiceMonitor {name} successfully deleted"))
    } else {
        Ok(info!("ServiceMonitor {name} in namespace {namespace} about to delete not found"))
    }
}
