use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource, HTTPGetAction, KeyToPath,
    PodSpec, PodTemplateSpec, Probe, ResourceRequirements, SecretKeySelector, SecretVolumeSource,
    Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{DeleteParams, ObjectMeta, PostParams};
use kube::{Api, Client, Error};
use std::collections::{BTreeMap};
use crate::crd::Secret;
use crate::servicemonitor::{ServiceMonitorSpec, ServiceMonitorEndpoints, ServiceMonitorEndpointsBasicAuth, ServiceMonitorEndpointsBasicAuthUsername, ServiceMonitorNamespaceSelector, ServiceMonitorSelector, ServiceMonitorEndpointsBasicAuthPassword};
use crate::{
    constants,
    crd::{HoprdSpec, Resource},
    servicemonitor::ServiceMonitor,
    utils,
};

/// Creates a new deployment for running the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the deployment to be created
/// - `hoprd_spec` - Details about the hoprd configuration node
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
/// Note: It is assumed the resource does not already exists for simplicity. Returns an `Error` if it does.
pub async fn create_deployment(
    client: Client,
    name: &str,
    hoprd_spec: &HoprdSpec,
    namespace: &str,
) -> Result<Deployment, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());

    // Definition of the deployment. Alternatively, a YAML representation could be used as well.
    let deployment: Deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            ..ObjectMeta::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: name.to_owned(),
                        image: Some(utils::get_hopr_image_tag(&hoprd_spec.version)),
                        image_pull_policy: Some("Always".to_owned()),
                        ports: Some(build_ports().await),
                        env: Some(build_env_vars(&hoprd_spec).await),
                        liveness_probe: Some(build_liveness_probe().await),
                        readiness_probe: Some(build_readiness_probe().await),
                        volume_mounts: Some(build_volume_mounts().await),
                        resources: build_resource_requirements(&hoprd_spec.resources).await,
                        ..Container::default()
                    }],
                    volumes: Some(build_volumes(&hoprd_spec.secret).await),
                    ..PodSpec::default()
                }),
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..ObjectMeta::default()
                }),
            },
            ..DeploymentSpec::default()
        }),
        ..Deployment::default()
    };

    // Create the deployment defined above
    let deployment_api: Api<Deployment> = Api::namespaced(client, namespace);
    deployment_api
        .create(&PostParams::default(), &deployment)
        .await
}

/// Creates a new service for accessing the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the service to be created
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
/// Note: It is assumed the resource does not already exists for simplicity. Returns an `Error` if it does.
pub async fn create_service(client: Client, name: &str, namespace: &str) -> Result<Service, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());

    // Definition of the service. Alternatively, a YAML representation could be used as well.
    let service: Service = Service {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
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

/// Creates a new serviceMonitor to enable the monitoring with Prometheus of the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `name` - Name of the deployment to be created
/// - `hoprd_spec` - Details about the hoprd configuration node
/// - `namespace` - Namespace to create the Kubernetes Deployment in.
///
/// Note: It is assumed the resource does not already exists for simplicity. Returns an `Error` if it does.
pub async fn create_service_monitor(client: Client, name: &str, hoprd_spec: &HoprdSpec, namespace: &str) -> Result<ServiceMonitor, Error> {
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
                        .secret
                        .api_token_ref_key
                        .as_ref()
                        .unwrap_or(&constants::HOPRD_API_TOKEN.to_owned())
                        .to_string(),
                        name: Some(hoprd_spec.secret.secret_name.to_owned()),
                        optional:Some(false)
                    }),
                    password:Some(ServiceMonitorEndpointsBasicAuthPassword {
                        key: hoprd_spec
                        .secret
                        .metrics_password_ref_key
                        .as_ref()
                        .unwrap_or(&"METRICS_PASSWORD".to_owned())
                        .to_string(),
                        name: Some(hoprd_spec.secret.secret_name.to_owned()),
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

/// Builds the struct ResourceRequirement from Resource specified in the node
///
/// # Arguments
/// - `resources` - The resources object on the Hoprd record
async fn build_resource_requirements(resources: &Option<Resource>) -> Option<ResourceRequirements> {
    let mut value_limits: BTreeMap<String, Quantity> = BTreeMap::new();
    let mut value_requests: BTreeMap<String, Quantity> = BTreeMap::new();
    if resources.is_some() {
        let resource = resources.as_ref().unwrap();
        value_limits.insert("cpu".to_owned(), Quantity(resource.limits.cpu.to_owned()));
        value_limits.insert(
            "memory".to_owned(),
            Quantity(resource.limits.memory.to_owned()),
        );
        value_requests.insert("cpu".to_owned(), Quantity(resource.requests.cpu.to_owned()));
        value_requests.insert(
            "memory".to_owned(),
            Quantity(resource.requests.memory.to_owned()),
        );
    } else {
        value_limits.insert("cpu".to_owned(), Quantity("1500m".to_owned()));
        value_limits.insert("memory".to_owned(), Quantity("2Gi".to_owned()));
        value_requests.insert("cpu".to_owned(), Quantity("750m".to_owned()));
        value_requests.insert("memory".to_owned(), Quantity("256Mi".to_owned()));
    }
    return Some(ResourceRequirements {
        limits: Some(value_limits),
        requests: Some(value_requests),
    });
}

/// Builds the struct VolumeMount to be attached into the Container
async fn build_volume_mounts() -> Vec<VolumeMount> {
    let mut volume_mounts = Vec::with_capacity(2);
    volume_mounts.push(VolumeMount {
        name: "hoprd-identity".to_owned(),
        mount_path: "/app/hoprd-identity".to_owned(),
        ..VolumeMount::default()
    });
    volume_mounts.push(VolumeMount {
        name: "hoprd-db".to_owned(),
        mount_path: "/app/hoprd-db".to_owned(),
        ..VolumeMount::default()
    });
    return volume_mounts;
}

/// Builds the struct Volume to be included as part of the PodSpec
/// 
/// # Arguments
/// - `secret` - Secret struct used to build the volume for HOPRD_IDENTITY path
async fn build_volumes(secret: &Secret) -> Vec<Volume> {
    let mut volumes = Vec::with_capacity(2);
    volumes.push(Volume {
        name: "hoprd-identity".to_owned(),
        secret: Some(SecretVolumeSource {
            secret_name: Some(secret.secret_name.to_owned()),
            items: Some(vec![KeyToPath {
                key: secret
                    .identity_ref_key
                    .as_ref()
                    .unwrap_or(&"HOPRD_IDENTITY".to_owned())
                    .to_owned(),
                mode: Some(440),
                path: ".hopr-id".to_owned(),
            }]),
            ..SecretVolumeSource::default()
        }),
        ..Volume::default()
    });

    volumes.push(Volume {
        name: "hoprd-db".to_owned(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Volume::default()
    });
    return volumes;
}

/// Build the liveness probe struct
async fn build_liveness_probe() -> Probe {
    return Probe {
        http_get: Some(HTTPGetAction {
            path: Some("/healthcheck/v2/version".to_owned()),
            port: IntOrString::Int(8080),
            ..HTTPGetAction::default()
        }),
        failure_threshold: Some(6),
        initial_delay_seconds: Some(30),
        period_seconds: Some(20),
        success_threshold: Some(1),
        timeout_seconds: Some(5),
        ..Probe::default()
    };
}

/// Build the readiness probe struct
async fn build_readiness_probe() -> Probe {
    return Probe {
        http_get: Some(HTTPGetAction {
            path: Some("/healthcheck/v1/version".to_owned()),
            port: IntOrString::Int(8080),
            ..HTTPGetAction::default()
        }),
        failure_threshold: Some(6),
        initial_delay_seconds: Some(15),
        period_seconds: Some(10),
        success_threshold: Some(1),
        timeout_seconds: Some(5),
        ..Probe::default()
    };
}

/// Build struct ContainerPort
async fn build_ports() -> Vec<ContainerPort> {
    let mut container_ports = Vec::with_capacity(3);

    container_ports.push(ContainerPort {
        container_port: 3001,
        name: Some("api".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    container_ports.push(ContainerPort {
        container_port: 8080,
        name: Some("heatlh".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    container_ports.push(ContainerPort {
        container_port: 9091,
        name: Some("p2p".to_owned()),
        protocol: Some("UDP".to_owned()),
        ..ContainerPort::default()
    });
    return container_ports;
}

///Build struct environment variable
///
async fn build_env_vars(hoprd_spec: &HoprdSpec) -> Vec<EnvVar> {
    let mut env_vars = build_secret_env_var(&hoprd_spec.secret).await;
    env_vars.extend_from_slice(&build_crd_env_var(&hoprd_spec).await);
    env_vars.extend_from_slice(&build_default_env_var().await);
    return env_vars;
}

/// Build environment variables from secrets
/// 
/// # Arguments
/// - `secret` - Secret struct used to build HOPRD_PASSWORD and HOPRD_API_TOKEN
async fn build_secret_env_var(secret: &Secret) -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(1);

    env_vars.push(EnvVar {
        name: constants::HOPRD_PASSWORD.to_owned(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: secret
                    .password_ref_key
                    .as_ref()
                    .unwrap_or(&constants::HOPRD_PASSWORD.to_owned())
                    .to_string(),
                name: Some(secret.secret_name.to_owned()),
                ..SecretKeySelector::default()
            }),
            ..EnvVarSource::default()
        }),
        ..EnvVar::default()
    });

    env_vars.push(EnvVar {
        name: constants::HOPRD_API_TOKEN.to_owned(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: secret
                    .api_token_ref_key
                    .as_ref()
                    .unwrap_or(&constants::HOPRD_API_TOKEN.to_owned())
                    .to_string(),
                name: Some(secret.secret_name.to_owned()),
                ..SecretKeySelector::default()
            }),
            ..EnvVarSource::default()
        }),
        ..EnvVar::default()
    });
    return env_vars;
}

/// Build environment variables from CRD
///
/// # Arguments
/// - `hoprd_spec` - Details about the hoprd configuration node
async fn build_crd_env_var(hoprd_spec: &HoprdSpec) -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(1);
    env_vars.push(EnvVar {
        name: constants::HOPRD_ENVIRONMENT.to_owned(),
        value: Some(hoprd_spec.environment.to_owned()),
        ..EnvVar::default()
    });

    if hoprd_spec.announce.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ANNOUNCE.to_owned(),
            value: Some(hoprd_spec.announce.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.provider.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_PROVIDER.to_owned(),
            value: Some(hoprd_spec.provider.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.default_strategy.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_DEFAULT_STRATEGY.to_owned(),
            value: Some(hoprd_spec.default_strategy.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.max_auto_channels.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_MAX_AUTOCHANNELS.to_owned(),
            value: Some(hoprd_spec.max_auto_channels.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.auto_redeem_tickets.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_AUTO_REDEEM_TICKETS.to_owned(),
            value: Some(hoprd_spec.auto_redeem_tickets.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.check_unrealized_balance.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_CHECK_UNREALIZED_BALANCE.to_owned(),
            value: Some(
                hoprd_spec
                    .check_unrealized_balance
                    .as_ref()
                    .unwrap()
                    .to_string(),
            ),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.allow_private_node_connections.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ALLOW_PRIVATE_NODE_CONNECTIONS.to_owned(),
            value: Some(
                hoprd_spec
                    .allow_private_node_connections
                    .as_ref()
                    .unwrap()
                    .to_string(),
            ),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.test_announce_local_address.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_TEST_ANNOUNCE_LOCAL_ADDRESSES.to_owned(),
            value: Some(
                hoprd_spec
                    .test_announce_local_address
                    .as_ref()
                    .unwrap()
                    .to_string(),
            ),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.heartbeat_interval.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_INTERVAL.to_owned(),
            value: Some(hoprd_spec.heartbeat_interval.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.heartbeat_threshold.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_THRESHOLD.to_owned(),
            value: Some(hoprd_spec.heartbeat_threshold.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.heartbeat_variance.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_VARIANCE.to_owned(),
            value: Some(hoprd_spec.heartbeat_variance.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.on_chain_confirmations.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ON_CHAIN_CONFIRMATIONS.to_owned(),
            value: Some(
                hoprd_spec
                    .on_chain_confirmations
                    .as_ref()
                    .unwrap()
                    .to_string(),
            ),
            ..EnvVar::default()
        });
    }

    if hoprd_spec.network_quality_threshold.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_NETWORK_QUALITY_THRESHOLD.to_owned(),
            value: Some(
                hoprd_spec
                    .network_quality_threshold
                    .as_ref()
                    .unwrap()
                    .to_string(),
            ),
            ..EnvVar::default()
        });
    }

    return env_vars;
}

/// Build default environment variables
///
async fn build_default_env_var() -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(7);
    env_vars.push(EnvVar {
        name: "DEBUG".to_owned(),
        value: Some("hopr*".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_IDENTITY.to_owned(),
        value: Some("/app/hoprd-identity/.hopr-id".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_DATA.to_owned(),
        value: Some("/app/hoprd-db".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_API.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_API_HOST.to_owned(),
        value: Some("0.0.0.0".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_INIT.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_HEALTH_CHECK.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_HEALTH_CHECK_HOST.to_owned(),
        value: Some("0.0.0.0".to_owned()),
        ..EnvVar::default()
    });
    return env_vars;
}

/// Deletes an existing deployment.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Deployment with
/// - `name` - Name of the deployment to delete
/// - `namespace` - Namespace the existing deployment resides in
///
/// Note: It is assumed the deployment exists for simplicity. Otherwise returns an Error.
pub async fn delete_depoyment(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    api.delete(name, &DeleteParams::default()).await?;
    Ok(())
}

/// Deletes an existing service.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Service with
/// - `name` - Name of the service to delete
/// - `namespace` - Namespace the existing service resides in
///
/// Note: It is assumed the service exists for simplicity. Otherwise returns an Error.
pub async fn delete_service(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Service> = Api::namespaced(client, namespace);
    api.delete(name, &DeleteParams::default()).await?;
    Ok(())
}

/// Deletes an existing serviceMonitor.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the ServiceMonitor with
/// - `name` - Name of the ServiceMonitor to delete
/// - `namespace` - Namespace the existing ServiceMonitor resides in
///
/// Note: It is assumed the ServiceMonitor exists for simplicity. Otherwise returns an Error.
pub async fn delete_service_monitor(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<ServiceMonitor> = Api::namespaced(client, namespace);
    api.delete(name, &DeleteParams::default()).await?;
    Ok(())
}
