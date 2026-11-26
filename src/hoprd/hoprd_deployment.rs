use crate::hoprd::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_hoprd::identity_hoprd_resource::IdentityHoprd;
use crate::identity_pool::identity_pool_resource::IdentityPool;
use crate::model::Error;
use crate::operator_config::OtlpConfig;
use crate::{context_data::ContextData, hoprd::hoprd_deployment};
use base64::{engine::general_purpose, Engine as _};
use rand::distr::Alphanumeric;

use crate::{
    constants,
    hoprd::hoprd_resource::{Hoprd, HoprdSpec},
    utils,
};
use futures::StreamExt;
use k8s_openapi::api::batch::v1::JobSpec;
use k8s_openapi::api::core::v1::{
    ConfigMapEnvSource, Container, ContainerPort, EmptyDirVolumeSource, EnvFromSource, EnvVar, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec, SecretEnvSource, Volume, VolumeMount
};
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy},
    batch::v1::{Job, JobStatus},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, OwnerReference};
use kube::api::{ObjectMeta, Patch, PatchParams, PostParams, WatchEvent, WatchParams};
use kube::{Api, Client, Resource, ResourceExt};
use rand::RngExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{error, info};

/// Creates a new deployment for running the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `hoprd` - Details about the hoprd configuration node
///
pub async fn create_deployment(context_data: Arc<ContextData>, hoprd: &Hoprd, identity_hoprd: &IdentityHoprd, hoprd_host: &str, p2p_port: u16) -> Result<Deployment, Error> {
    let namespace: String = hoprd.namespace().ok_or_else(|| Error::UserInputError("The Hoprd resource has no namespace".to_owned()))?;
    let name: String = hoprd.name_any();
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).ok_or_else(|| Error::UserInputError("The Hoprd resource has no uid".to_owned()))?]);
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await?;

    let mut labels: BTreeMap<String, String> = utils::common_lables(identity_pool.name_any(), Some(name.to_owned()), Some("node".to_owned()));
    labels.insert(constants::LABEL_NODE_NETWORK.to_owned(), identity_pool.spec.network.clone());
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool.name_any());
    labels.insert(constants::LABEL_NODE_ADDRESS.to_owned(), identity_hoprd.spec.node_address.to_owned());
    labels.insert(constants::LABEL_NODE_SAFE_ADDRESS.to_owned(), identity_hoprd.spec.safe_address.to_owned());
    labels.insert(constants::LABEL_NODE_MODULE_ADDRESS.to_owned(), identity_hoprd.spec.module_address.to_owned());

    // Propagating ClusterHopd instance
    if let Some(cluster_hoprd) = hoprd.labels().get(constants::LABEL_NODE_CLUSTER) {
        labels.insert(constants::LABEL_NODE_CLUSTER.to_owned(), cluster_hoprd.to_owned());
    }

    // Add the user defined labels and annotations to the deployment metadata, keeping them out of the
    // selector labels. Managed labels win on key collisions so that user labels cannot desync the
    // deployment metadata from the immutable selector
    let custom_labels = HoprdDeploymentSpec::get_labels(hoprd.spec.deployment.clone());
    let custom_annotations = HoprdDeploymentSpec::get_annotations(hoprd.spec.deployment.clone());
    let mut deployment_labels = labels.clone();
    deployment_labels.extend(custom_labels.into_iter().filter(|(key, _)| !labels.contains_key(key)));
    let deployment_annotations = if custom_annotations.is_empty() { None } else { Some(custom_annotations) };

    // Definition of the deployment. Alternatively, a YAML representation could be used as well.
    let deployment: Deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(deployment_labels),
            annotations: deployment_annotations,
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(build_deployment_spec(labels, &hoprd.spec, identity_pool, identity_hoprd, hoprd_host, p2p_port, context_data.config.otlp.as_ref()).await?),
        ..Deployment::default()
    };

    // Create the deployment defined above
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), &namespace);
    let deployment = api.create(&PostParams::default(), &deployment).await?;
    info!("Deployment {} created successfully", name.to_owned());
    Ok(deployment)
}

#[allow(clippy::too_many_arguments)]
pub async fn build_deployment_spec(
    labels: BTreeMap<String, String>,
    hoprd_spec: &HoprdSpec,
    identity_pool: IdentityPool,
    identity_hoprd: &IdentityHoprd,
    hoprd_host: &str,
    p2p_port: u16,
    otlp: Option<&OtlpConfig>,
) -> Result<DeploymentSpec, Error> {
    let replicas: i32 = if hoprd_spec.enabled.unwrap_or(true) { 1 } else { 0 };
    let mut containers: Vec<Container> = extra_containers(hoprd_spec.deployment.clone())?;
    containers.push(hoprd_container(hoprd_spec, &identity_pool, identity_hoprd, hoprd_host, p2p_port, &labels, otlp)?);

    // Add the user defined labels and annotations to the pod template, keeping them out of the
    // selector labels. Managed labels win on key collisions so that the pod template always keeps
    // matching the immutable selector
    let mut pod_labels = labels.clone();
    pod_labels.extend(HoprdDeploymentSpec::get_labels(hoprd_spec.deployment.clone()).into_iter().filter(|(key, _)| !labels.contains_key(key)));
    let custom_annotations = HoprdDeploymentSpec::get_annotations(hoprd_spec.deployment.clone());
    let pod_annotations = if custom_annotations.is_empty() { None } else { Some(custom_annotations) };

    Ok(DeploymentSpec {
        replicas: Some(replicas),
        strategy: Some(DeploymentStrategy {
            type_: Some("Recreate".to_owned()),
            ..DeploymentStrategy::default()
        }),
        selector: LabelSelector {
            match_expressions: None,
            match_labels: Some(labels.clone()),
        },
        template: PodTemplateSpec {
            spec: Some(PodSpec {
                init_containers: Some(vec![init_container(hoprd_spec, &identity_pool, identity_hoprd)]),
                containers,
                volumes: Some(build_volumes(&identity_hoprd.name_any()).await),
                ..PodSpec::default()
            }),
            metadata: Some(ObjectMeta {
                labels: Some(pod_labels),
                annotations: pod_annotations,
                ..ObjectMeta::default()
            }),
        },
        ..DeploymentSpec::default()
    })
}

pub async fn modify_deployment(
    context_data: Arc<ContextData>,
    deployment_name: &str,
    namespace: &str,
    hoprd_spec: &HoprdSpec,
    previous_deployment_spec: Option<HoprdDeploymentSpec>,
    identity_hoprd: &IdentityHoprd,
) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), namespace);
    let deployment = match api.get_opt(deployment_name).await? {
        Some(deployment) => deployment,
        None => {
            error!("Deployment {deployment_name} not found in namespace {namespace}");
            return Err(Error::HoprdStatusError(format!("Deployment '{deployment_name}' not found in namespace '{namespace}'")));
        }
    };
    let invalid_deployment = || Error::HoprdStatusError(format!("Deployment '{deployment_name}' in namespace '{namespace}' has an unexpected structure"));
    let pod_spec = deployment.spec.clone().and_then(|spec| spec.template.spec).ok_or_else(invalid_deployment)?;
    let hoprd_container = pod_spec.containers.iter().find(|&container| container.name == "hoprd").ok_or_else(invalid_deployment)?;
    let hoprd_host_env_var = hoprd_container.env.as_ref().and_then(|env| env.iter().find(|&env_var| env_var.name.eq(&constants::HOPRD_HOST.to_owned()))).ok_or_else(invalid_deployment)?;
    let hoprd_host_port = hoprd_host_env_var.value.as_ref().ok_or_else(invalid_deployment)?.to_owned();
    let (hoprd_host, p2p_port_string) = hoprd_host_port.split_once(':').ok_or_else(invalid_deployment)?;
    let p2p_port = p2p_port_string.parse::<u16>().map_err(|_| invalid_deployment())?;
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await?;
    // Use the selector labels so that user defined labels on the deployment metadata do not leak into the immutable selector
    let selector_labels = deployment.spec.clone().ok_or_else(invalid_deployment)?.selector.match_labels.unwrap_or(deployment.labels().to_owned());
    let spec = build_deployment_spec(selector_labels.clone(), hoprd_spec, identity_pool, identity_hoprd, hoprd_host, p2p_port, context_data.config.otlp.as_ref()).await?;
    let custom_labels = HoprdDeploymentSpec::get_labels(hoprd_spec.deployment.clone());
    let custom_annotations = HoprdDeploymentSpec::get_annotations(hoprd_spec.deployment.clone());
    let previous_labels = HoprdDeploymentSpec::get_labels(previous_deployment_spec.clone());
    let previous_annotations = HoprdDeploymentSpec::get_annotations(previous_deployment_spec);
    // User labels and annotations removed from the spec must be patched as null so the JSON merge patch clears them
    let removed_labels: Vec<String> = previous_labels.keys().filter(|key| !custom_labels.contains_key(*key) && !selector_labels.contains_key(*key)).cloned().collect();
    let removed_annotations: Vec<String> = previous_annotations.keys().filter(|key| !custom_annotations.contains_key(*key)).cloned().collect();
    let mut spec_value = serde_json::to_value(&spec)?;
    if let Some(template_metadata) = spec_value.pointer_mut("/template/metadata").and_then(|metadata| metadata.as_object_mut()) {
        mark_keys_as_removed(template_metadata, "labels", &removed_labels);
        mark_keys_as_removed(template_metadata, "annotations", &removed_annotations);
    }
    let mut metadata = serde_json::Map::new();
    metadata.insert("labels".to_owned(), serde_json::to_value(&custom_labels)?);
    metadata.insert("annotations".to_owned(), serde_json::to_value(&custom_annotations)?);
    mark_keys_as_removed(&mut metadata, "labels", &removed_labels);
    mark_keys_as_removed(&mut metadata, "annotations", &removed_annotations);
    let patch = &Patch::Merge(json!({
        "metadata": metadata,
        "spec": spec_value
    }));
    api.patch(deployment_name, &PatchParams::default(), patch).await?;
    Ok(())
}

/// Sets the given keys to null in a map field of a JSON merge patch so they get removed from the resource
fn mark_keys_as_removed(metadata: &mut serde_json::Map<String, Value>, field: &str, removed_keys: &[String]) {
    if removed_keys.is_empty() {
        return;
    }
    let field_value = metadata.entry(field.to_owned()).or_insert_with(|| json!({}));
    if let Some(field_object) = field_value.as_object_mut() {
        for key in removed_keys {
            field_object.insert(key.to_owned(), Value::Null);
        }
    }
}

pub fn extra_containers(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Result<Vec<Container>, Error> {
    let default_deployment_spec = HoprdDeploymentSpec::default();
    let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
    if let Some(extra_containers_string) = hoprd_deployment_spec.extra_containers {
        let extra_containers: Vec<Container> = serde_yml::from_str(&extra_containers_string)?;
        Ok(extra_containers)
    } else {
        Ok(vec![])
    }
}

pub fn init_container(hoprd_spec: &HoprdSpec,
    identity_pool: &IdentityPool,
    identity_hoprd: &IdentityHoprd,) -> Container {
    let encoded_configuration = general_purpose::STANDARD.encode(&hoprd_spec.config);
    let volume_mounts: Option<Vec<VolumeMount>> = build_volume_mounts();
    let args = Some(vec![String::from(
        "set -x\n\
        set -e\n\
        echo $HOPRD_IDENTITY_FILE | base64 -d > /app/hoprd-identity/.hopr-id\n\
        echo $HOPRD_CONFIGURATION | base64 -d > /app/hoprd-identity/config.yaml"
    )]);
    Container {
        name: "init".to_owned(),
        image: Some(constants::ALPINE_IMAGE.to_owned()),
        env: Some(vec![
            EnvVar {
                name: constants::HOPRD_IDENTITY_FILE.to_owned(),
                value: Some(identity_hoprd.spec.identity_file.to_owned()),
                ..EnvVar::default()
            },
            EnvVar {
                name: constants::HOPRD_CONFIGURATION.to_owned(),
                value: Some(encoded_configuration),
                ..EnvVar::default()
            },
        ]),
        env_from: Some(vec![
            EnvFromSource {
                config_map_ref: Some(ConfigMapEnvSource {
                    name: format!("{}-env-vars", identity_pool.name_any()),
                    ..ConfigMapEnvSource::default()
                }),
                ..EnvFromSource::default()
            }
        ]),
        command: Some(vec!["sh".to_string(), "-c".to_string()]),
        args,
        volume_mounts,
        ..Container::default()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn hoprd_container(hoprd_spec: &HoprdSpec,
    identity_pool: &IdentityPool,
    identity_hoprd: &IdentityHoprd,
    hoprd_host: &str,
    p2p_port: u16,
    labels: &BTreeMap<String, String>,
    otlp: Option<&OtlpConfig>) -> Result<Container, Error> {
    let image = format!(
        "{}/{}:{}",
        constants::HOPR_DOCKER_REGISTRY.to_owned(),
        constants::HOPR_DOCKER_IMAGE_NAME.to_owned(),
        hoprd_spec.version.to_owned()
    );

    let resources = Some(HoprdDeploymentSpec::get_resource_requirements(hoprd_spec.deployment.clone())?);
    let liveness_probe = HoprdDeploymentSpec::get_liveness_probe(hoprd_spec.deployment.clone())?;
    let readiness_probe = HoprdDeploymentSpec::get_readiness_probe(hoprd_spec.deployment.clone())?;
    let startup_probe = HoprdDeploymentSpec::get_startup_probe(hoprd_spec.deployment.clone())?;
    let volume_mounts: Option<Vec<VolumeMount>> = build_volume_mounts();
    let hoprd_host_port = format!("{}:{}", hoprd_host, p2p_port);

    Ok(Container {
        name: "hoprd".to_owned(),
        image: Some(image),
        image_pull_policy: Some("Always".to_owned()),
        ports: Some(build_ports(p2p_port.into())),
        env: Some(build_env_vars(identity_hoprd, &hoprd_host_port, hoprd_spec, otlp, labels)?),
        env_from: Some(vec![
            EnvFromSource {
                secret_ref: Some(SecretEnvSource {
                    name: format!("{}-env-vars", identity_pool.name_any()),
                    ..SecretEnvSource::default()
                }),
                ..EnvFromSource::default()
            },
            EnvFromSource {
                config_map_ref: Some(ConfigMapEnvSource {
                    name: format!("{}-env-vars", identity_pool.name_any()),
                    ..ConfigMapEnvSource::default()
                }),
                ..EnvFromSource::default()
            }
        ]),
        liveness_probe,
        readiness_probe,
        startup_probe,
        volume_mounts,
        resources,
        ..Container::default()
    })
}

/// Deletes an existing deployment.
pub async fn delete_deployment(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    crate::utils::delete_and_wait(api, name).await?;
    Ok(())
}

pub async fn delete_database(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), namespace);
    let deployment = api.get(deployment_name).await?;
    let invalid_deployment = || Error::HoprdStatusError(format!("Deployment '{deployment_name}' in namespace '{namespace}' has no hoprd-db volume"));
    let volumes = deployment.spec.as_ref().and_then(|spec| spec.template.spec.as_ref()).and_then(|pod_spec| pod_spec.volumes.as_ref()).ok_or_else(invalid_deployment)?;
    let volume: &Volume = volumes.iter().find(|&volume| volume.name.eq("hoprd-db")).ok_or_else(invalid_deployment)?;
    let pvc_name = volume.persistent_volume_claim.as_ref().ok_or_else(invalid_deployment)?.claim_name.clone();
    info!("Scaling down deployment {} in namespace {}", deployment_name, namespace);
    let patch = Patch::Merge(json!({ "spec": { "replicas": 0 } }));
    match api.patch(deployment_name, &PatchParams::default(), &patch).await {
        Ok(_) => {}
        Err(error) => error!("Could not scale down deployment {deployment_name}: {:?}", error),
    };
    info!("Deleting hoprd database for {} in namespace {}", deployment_name, namespace);
    let delete_result = hoprd_deployment::job_delete_database(context_data.clone(), deployment_name, namespace, &pvc_name).await;
    if let Err(e) = delete_result {
        error!("Failed to delete database: {:?}", e);
        return Err(e);
    }
    info!("Scaling up deployment {} in namespace {}", deployment_name, namespace);
    let patch = Patch::Merge(json!({ "spec": { "replicas": 1 } }));
    match api.patch(deployment_name, &PatchParams::default(), &patch).await {
        Ok(_) => {}
        Err(error) => error!("Could not scale up deployment {deployment_name}: {:?}", error),
    };
    Ok(())
}

pub async fn job_delete_database(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str, pvc_name: &str) -> Result<(), Error> {
    let api: Api<Job> = Api::namespaced(context_data.client.clone(), namespace);
    let rng = rand::rng();
    let suffix: String = rng.sample_iter(&Alphanumeric).take(10).map(char::from).collect();
    // TODO: Remove only indexes when migration to v4 is finished. The new path will be: /app/hoprd-db/index_db/*
    let command = "rm -rf /app/hoprd-db/*".to_string();

    let job_name = format!("{}-delete-db-{}", deployment_name, suffix.to_lowercase());
    let job = Job {
        metadata: ObjectMeta {
            name: Some(job_name.clone()),
            ..Default::default()
        },
        spec: Some(JobSpec {
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    volumes: Some(vec![Volume {
                        name: "hoprd-db".to_string(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: pvc_name.to_string(),
                            read_only: Some(false),
                        }),
                        ..Default::default()
                    }]),
                    containers: vec![Container {
                        name: "delete-hoprd-db".to_string(),
                        image: Some(constants::ALPINE_IMAGE.to_string()),
                        command: Some(vec!["/bin/sh".to_string(), "-c".to_string(), command]),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "hoprd-db".to_string(),
                            mount_path: "/app/hoprd-db".to_string(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    restart_policy: Some("Never".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    api.create(&PostParams::default(), &job).await?;
    // Watch the Job to wait for it to complete
    let mut stream = api.watch(&WatchParams::default(), "0").await?.boxed();

    while let Some(event) = stream.next().await {
        match event {
            Ok(WatchEvent::Modified(ref job)) if job.metadata.name.as_deref() == Some(&job_name.clone()) => {
                if let Some(JobStatus { succeeded: Some(1), .. }) = job.status {
                    info!("Job {} completed successfully", job_name);
                    return Ok(());
                } else if let Some(JobStatus { failed: Some(1), .. }) = job.status {
                    error!("Job {} failed", job_name);
                    return Err(Error::JobExecutionError(format!("Job {} failed", job_name)));
                }
            }
            Err(e) => {
                error!("Error watching Job {}: {}", job_name, e);
                return Err(Error::JobExecutionError(format!("Error watching Job {}", job_name)));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Builds the struct VolumeMount to be attached into the Container
fn build_volume_mounts() -> Option<Vec<VolumeMount>> {
    Some(vec![
        VolumeMount {
            name: "hoprd-identity".to_owned(),
            mount_path: "/app/hoprd-identity".to_owned(),
            ..VolumeMount::default()
        },
        VolumeMount {
            name: "hoprd-db".to_owned(),
            mount_path: "/app/hoprd-db".to_owned(),
            ..VolumeMount::default()
        },
        VolumeMount {
            name: "tmp-volume".to_owned(),
            mount_path: "/tmp".to_owned(),
            ..VolumeMount::default()
        },
    ])
}

/// Builds the struct Volume to be included as part of the PodSpec
///
/// # Arguments
/// - `secret` - Secret struct used to build the volume for HOPRD_IDENTITY path
async fn build_volumes(pvc_name: &String) -> Vec<Volume> {
    vec![
        Volume {
            name: "hoprd-identity".to_owned(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Volume::default()
        },
        Volume {
            name: "hoprd-db".to_owned(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: pvc_name.to_owned(),
                read_only: Some(false),
            }),
            ..Volume::default()
        },
        Volume {
            name: "tmp-volume".to_owned(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Volume::default()
        },
    ]
}

/// Build struct ContainerPort
fn build_ports(p2p_port: i32) -> Vec<ContainerPort> {
    vec![
        ContainerPort { container_port: constants::API_PORT, name: Some("api".to_owned()), protocol: Some("TCP".to_owned()), ..ContainerPort::default() },
        ContainerPort { container_port: p2p_port, name: Some("p2p-tcp".to_owned()), protocol: Some("TCP".to_owned()), ..ContainerPort::default() },
        ContainerPort { container_port: p2p_port, name: Some("p2p-udp".to_owned()), protocol: Some("UDP".to_owned()), ..ContainerPort::default() },
    ]
}

///Build struct environment variable
///
fn build_env_vars(identity_hoprd: &IdentityHoprd, hoprd_host: &String, hoprd_spec: &HoprdSpec, otlp: Option<&OtlpConfig>, labels: &BTreeMap<String, String>) -> Result<Vec<EnvVar>, Error> {
    let mut env_vars = Vec::new();
    env_vars.extend_from_slice(&HoprdDeploymentSpec::get_environment_variables(hoprd_spec.deployment.to_owned())?);

    env_vars.push(EnvVar {
            name: constants::HOPRD_HOST.to_owned(),
            value: Some(hoprd_host.to_owned()),
            ..EnvVar::default()
    });
    env_vars.push(EnvVar {
            name: constants::OTEL_SERVICE_NAME.to_owned(),
            value: Some(identity_hoprd.metadata.name.clone().unwrap_or_default()),
            ..EnvVar::default()
    });
    env_vars.push(EnvVar {
            name: constants::HOPRD_SAFE_ADDRESS.to_owned(),
            value: Some(identity_hoprd.spec.safe_address.to_owned()),
            ..EnvVar::default()
    });
    env_vars.push(EnvVar {
            name: constants::HOPRD_MODULE_ADDRESS.to_owned(),
            value: Some(identity_hoprd.spec.module_address.to_owned()),
            ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_API.to_owned(),
        value: Some("1".to_owned()),
        ..EnvVar::default()
    });

    env_vars.extend(otlp_env_vars(otlp, labels));
    Ok(env_vars)
}

/// Environment variables that make hoprd send its telemetry to the OTLP endpoint of the operator configuration.
/// Nothing is set when no endpoint is configured, which keeps the telemetry of hoprd disabled.
///
/// `HOPRD_OTEL_EXPORT_LABELS` adds to every signal the labels that identify the node (network, cluster, node address,
/// safe address and the Kubernetes application labels), so the pipeline that receives them can tell the nodes apart.
fn otlp_env_vars(otlp: Option<&OtlpConfig>, labels: &BTreeMap<String, String>) -> Vec<EnvVar> {
    let Some(otlp) = otlp.filter(|otlp| !otlp.endpoint.trim().is_empty()) else {
        return vec![];
    };
    let env_var = |name: &str, value: String| EnvVar { name: name.to_owned(), value: Some(value), ..EnvVar::default() };
    let mut env_vars = vec![env_var(constants::HOPRD_OTLP_ENDPOINT, otlp.endpoint.trim().to_owned())];
    if let Some(signals) = otlp.signals.as_ref().filter(|signals| !signals.trim().is_empty()) {
        env_vars.push(env_var(constants::HOPRD_OTEL_SIGNALS, signals.trim().to_owned()));
    }
    if let Some(interval) = otlp.metric_export_interval.as_ref().filter(|interval| !interval.trim().is_empty()) {
        env_vars.push(env_var(constants::HOPRD_METRIC_EXPORT_INTERVAL, interval.trim().to_owned()));
    }
    let export_labels: Vec<String> = [
        ("network", constants::LABEL_NODE_NETWORK),
        ("hoprd_cluster", constants::LABEL_NODE_CLUSTER),
        ("node_address", constants::LABEL_NODE_ADDRESS),
        ("safe_address", constants::LABEL_NODE_SAFE_ADDRESS),
        ("app_kubernetes_io_name", constants::LABEL_KUBERNETES_NAME),
        ("app_kubernetes_io_instance", constants::LABEL_KUBERNETES_INSTANCE),
        ("app_kubernetes_io_component", constants::LABEL_KUBERNETES_COMPONENT),
    ]
    .iter()
    .filter_map(|(label, key)| labels.get(*key).filter(|value| !value.is_empty()).map(|value| format!("{label}={value}")))
    .collect();
    if !export_labels.is_empty() {
        env_vars.push(env_var(constants::HOPRD_OTEL_EXPORT_LABELS, export_labels.join(",")));
    }
    env_vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity_hoprd::identity_hoprd_resource::IdentityHoprdSpec;
    use crate::identity_pool::identity_pool_resource::IdentityPoolSpec;

    fn init_script() -> String {
        let spec = HoprdSpec::default();
        let identity_pool = IdentityPool::new("pool", IdentityPoolSpec::default());
        let identity = IdentityHoprd::new("identity", IdentityHoprdSpec::default());
        let args = init_container(&spec, &identity_pool, &identity).args.expect("the init container has args");
        assert_eq!(args.len(), 1);
        args[0].clone()
    }

    // The script is rendered as a multi-line string in the Deployment manifest, so its exact lines are part of the contract
    fn node_labels() -> BTreeMap<String, String> {
        BTreeMap::from([
            (constants::LABEL_NODE_NETWORK.to_owned(), "rotsee".to_owned()),
            (constants::LABEL_NODE_CLUSTER.to_owned(), "cluster-1".to_owned()),
            (constants::LABEL_NODE_SAFE_ADDRESS.to_owned(), "0xsafe".to_owned()),
            (constants::LABEL_KUBERNETES_NAME.to_owned(), "pool".to_owned()),
            (constants::LABEL_KUBERNETES_INSTANCE.to_owned(), "node-1".to_owned()),
            (constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "node".to_owned()),
            (constants::LABEL_NODE_ADDRESS.to_owned(), "0xnode".to_owned()),
        ])
    }

    fn env_value(env: &[EnvVar], name: &str) -> Option<String> {
        env.iter().find(|var| var.name == name).and_then(|var| var.value.clone())
    }

    #[test]
    fn no_otlp_variables_without_an_endpoint() {
        assert!(otlp_env_vars(None, &node_labels()).is_empty());
        let empty = OtlpConfig { endpoint: "  ".to_owned(), signals: Some("traces".to_owned()), metric_export_interval: None };
        assert!(otlp_env_vars(Some(&empty), &node_labels()).is_empty());
    }

    #[test]
    fn otlp_variables_carry_the_endpoint_and_the_node_labels() {
        let otlp = OtlpConfig { endpoint: "http://collector:4318".to_owned(), signals: Some("metrics".to_owned()), metric_export_interval: Some("15000,hopr_session=1000".to_owned()) };
        let env = otlp_env_vars(Some(&otlp), &node_labels());
        assert_eq!(env_value(&env, "HOPRD_OTLP_ENDPOINT").as_deref(), Some("http://collector:4318"));
        assert_eq!(env_value(&env, "HOPRD_OTEL_SIGNALS").as_deref(), Some("metrics"));
        assert_eq!(env_value(&env, "HOPRD_METRIC_EXPORT_INTERVAL").as_deref(), Some("15000,hopr_session=1000"));
        assert_eq!(
            env_value(&env, "HOPRD_OTEL_EXPORT_LABELS").as_deref(),
            Some("network=rotsee,hoprd_cluster=cluster-1,node_address=0xnode,safe_address=0xsafe,app_kubernetes_io_name=pool,app_kubernetes_io_instance=node-1,app_kubernetes_io_component=node")
        );
    }

    #[test]
    fn optional_otlp_settings_and_labels_are_skipped_when_missing() {
        let otlp = OtlpConfig { endpoint: "collector:4317".to_owned(), signals: None, metric_export_interval: None };
        let mut labels = node_labels();
        labels.remove(constants::LABEL_NODE_CLUSTER);
        let env = otlp_env_vars(Some(&otlp), &labels);
        assert!(env_value(&env, "HOPRD_OTEL_SIGNALS").is_none());
        assert!(env_value(&env, "HOPRD_METRIC_EXPORT_INTERVAL").is_none());
        assert!(!env_value(&env, "HOPRD_OTEL_EXPORT_LABELS").unwrap_or_default().contains("hoprd_cluster"));
    }

    #[test]
    fn the_node_opens_the_p2p_port_for_both_protocols_and_no_session_ports() {
        let identity_pool = IdentityPool::new("pool", IdentityPoolSpec::default());
        let identity = IdentityHoprd::new("identity", IdentityHoprdSpec::default());
        let container = hoprd_container(&HoprdSpec::default(), &identity_pool, &identity, "1.2.3.4", 9000, &node_labels(), None).expect("container builds");
        let ports: Vec<(String, i32, String)> = container.ports.unwrap_or_default().into_iter().map(|port| (port.name.unwrap_or_default(), port.container_port, port.protocol.unwrap_or_default())).collect();
        assert_eq!(ports, vec![("api".to_owned(), constants::API_PORT, "TCP".to_owned()), ("p2p-tcp".to_owned(), 9000, "TCP".to_owned()), ("p2p-udp".to_owned(), 9000, "UDP".to_owned())]);
        let env = container.env.unwrap_or_default();
        assert!(env_value(&env, "HOPRD_SESSION_PORT_RANGE").is_none());
        assert_eq!(env_value(&env, "HOPRD_HOST").as_deref(), Some("1.2.3.4:9000"));
    }

    #[test]
    fn init_script_only_writes_the_identity_and_config() {
        assert_eq!(
            init_script(),
            "set -x\nset -e\necho $HOPRD_IDENTITY_FILE | base64 -d > /app/hoprd-identity/.hopr-id\necho $HOPRD_CONFIGURATION | base64 -d > /app/hoprd-identity/config.yaml"
        );
    }
}
