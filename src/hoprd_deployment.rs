use crate::context_data::ContextData;
use crate::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_hoprd::IdentityHoprd;
use crate::identity_pool::IdentityPool;
use crate::model::Error;
use crate::operator_config::IngressConfig;
use base64::{Engine as _, engine::general_purpose};
use crate::{
    constants,
    hoprd::{Hoprd, HoprdSpec},
    utils,
};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource,
    PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec, ResourceRequirements,
    SecretKeySelector, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, OwnerReference};
use kube::api::{DeleteParams, ObjectMeta, Patch, PatchParams, PostParams};
use kube::runtime::wait::{await_condition, conditions};
use kube::{Api, Client, Resource, ResourceExt};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::info;

/// Creates a new deployment for running the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `hoprd` - Details about the hoprd configuration node
///
pub async fn create_deployment(context_data: Arc<ContextData>, hoprd: &Hoprd, identity_hoprd: &IdentityHoprd, p2p_port: i32, ingress_config: IngressConfig) -> Result<Deployment, kube::Error> {
    let namespace: String = hoprd.namespace().unwrap();
    let name: String = hoprd.name_any();
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).unwrap()]);
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await.unwrap();

    let mut labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(), Some(name.to_owned()), Some("node".to_owned()));
    labels.insert(constants::LABEL_NODE_NETWORK.to_owned(),identity_pool.spec.network.clone());
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool.name_any());
    labels.insert(constants::LABEL_NODE_NATIVE_ADDRESS.to_owned(), identity_hoprd.spec.native_address.to_owned());
    labels.insert(constants::LABEL_NODE_PEER_ID.to_owned(), identity_hoprd.spec.peer_id.to_owned());
    labels.insert(constants::LABEL_NODE_SAFE_ADDRESS.to_owned(), identity_hoprd.spec.safe_address.to_owned());
    labels.insert(constants::LABEL_NODE_MODULE_ADDRESS.to_owned(),identity_hoprd.spec.module_address.to_owned());
    let hoprd_host = format!("{}:{}", ingress_config.public_ip.unwrap(), p2p_port);

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
        spec: Some(
            build_deployment_spec(
                labels,
                &hoprd.spec,
                identity_pool,
                identity_hoprd,
                &hoprd_host,
            )
            .await,
        ),
        ..Deployment::default()
    };

    // Create the deployment defined above
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), &namespace);
    api.create(&PostParams::default(), &deployment).await
}

pub async fn build_deployment_spec(labels: BTreeMap<String, String>, hoprd_spec: &HoprdSpec, identity_pool: IdentityPool, identity_hoprd: &IdentityHoprd, hoprd_host: &String) -> DeploymentSpec {
    let image = format!("{}/{}:{}", constants::HOPR_DOCKER_REGISTRY.to_owned(), constants::HOPR_DOCKER_IMAGE_NAME.to_owned(), &hoprd_spec.version.to_owned());
    let replicas: i32 = if hoprd_spec.enabled.unwrap_or(true) { 1 } else { 0 };
    let resources: Option<ResourceRequirements> = Some(
        HoprdDeploymentSpec::get_resource_requirements(hoprd_spec.deployment.clone()),
    );
    let liveness_probe = HoprdDeploymentSpec::get_liveness_probe(hoprd_spec.supported_release, hoprd_spec.deployment.clone());
    let readiness_probe = HoprdDeploymentSpec::get_readiness_probe(hoprd_spec.supported_release, hoprd_spec.deployment.clone());
    let startup_probe = HoprdDeploymentSpec::get_startup_probe(hoprd_spec.supported_release, hoprd_spec.deployment.clone());
    let volume_mounts: Option<Vec<VolumeMount>> = build_volume_mounts();
    let port = hoprd_host.split(':').collect::<Vec<&str>>().get(1).unwrap().to_string().parse::<i32>().unwrap();
    let encoded_configuration = general_purpose::STANDARD.encode(&hoprd_spec.config);

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
                    init_containers: Some(vec![Container {
                        name: "init".to_owned(),
                        image: Some("alpine".to_owned()),
                        env: Some(vec![EnvVar {
                            name: constants::HOPRD_IDENTITY_FILE.to_owned(),
                            value: Some(identity_hoprd.spec.identity_file.to_owned()),
                            ..EnvVar::default()
                        },
                        EnvVar {
                            name: constants::HOPRD_CONFIGURATION.to_owned(),
                            value: Some(encoded_configuration),
                            ..EnvVar::default()
                        }]),
                        command: Some(vec!["/bin/sh".to_owned(), "-c".to_owned()]),
                        args: Some(vec![format!("{} && {}",
                            "echo $HOPRD_IDENTITY_FILE | base64 -d > /app/hoprd-identity/.hopr-id", 
                            "echo $HOPRD_CONFIGURATION | base64 -d > /app/hoprd-identity/config.yaml")
                        ]),
                        volume_mounts: volume_mounts.to_owned(),
                        ..Container::default()
                    }]),
                    containers: vec![Container {
                        name: "hoprd".to_owned(),
                        image: Some(image),
                        image_pull_policy: Some("Always".to_owned()),
                        ports: Some(build_ports(port)),
                        env: Some(build_env_vars(&identity_pool, identity_hoprd, hoprd_host)),
                        // command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                        // args: Some(vec!["sleep 99999999".to_owned()]),
                        liveness_probe,
                        readiness_probe,
                        startup_probe,
                        volume_mounts,
                        resources,
                        ..Container::default()
                    }],
                    volumes: Some(build_volumes(&identity_hoprd.name_any()).await),
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

pub async fn modify_deployment(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str, hoprd_spec: &HoprdSpec, identity_hoprd: &IdentityHoprd) -> Result<Deployment, kube::Error> {
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), namespace);
    let deployment = api.get(deployment_name).await.unwrap();
    let hoprd_host = deployment.spec.clone().unwrap().template.spec.unwrap().containers.first().as_ref().unwrap()
        .env.as_ref().unwrap().iter()
        .find(|&env_var| env_var.name.eq(&constants::HOPRD_HOST.to_owned())).unwrap()
        .value.as_ref().unwrap().to_owned();
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await.unwrap();
    let spec = build_deployment_spec(deployment.labels().to_owned(), hoprd_spec, identity_pool, identity_hoprd, &hoprd_host).await;
    let patch = &Patch::Merge(json!({ "spec": spec }));
    api.patch(deployment_name, &PatchParams::default(), patch).await
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
    if let Some(deployment) = api.get_opt(name).await? {
        let uid = deployment.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("Deployment {name} successfully deleted"))
    } else {
        Ok(info!("Deployment {name} in namespace {namespace} about to delete not found"))
    }
}

/// Builds the struct VolumeMount to be attached into the Container
fn build_volume_mounts() -> Option<Vec<VolumeMount>> {
    Some(vec![VolumeMount {
        name: "hoprd-identity".to_owned(),
        mount_path: "/app/hoprd-identity".to_owned(),
        ..VolumeMount::default()
    }, VolumeMount {
        name: "hoprd-db".to_owned(),
        mount_path: "/app/hoprd-db".to_owned(),
        ..VolumeMount::default()
    }])
}

/// Builds the struct Volume to be included as part of the PodSpec
///
/// # Arguments
/// - `secret` - Secret struct used to build the volume for HOPRD_IDENTITY path
async fn build_volumes(pvc_name: &String) -> Vec<Volume> {
    vec![Volume {
        name: "hoprd-identity".to_owned(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Volume::default()
    },Volume {
        name: "hoprd-db".to_owned(),
        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
            claim_name: pvc_name.to_owned(),
            read_only: Some(false),
        }),
        ..Volume::default()
    }]
}

/// Build struct ContainerPort
fn build_ports(p2p_port: i32) -> Vec<ContainerPort> {
    vec![ContainerPort {
        container_port: 3001,
        name: Some("api".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    }, ContainerPort {
        container_port: 8080,
        name: Some("heatlh".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    }, ContainerPort {
        container_port: p2p_port,
        name: Some("p2p-tcp".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    }, ContainerPort {
        container_port: p2p_port,
        name: Some("p2p-udp".to_owned()),
        protocol: Some("UDP".to_owned()),
        ..ContainerPort::default()
    }]
}

///Build struct environment variable
///
fn build_env_vars(
    identity_pool: &IdentityPool,
    identity_hoprd: &IdentityHoprd,
    hoprd_host: &String,
) -> Vec<EnvVar> {
    let mut env_vars = build_secret_env_var(identity_pool);
    env_vars.extend_from_slice(&build_crd_env_var(identity_pool, identity_hoprd));
    env_vars.extend_from_slice(&build_default_env_var(hoprd_host));
    env_vars
}

/// Build environment variables from secrets
///
/// # Arguments
/// - `secret` - Secret struct used to build HOPRD_PASSWORD and HOPRD_API_TOKEN
fn build_secret_env_var(identity_pool: &IdentityPool) -> Vec<EnvVar> {
    vec![EnvVar {
        name: constants::HOPRD_PASSWORD.to_owned(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: constants::IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY.to_owned(),
                name: Some(identity_pool.spec.secret_name.to_owned()),
                ..SecretKeySelector::default()
            }),
            ..EnvVarSource::default()
        }),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_API_TOKEN.to_owned(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: constants::IDENTITY_POOL_API_TOKEN_REF_KEY.to_owned(),
                name: Some(identity_pool.spec.secret_name.to_owned()),
                ..SecretKeySelector::default()
            }),
            ..EnvVarSource::default()
        }),
        ..EnvVar::default()
    }]
}

/// Build environment variables from CRD
///
/// # Arguments
/// - `hoprd_spec` - Details about the hoprd configuration node
fn build_crd_env_var(identity_pool: &IdentityPool, identity_hoprd: &IdentityHoprd) -> Vec<EnvVar> {
    vec![EnvVar {
        name: constants::HOPRD_CONFIGURATION_FILE_PATH.to_owned(),
        value: Some("/app/hoprd-identity/config.yaml".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_NETWORK.to_owned(),
        value: Some(identity_pool.spec.network.to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_SAFE_ADDRESS.to_owned(),
        value: Some(identity_hoprd.spec.safe_address.to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_MODULE_ADDRESS.to_owned(),
        value: Some(identity_hoprd.spec.module_address.to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_ANNOUNCE.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    }]

}

/// Build default environment variables
///
fn build_default_env_var(hoprd_host: &String) -> Vec<EnvVar> {
    vec![EnvVar {
        name: "DEBUG".to_owned(),
        value: Some("hopr*".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_IDENTITY.to_owned(),
        value: Some("/app/hoprd-identity/.hopr-id".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_DATA.to_owned(),
        value: Some("/app/hoprd-db".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_HOST.to_owned(),
        value: Some(hoprd_host.to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_API.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_API_HOST.to_owned(),
        value: Some("0.0.0.0".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_INIT.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_HEALTH_CHECK.to_owned(),
        value: Some("true".to_owned()),
        ..EnvVar::default()
    }, EnvVar {
        name: constants::HOPRD_HEALTH_CHECK_HOST.to_owned(),
        value: Some("0.0.0.0".to_owned()),
        ..EnvVar::default()
    }]
}
