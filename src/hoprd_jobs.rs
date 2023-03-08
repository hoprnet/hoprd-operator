use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, EnvVarSource, KeyToPath, 
    PodSpec, PodTemplateSpec, SecretKeySelector, SecretVolumeSource,
     Volume, VolumeMount, PersistentVolumeClaimVolumeSource, ConfigMapVolumeSource
};
use kube::{Api,  Client, Error, runtime::wait::{await_condition, conditions}};
use kube::api::{ObjectMeta, PostParams};
use std::collections::{BTreeMap};
use crate::hoprd::HoprdSpec;
use crate::model::{Secret as HoprdSecret, OperatorInstance};
use crate::{
    constants,
    utils,
};
use rand::{distributions::Alphanumeric, Rng};

/// Creates a new Job for creating a hoprd node
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `hoprd_name` - Name of the hoprd node
/// - `operator_namespace` - Operator namespace
/// - `hoprd_spec` - Details about the hoprd configuration node
///
pub async fn execute_job_create_node(client: Client, hoprd_name: &str, hoprd_spec: &HoprdSpec, operator_instance: &OperatorInstance) -> Result<Job, Error> {
    let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
    let job_name: String = format!("job-create-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
    let namespace: String = operator_instance.namespace.to_owned();
    let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "create-node".to_owned());
    let secret = hoprd_spec.secret.as_ref().unwrap();


    let command_args: Vec<String> = vec![format!("/app/scripts/create-node.sh {}", secret.secret_name.to_owned())];
    let env_vars: Vec<EnvVar> = build_env_vars(client.clone(), &hoprd_spec, &true, &operator_instance).await;
    let image: String = format!("{}/{}:{}", constants::HOPR_DOCKER_REGISTRY.to_owned(), constants::HOPR_DOCKER_IMAGE_NAME.to_owned(), &hoprd_spec.version.to_owned());
    let volume_mounts: Vec<VolumeMount> = build_volume_mounts(&true).await;
    let volumes: Vec<Volume> = build_volumes(secret, &true, &operator_instance.name.to_owned()).await;
    // Definition of the Job
    let create_node_job: Job = build_job(job_name.to_owned(), namespace, image, labels, command_args, env_vars, volume_mounts, volumes);

    // Create the Job defined above
    println!("[INFO] Launching job '{}'", &job_name.to_owned());
    let api: Api<Job> = Api::namespaced(client.clone(), &operator_instance.namespace);
    api.create(&PostParams::default(), &create_node_job).await?;
    return Ok(await_condition(api, &job_name.to_owned(), conditions::is_job_completed()).await.unwrap().unwrap())
}

/// Creates a new Job for registering hoprd node in Network Registry
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `hoprd_name` - Name of the hoprd node
/// - `operator_namespace` - Operator namespace
/// - `hoprd_spec` - Details about the hoprd configuration node
///
pub async fn execute_job_registering_node(client: Client, hoprd_name: &str, hoprd_spec: &HoprdSpec, operator_instance: &OperatorInstance) -> Result<Job, Error> {
    let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
    let job_name: String = format!("job-register-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
    let namespace: String = operator_instance.namespace.to_owned();
    let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "registe-node".to_owned());
    let secret = hoprd_spec.secret.as_ref().unwrap();
    let command_args = vec!["/app/scripts/register-node.sh".to_owned()];
    let env_vars: Vec<EnvVar> = build_env_vars(client.clone(), &hoprd_spec, &false, &operator_instance).await;
    let image = format!("{}/{}:{}", constants::HOPR_DOCKER_REGISTRY.to_owned(), constants::HOPLI_DOCKER_IMAGE_NAME.to_owned(), &hoprd_spec.version.to_owned());
   
    let volume_mounts: Vec<VolumeMount> = build_volume_mounts(&false).await;
    let volumes: Vec<Volume> = build_volumes(secret, &false, &operator_instance.name.to_owned()).await;
    // Definition of the Job
    let registering_job: Job = build_job(job_name.to_owned(), namespace, image, labels, command_args, env_vars, volume_mounts, volumes);

    // Create the Job defined above
    println!("[INFO] Launching job '{}'", &job_name);
    let api: Api<Job> = Api::namespaced(client.clone(), &operator_instance.namespace.to_owned());
    api.create(&PostParams::default(), &registering_job).await?;
    return Ok(await_condition(api, &job_name.to_owned(), conditions::is_job_completed()).await.unwrap().unwrap())
}

/// Creates a new Job for funding hoprd node
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `hoprd_name` - Name of the hoprd node
/// - `operator_namespace` - Operator namespace
/// - `hoprd_spec` - Details about the hoprd configuration node
///
pub async fn execute_job_funding_node(client: Client, hoprd_name: &str, hoprd_spec: &HoprdSpec, operator_instance: &OperatorInstance) -> Result<Job, Error> {
    let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
    let job_name: String = format!("job-fund-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
    let namespace: String = operator_instance.namespace.to_owned();
    let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
    labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "fund-node".to_owned());
    let secret = hoprd_spec.secret.as_ref().unwrap();
    let command_args = vec!["/app/scripts/fund-node.sh".to_owned()];
    let env_vars: Vec<EnvVar> = build_env_vars(client.clone(), &hoprd_spec, &false, &operator_instance).await;
    let image = format!("{}/{}:{}", constants::HOPR_DOCKER_REGISTRY.to_owned(), constants::HOPLI_DOCKER_IMAGE_NAME.to_owned(), &hoprd_spec.version.to_owned());
   
    let volume_mounts: Vec<VolumeMount> = build_volume_mounts(&false).await;
    let volumes: Vec<Volume> = build_volumes(secret, &false, &operator_instance.name.to_owned()).await;
    // Definition of the Job
    let funding_job: Job = build_job(job_name.to_owned(), namespace, image, labels, command_args, env_vars, volume_mounts, volumes);

    // Create the Job defined above
    println!("[INFO] Launching job '{}'", &job_name);
    let api: Api<Job> = Api::namespaced(client.clone(), &operator_instance.namespace.to_owned());
    api.create(&PostParams::default(), &funding_job).await?;
    return Ok(await_condition(api, &job_name.to_owned(), conditions::is_job_completed()).await.unwrap().unwrap())
}

/// Builds the Job Spec which is similar to all jobs
///
/// # Arguments
/// - `api_secret` - A Secret API  Kubernetes client.
/// - `hoprd_spec` - Details about the hoprd configuration node
/// - `labels` - Labels to be added to the JobSpec
/// - `command_args` - Function which return the command to be executed within the Job
/// - `is_create_node_job` - Whether to job is create node
/// 
fn build_job(job_name: String, namespace: String, image: String, labels: BTreeMap<String, String>, command_args: Vec<String>, env_vars: Vec<EnvVar>, volume_mounts: Vec<VolumeMount>, volumes: Vec<Volume>) -> Job {
    Job {
        metadata: ObjectMeta {
            name: Some(job_name),
            namespace: Some(namespace),
            labels: Some(labels.clone()),
            ..ObjectMeta::default()
        },
        spec: Some(JobSpec {
            template: PodTemplateSpec {
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "hoprd".to_owned(),
                        image: Some(image),
                        image_pull_policy: Some("Always".to_owned()),
                        command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                        args: Some(command_args),
                        env: Some(env_vars),
                        volume_mounts: Some(volume_mounts),
                        resources: utils::build_resource_requirements(&None),
                        ..Container::default()
                    }],
                    volumes: Some(volumes),
                    restart_policy: Some("Never".to_owned()),
                    ..PodSpec::default()
                }),
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..ObjectMeta::default()
                }),
            },
            ..JobSpec::default()
        }),
        ..Job::default()
    }
   
}

/// Builds the struct VolumeMount to be attached into the Container
/// 
/// # Arguments
/// - `is_create_node_job` - Whether to mount specific volumes needed for certain jobs` - Whether to mount extra volumes needed for certain jobs
async fn build_volume_mounts(is_create_node_job: &bool) -> Vec<VolumeMount> {
    let mut volume_mounts = Vec::with_capacity(2);
    if is_create_node_job.to_owned() {
        volume_mounts.push(VolumeMount {
            name: "hopr-repo-volume".to_owned(),
            mount_path: "/app/node_secrets".to_owned(),
            ..VolumeMount::default()
        });
    } else {
        volume_mounts.push(VolumeMount {
            name: "hoprd-identity".to_owned(),
            mount_path: "/app/hoprd-identity".to_owned(),
            ..VolumeMount::default()
        });
    }
        volume_mounts.push(VolumeMount {
            name: "hopr-script-volume".to_owned(),
            mount_path: "/app/scripts".to_owned(),
            ..VolumeMount::default()
        });
    return volume_mounts;
}

/// Builds the struct Volume to be included as part of the PodSpec
/// 
/// # Arguments
/// - `secret` - Secret struct used to build the volume for HOPRD_IDENTITY path
/// - `is_create_node_job` - Whether to mount specific volumes needed for certain jobs
async fn build_volumes(secret: &HoprdSecret, is_create_node_job: &bool, operator_name: &str) -> Vec<Volume> {
    let mut volumes = Vec::with_capacity(2);

    if is_create_node_job.to_owned() {
        
        volumes.push(Volume {
            name: "hopr-repo-volume".to_owned(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: operator_name.to_owned(),
                read_only: Some(false)
            }),
            ..Volume::default()
        });
    } else {
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
    }
    let configmap_name = format!("{operator_name}-scripts");
        volumes.push(Volume {
            name: "hopr-script-volume".to_owned(),
            config_map: Some(ConfigMapVolumeSource {
                name: Some(configmap_name.to_owned()),
                default_mode: Some(0o550),
                ..ConfigMapVolumeSource::default()
            }),
            ..Volume::default()
        });
    return volumes;
}

///Build struct environment variable
///
pub async fn build_env_vars(client: Client, hoprd_spec: &HoprdSpec, is_create_node_job: &bool, operator_instance: &OperatorInstance) -> Vec<EnvVar> {
    let mut env_vars = Vec::with_capacity(2); 
    let secret = hoprd_spec.secret.as_ref().unwrap();
    if ! is_create_node_job {
        env_vars.push(EnvVar {
            name: "IDENTITY_PASSWORD".to_owned(),
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
        let labels = utils::get_resource_kinds(client, utils::ResourceType::Secret, utils::ResourceKind::Labels, &secret.secret_name.to_owned(), &operator_instance.namespace).await;
        if labels.contains_key(constants::LABEL_NODE_ADDRESS) {
            let node_address: String = labels.get_key_value(constants::LABEL_NODE_ADDRESS).unwrap().1.parse().unwrap();
            env_vars.push(EnvVar {
                name: constants::HOPRD_ADDRESS.to_owned(),
                value: Some(node_address.to_owned()),
                ..EnvVar::default()
            });
        }

        if labels.contains_key(constants::LABEL_NODE_PEER_ID) {
            let node_peer_id: String = labels.get_key_value(constants::LABEL_NODE_PEER_ID).unwrap().1.parse().unwrap();
            env_vars.push(EnvVar {
                name: constants::HOPRD_PEER_ID.to_owned(),
                value: Some(node_peer_id.to_owned()),
                ..EnvVar::default()
            });
        }
    }
    env_vars.push(EnvVar {
        name: constants::HOPRD_ENVIRONMENT.to_owned(),
        value: Some(hoprd_spec.environment_name.to_owned()),
        ..EnvVar::default()
    });

    env_vars.push(EnvVar {
        name: constants::HOPR_PRIVATE_KEY.to_owned(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: constants::HOPR_PRIVATE_KEY.to_owned(),
                name: Some(operator_instance.name.to_owned()),
                ..SecretKeySelector::default()
            }),
            ..EnvVarSource::default()
        }),
        ..EnvVar::default()
    });
    return env_vars;
}
