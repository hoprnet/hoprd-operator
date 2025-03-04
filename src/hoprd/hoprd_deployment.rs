use crate::hoprd::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_hoprd::identity_hoprd_resource::IdentityHoprd;
use crate::identity_pool::identity_pool_resource::IdentityPool;
use crate::model::Error;
use crate::{context_data::ContextData, hoprd::hoprd_deployment};
use base64::{engine::general_purpose, Engine as _};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::{
    constants,
    hoprd::hoprd_resource::{Hoprd, HoprdSpec},
    utils,
};
use futures::StreamExt;
use k8s_openapi::api::batch::v1::JobSpec;
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource, SecretEnvSource, EnvFromSource, PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec, Probe, SecretKeySelector, TCPSocketAction, Volume, VolumeMount
};
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentSpec, DeploymentStrategy},
    batch::v1::{Job, JobStatus},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, OwnerReference};
use kube::api::{DeleteParams, ObjectMeta, Patch, PatchParams, PostParams, WatchEvent, WatchParams};
use kube::runtime::wait::{await_condition, conditions};
use kube::{Api, Client, Resource, ResourceExt};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{error, info};

/// Creates a new deployment for running the hoprd node,
///
/// # Arguments
/// - `client` - A Kubernetes client to create the deployment with.
/// - `hoprd` - Details about the hoprd configuration node
///
pub async fn create_deployment(context_data: Arc<ContextData>, hoprd: &Hoprd, identity_hoprd: &IdentityHoprd, hoprd_host: &str, starting_port: u16, last_port: u16) -> Result<Deployment, kube::Error> {
    let namespace: String = hoprd.namespace().unwrap();
    let name: String = hoprd.name_any();
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).unwrap()]);
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await.unwrap();
    let bucket_name= context_data.config.bucket_name.to_owned().unwrap();
    let instance_name = context_data.config.instance.name.to_owned();

    let mut labels: BTreeMap<String, String> = utils::common_lables(instance_name.to_owned(), Some(name.to_owned()), Some("node".to_owned()));
    labels.insert(constants::LABEL_NODE_NETWORK.to_owned(), identity_pool.spec.network.clone());
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool.name_any());
    labels.insert(constants::LABEL_NODE_NATIVE_ADDRESS.to_owned(), identity_hoprd.spec.native_address.to_owned());
    labels.insert(constants::LABEL_NODE_PEER_ID.to_owned(), identity_hoprd.spec.peer_id.to_owned());
    labels.insert(constants::LABEL_NODE_SAFE_ADDRESS.to_owned(), identity_hoprd.spec.safe_address.to_owned());
    labels.insert(constants::LABEL_NODE_MODULE_ADDRESS.to_owned(), identity_hoprd.spec.module_address.to_owned());

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
        spec: Some(build_deployment_spec(labels, &hoprd.spec, identity_pool, identity_hoprd, &hoprd_host, starting_port, last_port, bucket_name, &instance_name).await),
        ..Deployment::default()
    };

    // Create the deployment defined above
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), &namespace);
    let deployment = api.create(&PostParams::default(), &deployment).await?;
    info!("Deployment {} created successfully", name.to_owned());
    Ok(deployment)
}

pub async fn build_deployment_spec(
    labels: BTreeMap<String, String>,
    hoprd_spec: &HoprdSpec,
    identity_pool: IdentityPool,
    identity_hoprd: &IdentityHoprd,
    hoprd_host: &str,
    starting_port: u16,
    last_port: u16,
    bucket_name: String,
    instance_name: &String
) -> DeploymentSpec {
    let image = format!(
        "{}/{}:{}",
        constants::HOPR_DOCKER_REGISTRY.to_owned(),
        constants::HOPR_DOCKER_IMAGE_NAME.to_owned(),
        &hoprd_spec.version.to_owned()
    );
    let replicas: i32 = if hoprd_spec.enabled.unwrap_or(true) { 1 } else { 0 };
    let resources = Some(HoprdDeploymentSpec::get_resource_requirements(hoprd_spec.deployment.clone()));
    let liveness_probe = HoprdDeploymentSpec::get_liveness_probe(hoprd_spec.deployment.clone());
    let readiness_probe = HoprdDeploymentSpec::get_readiness_probe(hoprd_spec.deployment.clone());
    let startup_probe = HoprdDeploymentSpec::get_startup_probe(hoprd_spec.deployment.clone());
    let volume_mounts: Option<Vec<VolumeMount>> = build_volume_mounts();
    let hoprd_host_port = format!("{}:{}", hoprd_host, starting_port);
    let session_port_range = format!("{}:{}", starting_port + 1, last_port - 1);
    let encoded_configuration = general_purpose::STANDARD.encode(&hoprd_spec.config);


    let init_args = Some(vec![
        format!(
        r#"
        set -x;
        set -e;
        if ! ls /app/hoprd-db/db/hopr_logs.db* 1> /dev/null 2>&1; then
            apk add --no-cache curl tar;
            curl -sf --retry 3 "https://storage.googleapis.com/{}/hopr_logs.tar.gz" -o /tmp/hopr_logs.tar.gz;
            tar xf /tmp/hopr_logs.tar.gz -C /;
            rm -f /tmp/hopr_logs.tar.gz;
        fi;
        echo $HOPRD_IDENTITY_FILE | base64 -d > /app/hoprd-identity/.hopr-id;
        echo $HOPRD_CONFIGURATION | base64 -d > /app/hoprd-identity/config.yaml
        "#,
        bucket_name
    )]);


    DeploymentSpec {
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
                init_containers: Some(vec![Container {
                    name: "init".to_owned(),
                    image: Some("alpine".to_owned()),
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
                    env_from: Some(vec![EnvFromSource {
                        secret_ref: Some(SecretEnvSource {
                            name: Some("hoprd-default-env".to_string()),
                            ..SecretEnvSource::default()
                        }),
                        ..EnvFromSource::default()
                    }]),
                    command: Some(vec!["sh".to_string(), "-c".to_string()]),
                    args: init_args,
                    volume_mounts: volume_mounts.to_owned(),
                    ..Container::default()
                }]),
                containers: vec![Container {
                    name: "hoprd".to_owned(),
                    image: Some(image),
                    image_pull_policy: Some("Always".to_owned()),
                    ports: Some(build_ports(starting_port.into(), last_port.into())),
                    env: Some(build_env_vars(&identity_pool, identity_hoprd, &hoprd_host_port, hoprd_spec, session_port_range)),
                    liveness_probe,
                    readiness_probe,
                    startup_probe,
                    volume_mounts,
                    resources,
                    ..Container::default()
                }, metrics_container(&identity_pool)],
                volumes: Some(build_volumes(&identity_hoprd.name_any(), instance_name).await),
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

pub async fn modify_deployment(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str, hoprd_spec: &HoprdSpec, identity_hoprd: &IdentityHoprd) -> Result<(), kube::Error> {
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), namespace);
    let deployment = api.get(deployment_name).await.unwrap();
    let hoprd_host_port = deployment
        .spec
        .clone()
        .unwrap()
        .template
        .spec
        .unwrap()
        .containers
        .first()
        .as_ref()
        .unwrap()
        .env
        .as_ref()
        .unwrap()
        .iter()
        .find(|&env_var| env_var.name.eq(&constants::HOPRD_HOST.to_owned()))
        .unwrap()
        .value
        .as_ref()
        .unwrap()
        .to_owned();
    let hoprd_host = *hoprd_host_port.split(':').collect::<Vec<&str>>().get(0).unwrap();
    let starting_port = hoprd_host_port.split(':').collect::<Vec<&str>>().get(1).unwrap().to_string().parse::<u16>().unwrap();
    let ports_allocation = hoprd_spec.ports_allocation.clone().unwrap_or(constants::HOPRD_PORTS_ALLOCATION);
    let last_port = starting_port + ports_allocation;
    let identity_pool: IdentityPool = identity_hoprd.get_identity_pool(context_data.client.clone()).await.unwrap();
    let bucket_name= context_data.config.bucket_name.to_owned().unwrap();
    let instance_name = context_data.config.instance.name.to_owned();
    let spec = build_deployment_spec(deployment.labels().to_owned(), hoprd_spec, identity_pool, identity_hoprd, &hoprd_host, starting_port, last_port, bucket_name, &instance_name).await;
    let patch = &Patch::Merge(json!({ "spec": spec }));
    api.patch(deployment_name, &PatchParams::default(), patch).await.unwrap();
    Ok(())
}

pub fn metrics_container(identity_pool: &IdentityPool) -> Container {

    let args = Some(vec![
        vec![
            "set -x",
            "apt update && apt install --no-install-recommends -y curl lighttpd",
            "mkdir -p /var/www/cgi-bin",
            "echo \"HOPRD_API_TOKEN=${HOPRD_API_TOKEN}\" > /etc/environment",
            "cp /app/hoprd-metrics/lighttpd.conf /etc/lighttpd/lighttpd.conf",
            "cp /app/hoprd-metrics/metrics.sh /var/www/cgi-bin/metrics.sh",
            "service lighttpd start",
            "while true; do sleep 15; done",
        ].join("\n")
    ]);


    Container {
        name: "metrics".to_owned(),
        image: Some("debian:stable-slim".to_owned()),
        ports: Some(vec![ContainerPort {
            container_port: 8080,
            name: Some("metrics".to_owned()),
            protocol: Some("TCP".to_owned()),
            ..ContainerPort::default()
        }]),
        env: Some(vec![ env_var_hoprd_api_token(identity_pool)]),
        command: Some(vec!["sh".to_string(), "-c".to_string()]),
        args,
        volume_mounts: Some(vec![
            VolumeMount {
                name: "hoprd-metrics".to_string(),
                mount_path: "/app/hoprd-metrics".to_string(),
                ..Default::default()
            },
        ]),
        readiness_probe: Some( Probe {
            tcp_socket: Some( TCPSocketAction {
                port: IntOrString::Int(8080),
                ..Default::default()
            }),
            initial_delay_seconds: Some(15),
            period_seconds: Some(10),
            failure_threshold: Some(6),
            ..Probe::default()
        }),
         ..Container::default()
    }
}


/// Deletes an existing deployment.
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

pub async fn delete_database(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str) -> Result<(), Error> {
    let api: Api<Deployment> = Api::namespaced(context_data.client.clone(), namespace);
    let deployment = api.get(deployment_name).await.unwrap();
    let spec = deployment.spec.as_ref().unwrap();
    let volumes = spec.template.spec.clone().unwrap().volumes.unwrap().clone();
    let volume: &Volume = volumes.iter().find(|&volume| volume.name.eq("hoprd-db")).unwrap();
    let pvc_name = volume.persistent_volume_claim.as_ref().unwrap().claim_name.clone();
    info!("Scaling down deployment {} in namespace {}", deployment_name, namespace);
    let patch = Patch::Merge(json!({ "spec": { "replicas": 0 } }));
    match api.patch(&deployment_name, &PatchParams::default(), &patch).await {
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
    match api.patch(&deployment_name, &PatchParams::default(), &patch).await {
        Ok(_) => {}
        Err(error) => error!("Could not scale up deployment {deployment_name}: {:?}", error),
    };
    Ok(())
}

pub async fn job_delete_database(context_data: Arc<ContextData>, deployment_name: &str, namespace: &str, pvc_name: &str) -> Result<(), Error> {
    let api: Api<Job> = Api::namespaced(context_data.client.clone(), namespace);
    let rng = rand::thread_rng();
    let suffix: String = rng.sample_iter(&Alphanumeric).take(10).map(char::from).collect();
    let command = "rm -rf /app/hoprd-db/db/hopr_index.db* /app/hoprd-db/db/hopr_logs.db*".to_string();

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
                        image: Some("debian:stable".to_string()),
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
    ])
}

/// Builds the struct Volume to be included as part of the PodSpec
///
/// # Arguments
/// - `secret` - Secret struct used to build the volume for HOPRD_IDENTITY path
async fn build_volumes(pvc_name: &String, instance_name: &String) -> Vec<Volume> {
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
            name: "hoprd-metrics".to_owned(),
            config_map: Some(ConfigMapVolumeSource {
                name: Some(format!("{}-metrics", instance_name).to_owned()),
                ..ConfigMapVolumeSource::default()
            }),
            ..Volume::default()
        },
    ]
}

/// Build struct ContainerPort
fn build_ports(starting_port: i32, last_port: i32) -> Vec<ContainerPort> {
    let port_range = (last_port - starting_port - 1) as usize;
    let mut ports: Vec<ContainerPort> = Vec::with_capacity(2 + port_range * 2);
    ports.push(ContainerPort {
        container_port: 3001,
        name: Some("api".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    ports.push(ContainerPort {
        container_port: 8080,
        name: Some("health".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    ports.push(ContainerPort {
        container_port: starting_port,
        name: Some("p2p-tcp".to_owned()),
        protocol: Some("TCP".to_owned()),
        ..ContainerPort::default()
    });
    ports.push(ContainerPort {
        container_port: starting_port,
        name: Some("p2p-udp".to_owned()),
        protocol: Some("UDP".to_owned()),
        ..ContainerPort::default()
    });
    for session_port in starting_port + 1..last_port {
        ports.push(ContainerPort {
            container_port: session_port,
            name: Some(format!("sessiont-{}", session_port)),
            protocol: Some("TCP".to_owned()),
            ..ContainerPort::default()
        });
        ports.push(ContainerPort {
            container_port: session_port,
            name: Some(format!("sessionu-{}", session_port)),
            protocol: Some("UDP".to_owned()),
            ..ContainerPort::default()
        });
    }
    ports
}

///Build struct environment variable
///
fn build_env_vars(identity_pool: &IdentityPool, identity_hoprd: &IdentityHoprd, hoprd_host: &String, hoprd_spec: &HoprdSpec, session_port_range: String) -> Vec<EnvVar> {
    let mut env_vars = build_secret_env_var(identity_pool);
    env_vars.extend_from_slice(&build_crd_env_var(identity_pool, identity_hoprd));
    env_vars.extend_from_slice(&build_default_env_var(hoprd_host));
    env_vars.extend_from_slice(&HoprdDeploymentSpec::get_environment_variables(hoprd_spec.deployment.to_owned()));
    env_vars.push(EnvVar {
        name: constants::HOPRD_API.to_owned(),
        value: Some("1".to_owned()),
        ..EnvVar::default()
    });
    env_vars.push(EnvVar {
        name: constants::HOPRD_SESSION_PORT_RANGE.to_owned(),
        value: Some(session_port_range),
        ..EnvVar::default()
    });
    env_vars
}

fn env_var_hoprd_api_token(identity_pool: &IdentityPool) -> EnvVar{
        EnvVar {
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
        }
}

/// Build environment variables from secrets
///
/// # Arguments
/// - `secret` - Secret struct used to build HOPRD_PASSWORD and HOPRD_API_TOKEN
fn build_secret_env_var(identity_pool: &IdentityPool) -> Vec<EnvVar> {
    vec![
        EnvVar {
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
        },
        env_var_hoprd_api_token(identity_pool),
    ]
}

/// Build environment variables from CRD
///
/// # Arguments
/// - `hoprd_spec` - Details about the hoprd configuration node
fn build_crd_env_var(identity_pool: &IdentityPool, identity_hoprd: &IdentityHoprd) -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: constants::HOPRD_CONFIGURATION_FILE_PATH.to_owned(),
            value: Some("/app/hoprd-identity/config.yaml".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_NETWORK.to_owned(),
            value: Some(identity_pool.spec.network.to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_SAFE_ADDRESS.to_owned(),
            value: Some(identity_hoprd.spec.safe_address.to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_MODULE_ADDRESS.to_owned(),
            value: Some(identity_hoprd.spec.module_address.to_owned()),
            ..EnvVar::default()
        },
    ]
}

/// Build default environment variables
///
fn build_default_env_var(hoprd_host: &String) -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: constants::HOPRD_IDENTITY.to_owned(),
            value: Some("/app/hoprd-identity/.hopr-id".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_DATA.to_owned(),
            value: Some("/app/hoprd-db".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_HOST.to_owned(),
            value: Some(hoprd_host.to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_API_HOST.to_owned(),
            value: Some("0.0.0.0".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_HEALTH_CHECK.to_owned(),
            value: Some("true".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_HEALTH_CHECK_HOST.to_owned(),
            value: Some("0.0.0.0".to_owned()),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_DEFAULT_SESSION_LISTEN_HOST.to_owned(),
            value: Some("0.0.0.0:0".to_owned()),
            ..EnvVar::default()
        },


    ]
}
