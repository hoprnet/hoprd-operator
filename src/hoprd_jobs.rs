use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, EnvVarSource, KeyToPath, 
    PodSpec, PodTemplateSpec, SecretKeySelector, SecretVolumeSource,
     Volume, VolumeMount, ConfigMapVolumeSource, EmptyDirVolumeSource
};
use tracing::{info};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::{ResourceExt};
use kube::{Api,  Client, runtime::wait::{await_condition, conditions}};
use kube::api::{ObjectMeta, PostParams};
use std::collections::{BTreeMap};
use crate::hoprd::Hoprd;
use crate::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::model::{HoprdSecret, Error};
use crate::operator_config::{OperatorConfig};
use crate::{
    constants,
    utils,
};
use rand::{distributions::Alphanumeric, Rng};


pub struct HoprdJob {
    client: Client,
    config: OperatorConfig,
    hoprd: Hoprd

}


impl HoprdJob {

    pub fn new(client: Client,config: OperatorConfig, hoprd: Hoprd) -> Self {
        Self { client, config, hoprd }
    }

    /// Creates a new Job for creating a hoprd node
    ///
    /// # Arguments
    /// - `hoprd` - Hoprd node
    /// - `owner_references` - Secret reference that owns this job execution
    ///
    pub async fn execute_job_create_node(&self, hoprd_secret: &HoprdSecret, owner_references: Option<Vec<OwnerReference>>) -> Result<(), Error> {
        let hoprd_name = &self.hoprd.name_any();
        let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
        let job_name: String = format!("job-create-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
        let namespace: String = self.config.instance.namespace.clone();
        let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "create-node".to_owned());

        let create_node_args: Vec<String> = vec!["/app/scripts/create-identity.sh".to_owned()];
        let create_secret_args: Vec<String> = vec!["/app/scripts/create-secret.sh".to_owned()];
        let mut env_vars: Vec<EnvVar> = self.build_env_vars(&hoprd_secret, &true).await;
        env_vars.push(EnvVar {
            name: constants::SECRET_NAME.to_owned(),
            value: Some(hoprd_secret.secret_name.to_owned()),
            ..EnvVar::default()
        });
        let volume_mounts: Vec<VolumeMount> = self.build_volume_mounts(&true).await;
        let volumes: Vec<Volume> = self.build_volumes(hoprd_secret, &true).await;
        // Definition of the Job
        let create_node_job: Job = Job {
            metadata: ObjectMeta {
                name: Some(job_name.to_owned()),
                namespace: Some(namespace),
                owner_references,
                labels: Some(labels.clone()),
                ..ObjectMeta::default()
            },
            spec: Some(JobSpec {
                parallelism: Some(1),
                completions: Some(1),
                backoff_limit: Some(1),
                active_deadline_seconds: Some(constants::OPERATOR_JOB_TIMEOUT.try_into().unwrap()),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        init_containers: Some(vec![Container {
                            name: "hopli".to_owned(),
                            image: Some(self.config.hopli_image.to_owned()),
                            image_pull_policy: Some("Always".to_owned()),
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: Some(create_node_args),
                            env: Some(env_vars.to_owned()),
                            volume_mounts: Some(volume_mounts.to_owned()),
                            resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                            ..Container::default()
                        }]),
                        containers: vec![Container {
                            name: "kubectl".to_owned(),
                            image: Some("registry.hub.docker.com/bitnami/kubectl:1.24".to_owned()),
                            image_pull_policy: Some("Always".to_owned()),
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: Some(create_secret_args),
                            env: Some(env_vars),
                            volume_mounts: Some(volume_mounts),
                            resources:Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                            ..Container::default()
                        }],
                        service_account_name: Some(self.config.instance.name.to_owned()),
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
        };

        // Create the Job defined above
        info!("Job {} started", &job_name.to_owned());
        let api: Api<Job> = Api::namespaced(self.client.clone(), &self.config.instance.namespace);
        api.create(&PostParams::default(), &create_node_job).await.unwrap();
        let job_completed = await_condition(api, &job_name, conditions::is_job_completed());
        match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT), job_completed).await {
            Ok(_) => Ok(info!("Job {} completed successfully", &job_name.to_owned())),
            Err(_error) => {
                Err(Error::JobExecutionError(format!(" Job execution for {} failed", &job_name.to_owned()).to_owned()))
            }
        }
    }

    /// Creates a new Job for registering hoprd node in Network Registry
    ///
    /// # Arguments
    /// - `hoprd` - Hoprd node
    /// - `owner_references` - Secret reference that owns this job execution
    ///
    pub async fn execute_job_registering_node(&self, hoprd_secret: &HoprdSecret, owner_references: Option<Vec<OwnerReference>>) -> Result<(), Error> {
        let hoprd_name = &self.hoprd.name_any();
        let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
        let job_name: String = format!("job-register-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
        let namespace: String = self.config.instance.namespace.to_owned();
        let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "registe-node".to_owned());
        let command_args = vec!["/app/scripts/register-node.sh".to_owned()];
        let env_vars: Vec<EnvVar> = self.build_env_vars(&hoprd_secret, &false).await;
    
        let volume_mounts: Vec<VolumeMount> = self.build_volume_mounts(&false).await;
        let volumes: Vec<Volume> = self.build_volumes(hoprd_secret, &false).await;
        // Definition of the Job
        let registering_job: Job = self.build_job(job_name.to_owned(), namespace, owner_references, self.config.hopli_image.to_owned(), labels, command_args, env_vars, volume_mounts, volumes);

        // Create the Job defined above
        info!("Job {} started", &job_name.to_owned());
        let api: Api<Job> = Api::namespaced(self.client.clone(), &self.config.instance.namespace.to_owned());
        api.create(&PostParams::default(), &registering_job).await.unwrap();
        let job_completed = await_condition(api, &job_name, conditions::is_job_completed());
        match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT), job_completed).await {
            Ok(_) => Ok(info!("Job {} completed successfully", &job_name.to_owned())),
            Err(_error) => {
                Err(Error::JobExecutionError(format!(" Job execution for {} failed", &job_name.to_owned()).to_owned()))
            }
        }
    }

    /// Creates a new Job for funding hoprd node
    ///
    /// # Arguments
    /// - `hoprd` - Hoprd node
    /// - `owner_references` - Secret reference that owns this job execution
    ///
    pub async fn execute_job_funding_node(&self, hoprd_secret: &HoprdSecret, owner_references: Option<Vec<OwnerReference>>) -> Result<(), Error> {
        let hoprd_name = &self.hoprd.name_any();
        let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
        let job_name: String = format!("job-fund-{}-{}",&hoprd_name.to_owned(),&random_string.to_ascii_lowercase());
        let namespace: String = self.config.instance.namespace.to_owned();
        let mut labels: BTreeMap<String, String> = utils::common_lables(&hoprd_name.to_owned());
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "fund-node".to_owned());
        let command_args = vec!["/app/scripts/fund-node.sh".to_owned()];
        let env_vars: Vec<EnvVar> = self.build_env_vars(&hoprd_secret, &false).await;

    
        let volume_mounts: Vec<VolumeMount> = self.build_volume_mounts(&false).await;
        let volumes: Vec<Volume> = self.build_volumes(hoprd_secret, &false).await;
        // Definition of the Job
        let funding_job: Job = self.build_job(job_name.to_owned(), namespace, owner_references, self.config.hopli_image.to_owned(), labels, command_args, env_vars, volume_mounts, volumes);

        // Create the Job defined above
        info!("Job {} started", &job_name.to_owned());
        let api: Api<Job> = Api::namespaced(self.client.clone(), &self.config.instance.namespace.to_owned());
        api.create(&PostParams::default(), &funding_job).await.unwrap();
        let job_completed = await_condition(api, &job_name, conditions::is_job_completed());
        match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT), job_completed).await {
            Ok(_) => Ok(info!("Job {} completed successfully", &job_name.to_owned())),
            Err(_error) => {
                Err(Error::JobExecutionError(format!(" Job execution for {} failed", &job_name.to_owned()).to_owned()))
            }
        }
    }

    /// Builds the Job Spec which is similar to all jobs
    fn build_job(&self, job_name: String, namespace: String, owner_references: Option<Vec<OwnerReference>>, image: String, labels: BTreeMap<String, String>, command_args: Vec<String>, env_vars: Vec<EnvVar>, volume_mounts: Vec<VolumeMount>, volumes: Vec<Volume>) -> Job {
        Job {
            metadata: ObjectMeta {
                name: Some(job_name),
                namespace: Some(namespace),
                labels: Some(labels.clone()),
                owner_references: owner_references.to_owned(),
                ..ObjectMeta::default()
            },
            spec: Some(JobSpec {
                parallelism: Some(1),
                completions: Some(1),
                backoff_limit: Some(1),
                active_deadline_seconds: Some(constants::OPERATOR_JOB_TIMEOUT.try_into().unwrap()),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "hopli".to_owned(),
                            image: Some(image),
                            image_pull_policy: Some("Always".to_owned()),
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: Some(command_args),
                            env: Some(env_vars),
                            volume_mounts: Some(volume_mounts),
                            resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
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
    async fn build_volume_mounts(&self, is_create_node_job: &bool) -> Vec<VolumeMount> {
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
    async fn build_volumes(&self, secret: &HoprdSecret, is_create_node_job: &bool) -> Vec<Volume> {
        let mut volumes = Vec::with_capacity(2);

        if is_create_node_job.to_owned() {
            
            volumes.push(Volume {
                name: "hopr-repo-volume".to_owned(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
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
        let configmap_name = format!("{}-scripts", self.config.instance.name);
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

    ///Build Job environment variables
    async fn build_env_vars(&self, hoprd_secret: &HoprdSecret, is_create_node_job: &bool) -> Vec<EnvVar> {
        let mut env_vars = Vec::with_capacity(2); 
        if ! is_create_node_job {
            env_vars.push(EnvVar {
                name: "IDENTITY_PASSWORD".to_owned(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        key: hoprd_secret
                            .password_ref_key
                            .as_ref()
                            .unwrap_or(&constants::HOPRD_PASSWORD.to_owned())
                            .to_string(),
                        name: Some(hoprd_secret.secret_name.to_owned()),
                        ..SecretKeySelector::default()
                    }),
                    ..EnvVarSource::default()
                }),
                ..EnvVar::default()
            });
            let labels = utils::get_resource_kinds(self.client.clone(), utils::ResourceType::Secret, utils::ResourceKind::Labels, &hoprd_secret.secret_name.to_owned(), &self.config.instance.namespace.to_owned()).await;
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
        } else {
            env_vars.push(EnvVar {
                name: constants::OPERATOR_INSTANCE_NAMESPACE.to_owned(),
                value: Some(self.config.instance.namespace.to_owned()),
                ..EnvVar::default()
            });
        }
        env_vars.push(EnvVar {
            name: constants::HOPRD_NETWORK.to_owned(),
            value: Some(self.hoprd.spec.network.to_owned()),
            ..EnvVar::default()
        });

        env_vars.push(EnvVar {
            name: constants::HOPR_PRIVATE_KEY.to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: constants::HOPR_PRIVATE_KEY.to_owned(),
                    name: Some(self.config.instance.secret_name.to_owned()),
                    ..SecretKeySelector::default()
                }),
                ..EnvVarSource::default()
            }),
            ..EnvVar::default()
        });

        env_vars.push(EnvVar {
            name: constants::HOPLI_ETHERSCAN_API_KEY.to_owned(),
            value: Some("DummyValue".to_owned()),
            ..EnvVar::default()
        });

        return env_vars;
    }

}

