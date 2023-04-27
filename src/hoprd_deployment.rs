use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, KeyToPath,
    PodSpec, PodTemplateSpec, Probe, SecretKeySelector, SecretVolumeSource,
     Volume, VolumeMount, Secret, PersistentVolumeClaimVolumeSource, ResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, OwnerReference};
use kube::api::{DeleteParams, ObjectMeta, PostParams, Patch, PatchParams};
use kube::runtime::wait::{await_condition, conditions};
use kube::{Api, Client, ResourceExt, Resource};
use serde_json::json;
use std::collections::{BTreeMap};
use crate::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::model::{Error};
use crate::{
    constants,
    hoprd::{ Hoprd, HoprdSpec},
    model::{HoprdSecret},
    utils,
};

/// Creates a new deployment for running the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `hoprd` - Details about the hoprd configuration node
///
pub async fn create_deployment(client: Client, hoprd: &Hoprd, secret: Secret) -> Result<Deployment, kube::Error> {
    let namespace: String = hoprd.namespace().unwrap();
    let name: String= hoprd.name_any();
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).unwrap()]);
    let hoprd_secret = hoprd.spec.secret.as_ref().unwrap_or(&HoprdSecret { secret_name: secret.name_any(), ..HoprdSecret::default() }).to_owned();
    let node_address = secret.labels().get(constants::LABEL_NODE_ADDRESS).unwrap().to_owned();
    let node_peer_id = secret.labels().get(constants::LABEL_NODE_PEER_ID).unwrap().to_owned();
    let node_network = secret.labels().get(constants::LABEL_NODE_NETWORK).unwrap().to_owned();


    let mut labels: BTreeMap<String, String> = utils::common_lables(&name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "node".to_owned());
    labels.insert(constants::LABEL_NODE_ADDRESS.to_owned(), node_address);
    labels.insert(constants::LABEL_NODE_PEER_ID.to_owned(), node_peer_id);
    labels.insert(constants::LABEL_NODE_NETWORK.to_owned(), node_network);

    // Propagating ClusterHopd instance
    if hoprd.labels().contains_key(constants::LABEL_NODE_CLUSTER) {
        let cluster_hoprd: String = hoprd.labels().get_key_value(constants::LABEL_NODE_CLUSTER).unwrap().1.parse().unwrap();
        labels.insert(constants::LABEL_NODE_CLUSTER.to_owned(), cluster_hoprd);
    }

    // Definition of the deployment. Alternatively, a YAML representation could be used as well.
    let deployment: Deployment = Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(build_deployment_spec(labels, &hoprd.spec, hoprd_secret, &name).await),
        ..Deployment::default()
    };

    // Create the deployment defined above
    let api: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
    api.create(&PostParams::default(), &deployment).await
}

pub async fn build_deployment_spec(labels: BTreeMap<String, String>, hoprd_spec: &HoprdSpec, hoprd_secret: HoprdSecret, pvc_name: &String) -> DeploymentSpec{
    let image = format!("{}/{}:{}", constants::HOPR_DOCKER_REGISTRY.to_owned(), constants::HOPR_DOCKER_IMAGE_NAME.to_owned(), &hoprd_spec.version.to_owned());
    let replicas: i32 = if hoprd_spec.enabled.unwrap_or(true) { 1 } else { 0 };
    let resources: Option<ResourceRequirements> = Some(HoprdDeploymentSpec::get_resource_requirements(hoprd_spec.deployment.clone()));
    let liveness_probe: Option<Probe> = Some(HoprdDeploymentSpec::get_liveness_probe(hoprd_spec.deployment.clone()));
    let readiness_probe: Option<Probe> = Some(HoprdDeploymentSpec::get_readiness_probe(hoprd_spec.deployment.clone()));
    let startup_probe: Option<Probe> = Some(HoprdDeploymentSpec::get_startup_probe(hoprd_spec.deployment.clone()));

    DeploymentSpec {
            replicas: Some(replicas),
            strategy: Some(DeploymentStrategy{
                type_: Some("Recreate".to_owned()),
                ..DeploymentStrategy::default()
            }),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "hoprd".to_owned(),
                        image: Some(image),
                        image_pull_policy: Some("Always".to_owned()),
                        ports: Some(build_ports().await),
                        env: Some(build_env_vars(&hoprd_spec, &hoprd_secret)),
                        liveness_probe,
                        readiness_probe,
                        startup_probe,
                        volume_mounts: Some(build_volume_mounts().await),
                        resources,
                        ..Container::default()
                    }],
                    volumes: Some(build_volumes(&hoprd_secret, &pvc_name).await),
                    ..PodSpec::default()
                }),
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..ObjectMeta::default()
                }),
            },
            ..DeploymentSpec::default()
        }
}

pub async fn modify_deployment(client: Client, deployment_name: &str, namespace: &str, hoprd_spec: &HoprdSpec, hoprd_secret: HoprdSecret) -> Result<Deployment, kube::Error> {

    
    let mut labels: BTreeMap<String, String> = utils::common_lables(&deployment_name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "node".to_owned());
    let spec = build_deployment_spec(labels, hoprd_spec, hoprd_secret, &deployment_name.to_owned()).await;
    let change_set =json!({ "spec": spec });
    let patch = &Patch::Merge(change_set);

    let api: Api<Deployment> = Api::namespaced(client, namespace);
    api.patch(&deployment_name, &PatchParams::default(),patch).await
}

/// Deletes an existing deployment.
///
/// # Arguments:
/// - `client` - A Kubernetes client to delete the Deployment with
/// - `name` - Name of the deployment to delete
/// - `namespace` - Namespace the existing deployment resides in
///
pub async fn delete_depoyment(client: Client, name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(client, namespace);
    if let Some(deployment) = api.get_opt(&name).await? {
        let uid = deployment.metadata.uid.unwrap();        
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, &name.to_owned(), conditions::is_deleted(&uid)).await.unwrap();
        Ok(println!("[INFO] Deployment {name} successfully deleted"))
    } else {
        Ok(println!("[INFO] Deployment {name} in namespace {namespace} about to delete not found"))
    }
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
async fn build_volumes(secret: &HoprdSecret, pvc_name: &String) -> Vec<Volume> {
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
        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
            claim_name: pvc_name.to_owned(),
            read_only: Some(false)
        }),
        ..Volume::default()
    });
    return volumes;
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
        name: Some("p2p-tcp".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    container_ports.push(ContainerPort {
        container_port: 9091,
        name: Some("p2p-udp".to_owned()),
        protocol: Some("UDP".to_owned()),
        ..ContainerPort::default()
    });
    return container_ports;
}

///Build struct environment variable
///
fn build_env_vars(hoprd_spec: &HoprdSpec, secret: &HoprdSecret) -> Vec<EnvVar> {
    let mut env_vars = build_secret_env_var(secret);
    env_vars.extend_from_slice(&build_crd_env_var(&hoprd_spec));
    env_vars.extend_from_slice(&build_default_env_var());
    return env_vars;
}

/// Build environment variables from secrets
/// 
/// # Arguments
/// - `secret` - Secret struct used to build HOPRD_PASSWORD and HOPRD_API_TOKEN
fn build_secret_env_var(secret: &HoprdSecret) -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(2);

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
fn build_crd_env_var(hoprd_spec: &HoprdSpec) -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(1);
    env_vars.push(EnvVar {
        name: constants::HOPRD_ENVIRONMENT.to_owned(),
        value: Some(hoprd_spec.network.to_owned()),
        ..EnvVar::default()
    });

    let config = hoprd_spec.config.to_owned().unwrap_or_default();

    if config.announce.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ANNOUNCE.to_owned(),
            value: Some(config.announce.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.provider.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_PROVIDER.to_owned(),
            value: Some(config.provider.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.default_strategy.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_DEFAULT_STRATEGY.to_owned(),
            value: Some(config.default_strategy.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.max_auto_channels.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_MAX_AUTOCHANNELS.to_owned(),
            value: Some(config.max_auto_channels.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.auto_redeem_tickets.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_AUTO_REDEEM_TICKETS.to_owned(),
            value: Some(config.auto_redeem_tickets.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.check_unrealized_balance.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_CHECK_UNREALIZED_BALANCE.to_owned(),
            value: Some(config.check_unrealized_balance.as_ref().unwrap().to_string(),
            ),
            ..EnvVar::default()
        });
    }

    if config.allow_private_node_connections.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ALLOW_PRIVATE_NODE_CONNECTIONS.to_owned(),
            value: Some(config.allow_private_node_connections.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.test_announce_local_address.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_TEST_ANNOUNCE_LOCAL_ADDRESSES.to_owned(),
            value: Some(config.test_announce_local_address.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.heartbeat_interval.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_INTERVAL.to_owned(),
            value: Some(config.heartbeat_interval.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.heartbeat_threshold.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_THRESHOLD.to_owned(),
            value: Some(config.heartbeat_threshold.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.heartbeat_variance.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_HEARTBEAT_VARIANCE.to_owned(),
            value: Some(config.heartbeat_variance.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.on_chain_confirmations.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_ON_CHAIN_CONFIRMATIONS.to_owned(),
            value: Some(config.on_chain_confirmations.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    if config.network_quality_threshold.is_some() {
        env_vars.push(EnvVar {
            name: constants::HOPRD_NETWORK_QUALITY_THRESHOLD.to_owned(),
            value: Some(config.network_quality_threshold.as_ref().unwrap().to_string()),
            ..EnvVar::default()
        });
    }

    return env_vars;
}

/// Build default environment variables
///
fn build_default_env_var() -> Vec<EnvVar> {
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
