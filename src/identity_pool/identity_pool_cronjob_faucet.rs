
use k8s_openapi::api::batch::v1::{CronJob, CronJobSpec, JobTemplateSpec, JobSpec};
use k8s_openapi::api::core::v1::{EnvVar, EnvVarSource, SecretKeySelector, PodTemplateSpec, Container, PodSpec, EmptyDirVolumeSource,Volume, VolumeMount};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{DeleteParams, PostParams, PatchParams, Patch};
use kube::core::ObjectMeta;
use kube::runtime::wait::{await_condition, conditions};
use kube::{ResourceExt, Resource};
use kube::{Api, Client};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::info;

use crate::context_data::ContextData;
use crate::hoprd::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_pool::identity_pool_resource::IdentityPool;
use crate::model::Error;
use crate::{utils, constants};

/// Creates a new CronJob to enable the monitoring with Prometheus of the hoprd node,
pub async fn create_cron_job(context_data: Arc<ContextData>, identity_pool: &IdentityPool) -> Result<CronJob, Error> {
    let identity_pool_name = identity_pool.name_any();
    let namespace: String = identity_pool.metadata.namespace.as_ref().unwrap().to_owned();
    let cron_job_name: String = format!("auto-funding-{}", identity_pool_name);
    info!("Creating CronJob {cron_job_name} for identity pool {identity_pool_name} in namespace {namespace}");
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![identity_pool.controller_owner_ref(&()).unwrap()]);
    let labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(), Some(identity_pool_name.to_owned()), Some("auto-funding".to_owned()));

    let cron_job: CronJob = CronJob {
        metadata: ObjectMeta {
            name: Some(cron_job_name.to_owned()),
            namespace: Some(namespace.to_owned()),
            owner_references,
            labels: Some(labels.clone()),
            ..ObjectMeta::default()
        },
        spec: Some(CronJobSpec {
            concurrency_policy: Some("Forbid".to_owned()),
            failed_jobs_history_limit: Some(3),
            successful_jobs_history_limit: Some(3),
            schedule: identity_pool.spec.funding.to_owned().unwrap().schedule,
            job_template: JobTemplateSpec {
                metadata: Some(ObjectMeta::default()),
                spec: Some(get_job_template(context_data.clone(), identity_pool).await),
            },
            ..CronJobSpec::default()
        }),
        ..CronJob::default()
    };

    info!("CronJob {} created", &cron_job_name.to_owned());
    let api: Api<CronJob> = Api::namespaced(context_data.client.clone(), &namespace);
    let created_cron_job = api.create(&PostParams::default(), &cron_job).await.unwrap();

    Ok(created_cron_job)
}

async fn build_args_line(identity_pool: &IdentityPool) -> Option<Vec<String>> {
    let native_amount: String =identity_pool.spec.funding.clone().unwrap().native_amount.to_string();
    let network: String = identity_pool.spec.network.to_owned();
    let command_line: String = format!("PATH=${{PATH}}:/app/hoprnet/.foundry/bin/ /bin/hopli faucet --provider-url https://gnosis-chain.rpc.rank1.co --network {} --hopr-amount 0 --native-amount \"{}\" --address $(cat /data/addresses.txt)", network, native_amount);
    Some(vec![command_line])
}

async fn get_job_template(context_data: Arc<ContextData>, identity_pool: &IdentityPool) -> JobSpec {
    let volumes: Vec<Volume> = vec![Volume {
        name: "data".to_owned(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Volume::default()
    }];
    let volume_mounts: Vec<VolumeMount> = vec![VolumeMount {
        name: "data".to_owned(),
        mount_path: "/data".to_owned(),
        ..VolumeMount::default()
    }];
    let kubectl_args = Some(vec![format!("kubectl get IdentityHoprd -o jsonpath='{{.items[?(.spec.identityPoolName == \"{}\")].spec.nativeAddress}}' | tr ' ' ',' > /data/addresses.txt", identity_pool.name_any())]);

    JobSpec {
        parallelism: Some(1),
        completions: Some(1),
        backoff_limit: Some(1),
        active_deadline_seconds: Some(constants::OPERATOR_JOB_TIMEOUT.try_into().unwrap()),
        template: 
            PodTemplateSpec {
                metadata: Some(ObjectMeta::default()),
                spec: Some(PodSpec {
                    init_containers: Some(vec![Container {
                            name: "kubectl".to_owned(),
                            image: Some("registry.hub.docker.com/bitnami/kubectl:1.24".to_owned()),
                            image_pull_policy: Some("IfNotPresent".to_owned()),
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: kubectl_args,
                            volume_mounts: Some(volume_mounts.to_owned()),
                            resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                            ..Container::default()
                    }]),
                    containers: vec![Container {
                        name: "hopli".to_owned(),
                        image: Some(context_data.config.hopli_image.to_owned()),
                        image_pull_policy: Some("Always".to_owned()),
                        command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                        args: build_args_line(identity_pool).await,
                        env: Some(get_env_var(identity_pool.spec.secret_name.to_owned()).await),
                        volume_mounts: Some(volume_mounts.to_owned()),
                        resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                        ..Container::default()
                    }],
                    service_account: Some(identity_pool.name_any()),
                    volumes: Some(volumes),
                    restart_policy: Some("Never".to_owned()),
                ..PodSpec::default()
                })
            },
        ..JobSpec::default()
    }
}

async fn get_env_var(secret_name: String)-> Vec<EnvVar> {
    vec![
            EnvVar {
                name: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        key: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                        name: Some(secret_name.to_owned()),
                        ..SecretKeySelector::default()
                    }),
                    ..EnvVarSource::default()
                }),
                ..EnvVar::default()
            },
            EnvVar {
                name: constants::IDENTITY_POOL_WALLET_PRIVATE_KEY_REF_KEY.to_owned(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        key: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                        name: Some(secret_name.to_owned()),
                        ..SecretKeySelector::default()
                    }),
                    ..EnvVarSource::default()
                }),
                ..EnvVar::default()
            }
        ]
}

pub async fn modify_cron_job(client: Client, identity_pool: &IdentityPool) -> Result<CronJob, Error> {
    let identity_pool_name = identity_pool.name_any();
    let namespace: String = identity_pool.metadata.namespace.as_ref().unwrap().to_owned();

    let cron_job_name: String = format!("auto-funding-{}", identity_pool_name);
    let api: Api<CronJob> = Api::namespaced(client.clone(), &namespace);
    if let Some(cron_job) = api.get_opt(&cron_job_name).await? {
        let mut cron_job_spec = cron_job.spec.clone().unwrap();
        cron_job_spec.schedule = identity_pool.spec.funding.clone().unwrap().schedule;
        let container = cron_job_spec.job_template.spec.as_mut().unwrap().template.spec.as_mut().unwrap().containers.first_mut().unwrap();
        container.args = build_args_line(identity_pool).await;
        let patch = &Patch::Merge(json!({ "spec": cron_job_spec }));
        let cron_job = api.patch(&cron_job_name, &PatchParams::default(), patch).await.expect("Could not modify cronjob");
        Ok(cron_job)
    } else {
        Err(Error::HoprdConfigError(format!("CronJob {cron_job_name} was not found in namespace {namespace}")))
    }
}

/// Deletes an existing CronJob.
pub async fn delete_cron_job(client: Client, name: &str,namespace: &str) -> Result<(), Error> {
    let api: Api<CronJob> = Api::namespaced(client, namespace);
    if let Some(create_cron_job) = api.get_opt(name).await? {
        let uid = create_cron_job.metadata.uid.unwrap();
        api.delete(name, &DeleteParams::default()).await?;
        await_condition(api, name, conditions::is_deleted(&uid)).await.unwrap();
        Ok(info!("CronJob {name} successfully deleted"))
    } else {
        Ok(info!("CronJob {name} in namespace {namespace} about to delete not found"))
    }
}
