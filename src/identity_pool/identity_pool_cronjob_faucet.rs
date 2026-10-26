use k8s_openapi::api::batch::v1::{CronJob, CronJobSpec, JobSpec, JobTemplateSpec};
use crate::utils::{OptionExt, ResourceIdentity};
use k8s_openapi::api::core::v1::{Container, EmptyDirVolumeSource, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec, SecretKeySelector, Volume, VolumeMount};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{Patch, PatchParams, PostParams};
use kube::core::ObjectMeta;
use kube::{Api, Client};
use kube::{Resource, ResourceExt};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::info;

use crate::context_data::ContextData;
use crate::hoprd::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_pool::identity_pool_resource::IdentityPool;
use crate::model::Error;
use crate::{constants, utils};

/// Creates a new CronJob to enable the monitoring with Prometheus of the hoprd node,
pub async fn create_cron_job(context_data: Arc<ContextData>, identity_pool: &IdentityPool) -> Result<CronJob, Error> {
    let identity_pool_name = identity_pool.name_any();
    let namespace: String = identity_pool.require_namespace()?;
    let cron_job_name: String = format!("auto-funding-{}", identity_pool_name);
    info!("Creating CronJob {cron_job_name} for identity pool {identity_pool_name} in namespace {namespace}");
    validate_funding(identity_pool)?;
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![identity_pool.controller_owner_ref(&()).or_missing("controller owner reference")?]);
    let mut labels: BTreeMap<String, String> = utils::common_lables(identity_pool_name.to_owned(), None, Some("auto-funding".to_owned()));
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool_name.to_owned());

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
            schedule: identity_pool.spec.funding.as_ref().or_missing("IdentityPool funding")?.schedule.to_owned(),
            job_template: JobTemplateSpec {
                metadata: Some(ObjectMeta::default()),
                spec: Some(get_job_template(context_data.clone(), identity_pool).await?),
            },
            ..CronJobSpec::default()
        }),
        ..CronJob::default()
    };

    info!("CronJob {} created", &cron_job_name.to_owned());
    let api: Api<CronJob> = Api::namespaced(context_data.client.clone(), &namespace);
    let created_cron_job = api.create(&PostParams::default(), &cron_job).await?;

    Ok(created_cron_job)
}

/// Builds the command of the faucet container.
///
/// The IdentityPool values are not interpolated into the command: they are passed as environment variables
/// (see `get_faucet_env_vars`) and referenced as quoted shell variables, so the strings written in the CR are
/// never parsed as shell code (e.g. a `$(...)` in `nativeAmount` is not executed).
async fn build_args_line() -> Option<Vec<String>> {
    let command_line: String = "PATH=${PATH}:/app/hoprnet/.foundry/bin/ /bin/hopli faucet --provider-url \"$FAUCET_PROVIDER_URL\" --network \"$FAUCET_NETWORK\" --hopr-amount 0 --native-amount \"$FAUCET_NATIVE_AMOUNT\" --address \"$(cat /data/addresses.txt)\"".to_owned();
    Some(vec![command_line])
}

/// Rejects the funding values that cannot be a network name or a decimal amount.
/// It is a second line of defense, as the values are already passed to the command as environment variables.
fn validate_funding(identity_pool: &IdentityPool) -> Result<(), Error> {
    let network = &identity_pool.spec.network;
    if network.is_empty() || !network.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(Error::UserInputError(format!("Invalid network '{}': only alphanumeric characters, '-', '_' and '.' are allowed", network)));
    }
    let native_amount = identity_pool.spec.funding.as_ref().map(|funding| funding.native_amount.to_string()).unwrap_or_default();
    let is_decimal = !native_amount.is_empty() && native_amount.chars().all(|c| c.is_ascii_digit() || c == '.') && native_amount.chars().filter(|c| *c == '.').count() <= 1;
    if !is_decimal {
        return Err(Error::UserInputError(format!("Invalid nativeAmount '{}': it must be a decimal number", native_amount)));
    }
    Ok(())
}

/// Environment variables of the faucet command that depend on the IdentityPool values and the operator configuration
fn get_faucet_env_vars(context_data: &ContextData, identity_pool: &IdentityPool) -> Vec<EnvVar> {
    let native_amount = identity_pool.spec.funding.as_ref().map(|funding| funding.native_amount.to_string()).unwrap_or_default();
    vec![
        EnvVar { name: "FAUCET_PROVIDER_URL".to_owned(), value: Some(context_data.config.hopli_rpc_provider_url.to_owned()), ..EnvVar::default() },
        EnvVar { name: "FAUCET_NETWORK".to_owned(), value: Some(identity_pool.spec.network.to_owned()), ..EnvVar::default() },
        EnvVar { name: "FAUCET_NATIVE_AMOUNT".to_owned(), value: Some(native_amount), ..EnvVar::default() },
    ]
}

async fn get_job_template(context_data: Arc<ContextData>, identity_pool: &IdentityPool) -> Result<JobSpec, Error> {
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
    let kubectl_args = Some(vec![format!(
        "kubectl get IdentityHoprd -o jsonpath='{{.items[?(.spec.identityPoolName == \"{}\")].spec.nodeAddress}}' | tr ' ' ',' | tee /data/addresses.txt",
        identity_pool.name_any()
    )]);

    Ok(JobSpec {
        ttl_seconds_after_finished: Some(600), // 10 minutes
        parallelism: Some(1),
        completions: Some(1),
        backoff_limit: Some(2),
        active_deadline_seconds: Some(i64::try_from(constants::OPERATOR_JOB_TIMEOUT).unwrap_or(i64::MAX)),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta::default()),
            spec: Some(PodSpec {
                init_containers: Some(vec![Container {
                    name: "kubectl".to_owned(),
                    image: Some("registry.hub.docker.com/bitnamilegacy/kubectl:1.33".to_owned()),
                    image_pull_policy: Some("IfNotPresent".to_owned()),
                    command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                    args: kubectl_args,
                    volume_mounts: Some(volume_mounts.to_owned()),
                    resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)?),
                    ..Container::default()
                }]),
                containers: vec![Container {
                    name: "hopli".to_owned(),
                    image: Some(context_data.config.hopli_image.to_owned()),
                    image_pull_policy: Some("Always".to_owned()),
                    command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                    args: build_args_line().await,
                    env: Some({
                        let mut env_vars = get_env_var(identity_pool.spec.secret_name.to_owned()).await;
                        env_vars.extend(get_faucet_env_vars(&context_data, identity_pool));
                        env_vars
                    }),
                    volume_mounts: Some(volume_mounts.to_owned()),
                    resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)?),
                    ..Container::default()
                }],
                service_account: Some(identity_pool.name_any()),
                volumes: Some(volumes),
                restart_policy: Some("Never".to_owned()),
                ..PodSpec::default()
            }),
        },
        ..JobSpec::default()
    })
}

async fn get_env_var(secret_name: String) -> Vec<EnvVar> {
    vec![
        EnvVar {
            name: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                    name: secret_name.to_owned(),
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
                    name: secret_name.to_owned(),
                    ..SecretKeySelector::default()
                }),
                ..EnvVarSource::default()
            }),
            ..EnvVar::default()
        },
    ]
}

pub async fn modify_cron_job(context_data: Arc<ContextData>, identity_pool: &IdentityPool) -> Result<CronJob, Error> {
    let identity_pool_name = identity_pool.name_any();
    let namespace: String = identity_pool.require_namespace()?;
    validate_funding(identity_pool)?;
    info!("Modifying Cronjob {identity_pool_name} in namespace {namespace}");
    let cron_job_name: String = format!("auto-funding-{}", identity_pool_name);
    let api: Api<CronJob> = Api::namespaced(context_data.client.clone(), &namespace);
    if let Some(cron_job) = api.get_opt(&cron_job_name).await? {
        let mut cron_job_spec = cron_job.spec.clone().or_missing("CronJob spec")?;
        cron_job_spec.schedule = identity_pool.spec.funding.as_ref().or_missing("IdentityPool funding")?.schedule.clone();
        let container = cron_job_spec
            .job_template
            .spec
            .as_mut()
            .and_then(|job_spec| job_spec.template.spec.as_mut())
            .and_then(|pod_spec| pod_spec.containers.first_mut())
            .or_missing("CronJob container")?;
        container.args = build_args_line().await;
        container.env = Some({
            let mut env_vars = get_env_var(identity_pool.spec.secret_name.to_owned()).await;
            env_vars.extend(get_faucet_env_vars(&context_data, identity_pool));
            env_vars
        });
        let patch = &Patch::Merge(json!({ "spec": cron_job_spec }));
        let cron_job = api.patch(&cron_job_name, &PatchParams::default(), patch).await?;
        Ok(cron_job)
    } else {
        Err(Error::HoprdConfigError(format!("CronJob {cron_job_name} was not found in namespace {namespace}")))
    }
}

/// Deletes an existing CronJob.
pub async fn delete_cron_job(client: Client, identity_pool: &IdentityPool) -> Result<(), Error> {
    let identity_pool_name = identity_pool.name_any();
    let namespace: String = identity_pool.require_namespace()?;
    info!("Deleting previous Cronjob {identity_pool_name} in namespace {namespace}");
    let cron_job_name: String = format!("auto-funding-{}", identity_pool_name);
    let api: Api<CronJob> = Api::namespaced(client, &namespace);
    crate::utils::delete_and_wait(api, &cron_job_name).await?;
    Ok(())
}
