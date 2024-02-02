use crate::events::IdentityPoolEventEnum;
use crate::hoprd::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::identity_hoprd::identity_hoprd_resource::{IdentityHoprd, IdentityHoprdPhaseEnum};
use crate::model::Error;
use crate::{constants, context_data::ContextData};
use crate::{identity_pool::{identity_pool_service_account,  identity_pool_cronjob_faucet, identity_pool_service_monitor}, utils, resource_generics};
use chrono::Utc;
use k8s_openapi::api::batch::v1::{Job, JobSpec, CronJob};
use k8s_openapi::api::core::v1::{
    Container, EmptyDirVolumeSource, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec, Secret, SecretKeySelector, Volume, VolumeMount
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{ListParams, PostParams};
use kube::core::ObjectMeta;
use kube::runtime::conditions;
use kube::runtime::wait::await_condition;
use kube::{
    api::{Api, Patch, PatchParams},
    client::Client,
    runtime::controller::Action,
    CustomResource, Result,
};
use kube::{Resource, ResourceExt};
use rand::{distributions::Alphanumeric, Rng};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha2",
    kind = "IdentityPool",
    plural = "identitypools",
    derive = "PartialEq",
    namespaced
)]
#[kube(status = "IdentityPoolStatus", shortname = "identitypool")]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolSpec {
    pub network: String,
    pub secret_name: String,
    pub min_ready_identities: i32,
    pub funding: Option<IdentityPoolFunding>
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolFunding {
    pub schedule: String,
    pub native_amount: String
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolStatus {
    pub update_timestamp: String,
    pub phase: IdentityPoolPhaseEnum,
    pub size: i32,
    pub locked: i32,
    pub checksum: String,
}

impl Default for IdentityPoolStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            phase: IdentityPoolPhaseEnum::Initialized,
            size: 0,
            locked: 0,
            checksum: "init".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum IdentityPoolPhaseEnum {
    // Status that represent when the IdentityPool is initialized after creation by creating the serviceMonitor
    Initialized,
    /// Status that represent when the IdentityPool initialization validation has failed
    Failed,
    // Status that represent when the IdentityPool is out of synchronization. It requires to create new identities
    OutOfSync,
    // Status that represent when the IdentityPool is ready to be used
    Ready,
    /// Status that represent when the IdentityPool is being deleted
    Deleting,
    // Event that represent when the IdentityPool has locked an identity
    Locked,
    // Event that represent when the IdentityPool has unlocked an identity
    Unlocked,
    // Event that represents when the IdentityPool has created a new identity
    IdentityCreated,
    // Event that represents when the IdentityPool has created a new identity
    IdentityDeleted,
}

impl Display for IdentityPoolPhaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            IdentityPoolPhaseEnum::Initialized => write!(f, "Initialized"),
            IdentityPoolPhaseEnum::Failed => write!(f, "Failed"),
            IdentityPoolPhaseEnum::OutOfSync => write!(f, "OutOfSync"),
            IdentityPoolPhaseEnum::Ready => write!(f, "Ready"),
            IdentityPoolPhaseEnum::Deleting => write!(f, "Deleting"),
            IdentityPoolPhaseEnum::Locked => write!(f, "Locked"),
            IdentityPoolPhaseEnum::Unlocked => write!(f, "Unlocked"),
            IdentityPoolPhaseEnum::IdentityCreated => write!(f, "IdentityCreated"),
            IdentityPoolPhaseEnum::IdentityDeleted => write!(f, "IdentityDeleted"),
        }
    }
}


impl Default for IdentityPool {
    fn default() -> Self {
        Self {
            metadata: ObjectMeta::default(),
            spec: IdentityPoolSpec::default(),
            status: Some(IdentityPoolStatus::default()),
        }
    }
}


impl IdentityPool {
    /// Handle the creation of IdentityPool resource
    pub async fn create(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        if ! self.check_wallet(client.clone()).await.unwrap() {
            context_data.send_event(self, IdentityPoolEventEnum::Failed, None).await;
            return Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY_ERROR)))
        }
        info!("Starting to create IdentityPool {identity_pool_name} in namespace {identity_pool_namespace}");
        resource_generics::add_finalizer(client.clone(), self).await;
        identity_pool_service_monitor::create_service_monitor(context_data.clone(), &identity_pool_name, &identity_pool_namespace, &self.spec.secret_name, owner_references.to_owned()).await?;
        identity_pool_service_account::create_rbac(context_data.clone(), &identity_pool_namespace, &identity_pool_name,owner_references.to_owned()).await?;
        if self.spec.funding.is_some() {
            identity_pool_cronjob_faucet::create_cron_job(context_data.clone(), self).await.expect("Could not create Cronjob");
        }
        // TODO: Validate data
        // - Check that the secret exist and contains the required keys
        // - Does the wallet private key have permissions in Network to register new nodes and create safes ?
        // - Does the wallet private key have enough funds to work ?
        // context_data.send_event(self, dentityPoolEventEnum::Initialized, None).await
        context_data.send_event(self, IdentityPoolEventEnum::Initialized, None).await;
        self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Initialized).await?;
        if self.spec.min_ready_identities == 0 {
            context_data.send_event(self, IdentityPoolEventEnum::Ready, None).await;
            self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Ready).await?;
        } else {
            context_data.send_event(self,IdentityPoolEventEnum::OutOfSync,Some(self.spec.min_ready_identities.to_string()),).await;
            self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::OutOfSync).await?;
            info!("Identity {identity_pool_name} in namespace {identity_pool_namespace} requires to create {} new identities", self.spec.min_ready_identities);
        }
        info!("IdentityPool {identity_pool_name} in namespace {identity_pool_namespace} successfully created");
        context_data.state.write().await.add_identity_pool(self.clone());
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    /// Handle the modification of IdentityPool resource
    pub async fn modify(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context_data.client.clone();
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();
        if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&IdentityPoolPhaseEnum::Ready) {
            if self.annotations().contains_key(constants::ANNOTATION_LAST_CONFIGURATION) {
                let previous_text: String = self.annotations().get_key_value(constants::ANNOTATION_LAST_CONFIGURATION).unwrap().1.parse().unwrap();
                match serde_json::from_str::<IdentityPool>(&previous_text) {
                    Ok(previous_identity_pool) => {
                        if self.changed_inmutable_fields(&previous_identity_pool.spec) {
                            context_data.send_event(self,IdentityPoolEventEnum::Failed,None).await;
                            self.update_status(client.clone(), IdentityPoolPhaseEnum::Failed).await?;
                        } else {
                            info!("Identity pool {identity_pool_name} in namespace {identity_pool_namespace} has been successfully modified");

                            // Syncrhonize size
                            if self.status.as_ref().unwrap().size - self.status.as_ref().unwrap().locked - self.spec.min_ready_identities < 0 {
                                let pending = self.spec.min_ready_identities - self.status.as_ref().unwrap().locked - self.status.as_ref().unwrap().size;
                                context_data.send_event(self,IdentityPoolEventEnum::OutOfSync,Some(pending.to_string())).await;
                                self.update_status(client.clone(), IdentityPoolPhaseEnum::OutOfSync).await?;
                                info!("Identity {identity_pool_name} in namespace {identity_pool_namespace} requires to create {} new identities", self.spec.min_ready_identities);
                            }else {
                                context_data.send_event(self,IdentityPoolEventEnum::Ready, None).await;
                                self.update_status(client.clone(), IdentityPoolPhaseEnum::Ready).await?;
                            }
                            self.modify_funding(context_data).await?
                        }
                    },
                    Err(_err) => {
                        error!("Could not parse the last applied configuration from {identity_pool_name}.");
                    }
                }
            }
        } else if self.status.is_some() && self.status.as_ref().unwrap().phase.eq(&IdentityPoolPhaseEnum::Failed) {
            // Assumes that the next modification of the resource is to recover to a good state
            context_data.send_event(self,IdentityPoolEventEnum::Ready,None).await;
            self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Ready).await?;
            warn!("Detected a change in IdentityPool {identity_pool_name}. Automatically recovering to a Ready phase");
        } else {
            error!("The resource cannot be modified");
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    // Syncrhonize funding
    async fn modify_funding(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String = self.name_any();

        let api: Api<CronJob> = Api::namespaced(context_data.client.clone(), &identity_pool_namespace);
        if (api.get_opt(format!("auto-funding-{}", identity_pool_name).as_str()).await?).is_some() {
            if self.spec.funding.is_none() {
                info!("Deleting previous Cronjob {identity_pool_name} in namespace {identity_pool_namespace}");
                identity_pool_cronjob_faucet::delete_cron_job(context_data.client.clone(), &identity_pool_namespace, &identity_pool_name).await.expect("Could not delete cronjob");
            } else {
                info!("Modifying Cronjob {identity_pool_name} in namespace {identity_pool_namespace}");
                identity_pool_cronjob_faucet::modify_cron_job(context_data.client.clone(), self).await.expect("Could not modify cronjob");
            }
        } else if self.spec.funding.is_some() {
            info!("Creating new Cronjob {identity_pool_name} in namespace {identity_pool_namespace}");
            identity_pool_cronjob_faucet::create_cron_job(context_data.clone(), self).await.expect("Could not create Cronjob");
        }
        Ok(())
    }

    // Handle the deletion of IdentityPool resource
    pub async fn delete(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let identity_pool_namespace = self.namespace().unwrap();        
        let identity_pool_name = self.name_any();
        if self.status.as_ref().unwrap().locked == 0 {
            let client: Client = context_data.client.clone();
            self.update_status(context_data.client.clone(), IdentityPoolPhaseEnum::Deleting).await?;
            context_data.send_event(self, IdentityPoolEventEnum::Deleting, None).await;
            info!("Starting to delete identity {identity_pool_name} from namespace {identity_pool_namespace}");
            identity_pool_service_monitor::delete_service_monitor(client.clone(), &identity_pool_name, &identity_pool_namespace).await?;
            identity_pool_service_account::delete_rbac(client.clone(), &identity_pool_namespace, &identity_pool_name).await?;
            if self.spec.funding.is_some() {
                identity_pool_cronjob_faucet::delete_cron_job(client.clone(), &identity_pool_namespace, &identity_pool_name).await?;
            }
            resource_generics::delete_finalizer(client.clone(), self).await;
            context_data.state.write().await.remove_identity_pool(&identity_pool_namespace, &identity_pool_name);
            info!("Identity {identity_pool_name} in namespace {identity_pool_namespace} has been successfully deleted");
            Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
        } else {
            warn!("Cannot delete an identity pool with identities in use");
            Ok(Action::requeue(Duration::from_secs(
                constants::RECONCILE_FREQUENCY,
            )))
        }
    }

    pub async fn sync(&mut self, context_data: Arc<ContextData>) -> Result<Action, Error> {
        let mut current_ready_identities = self.status.as_ref().unwrap().size - self.status.as_ref().unwrap().locked;
        let mut iterations = (self.spec.min_ready_identities - current_ready_identities) * 2;
        if self.are_active_jobs(context_data.clone()).await.unwrap() {
            warn!("Skipping synchornization for {} in namespace {} as there is still one job in progress", self.name_any(), self.namespace().unwrap());
        } else {
            while current_ready_identities < self.spec.min_ready_identities && iterations > 0 {
                iterations -= 1;
                // Invoke Job
                match self.create_new_identity(context_data.clone()).await {
                    Ok(()) => current_ready_identities += 1,
                    Err(error) => {
                        error!("Could not create identity: {:?}", error);
                        iterations = 0;
                    }
                };
            }
            if current_ready_identities >= self.spec.min_ready_identities {
                context_data.send_event(self, IdentityPoolEventEnum::Ready, None).await;
            } else {
                context_data.send_event(self, IdentityPoolEventEnum::OutOfSync, Some((self.spec.min_ready_identities - current_ready_identities).to_string())).await;
                info!("Identity {} in namespace {} failed to create required identities", self.name_any(), self.namespace().unwrap());
            }
        }
        Ok(Action::requeue(Duration::from_secs(
            constants::RECONCILE_FREQUENCY,
        )))
    }

    fn changed_inmutable_fields(&self, previous_identity: &IdentityPoolSpec) -> bool {
        if !self.spec.network.eq(&previous_identity.network) {
            error!("Configuration is invalid, 'network' field cannot be changed on {}.", self.name_any());
            true
        } else if !self.spec.secret_name.eq(&previous_identity.secret_name) {
            error!("Configuration is invalid, 'secret_name' field cannot be changed on {}.", self.name_any());
            true
        } else {
            false
        }
    }

    pub fn get_checksum(&self) -> String {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        hasher.finish().to_string()
    }

    /// Updates the status of IdentityPool
    pub async fn update_status(&mut self, client: Client, phase: IdentityPoolPhaseEnum) -> Result<(), Error> {
        let identity_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();
        let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
        let mut identity_pool_status = self.status.as_ref().unwrap_or(&IdentityPoolStatus::default()).to_owned();

        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
        if phase.eq(&IdentityPoolPhaseEnum::Deleting) {
            Ok(())
        } else {
            identity_pool_status.update_timestamp = Utc::now().to_rfc3339();
            identity_pool_status.checksum = self.get_checksum();
            identity_pool_status.phase = phase;
            if phase.eq(&IdentityPoolPhaseEnum::IdentityCreated) {
                if (identity_pool_status.size - identity_pool_status.locked + 1) >= self.spec.min_ready_identities
                {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
                identity_pool_status.size += 1;
            } else if phase.eq(&IdentityPoolPhaseEnum::IdentityDeleted) {
                if (identity_pool_status.size - identity_pool_status.locked - 1) >= self.spec.min_ready_identities
                {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
                identity_pool_status.size -= 1;
            };

            if phase.eq(&IdentityPoolPhaseEnum::Locked) {
                if (identity_pool_status.size - identity_pool_status.locked - 1) >= self.spec.min_ready_identities
                {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
                identity_pool_status.locked += 1;
            } else if phase.eq(&IdentityPoolPhaseEnum::Unlocked) {
                if (identity_pool_status.size - identity_pool_status.locked + 1) >= self.spec.min_ready_identities
                {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::Ready;
                } else {
                    identity_pool_status.phase = IdentityPoolPhaseEnum::OutOfSync;
                }
                identity_pool_status.locked -= 1;
            };
            let patch = Patch::Merge(json!({
                    "status": identity_pool_status
            }));
            match api.patch(&identity_hoprd_name, &PatchParams::default(), &patch).await {
                Ok(_identity) => {
                    self.status = Some(identity_pool_status.clone());
                    Ok(debug!("IdentityPool current status: {:?}", identity_pool_status))
                },
                Err(error) => Ok(error!("Could not update status on {identity_hoprd_name}: {:?}",error)),
            }
        }
    }

    pub async fn get_pool_identities(&self, client: Client) -> Vec<IdentityHoprd> {
        let api: Api<IdentityHoprd> = Api::namespaced(client,&self.namespace().unwrap().to_owned());
        let namespace_identities = api.list(&ListParams::default()).await.expect("Could not list namespace identities");
        let pool_identities: Vec<IdentityHoprd>  = namespace_identities.iter()
            .filter(|&identity| {
                identity.metadata.owner_references.as_ref().unwrap().first().unwrap().name.eq(&self.name_any())
            }).cloned().collect();
        pool_identities
    }

    /// Gets the first identity in ready status
    pub async fn get_ready_identity(&mut self, client: Client, identity_name: Option<String>) -> Result<Option<IdentityHoprd>, Error> {
        let pool_identities: Vec<IdentityHoprd>  = self.get_pool_identities(client).await;

        let identity: Option<IdentityHoprd> = match identity_name.clone() {
            Some(provided_identity_name) => {
                let found = pool_identities.iter().find(|&identity| identity.metadata.name.clone().unwrap().eq(&provided_identity_name)).cloned();
                if found.is_none() {
                    warn!("The identity provided {} does not exist", provided_identity_name); 
                    None
                } else if found.as_ref().unwrap().status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::Ready) {
                    found
                } else {
                    let status = found.as_ref().unwrap().status.as_ref().unwrap().to_owned();
                    warn!("The identity {} is in phase {} and might be used by {}", provided_identity_name, status.phase, status.hoprd_node_name.unwrap_or("unknown".to_owned())); 
                    None
                }
            },
            None => { // No identity has been provided
                let ready_pool_identity: Option<IdentityHoprd> = pool_identities.iter()
                .find(|&identity| identity.status.as_ref().unwrap().phase.eq(&IdentityHoprdPhaseEnum::Ready)).cloned();
                if ready_pool_identity.is_none() {
                    warn!("There are no identities ready to be used in this pool {}", self.name_any()); 
                }
                ready_pool_identity
            }
        };
        Ok(identity)

    }

    async fn are_active_jobs(&self, context_data: Arc<ContextData>) -> Result<bool, Error> {
        let namespace: String = self.metadata.namespace.as_ref().unwrap().to_owned();
        let api: Api<Job> = Api::namespaced(context_data.client.clone(), &namespace);
        let label_selector: String = format!(
            "{}={},{}={},{}={}",
            constants::LABEL_KUBERNETES_NAME,
            context_data.config.instance.name.to_owned(),
            constants::LABEL_KUBERNETES_COMPONENT,
            "create-identity".to_owned(),
            constants::LABEL_KUBERNETES_IDENTITY_POOL,
            self.name_any()
        );
        let lp = ListParams::default().labels(&label_selector);
        let jobs = api.list(&lp).await.unwrap().items;
        let active_jobs: Vec<&Job> = jobs.iter().filter(|&job| job.status.as_ref().unwrap().active.is_some()).collect();
        Ok(!active_jobs.is_empty())
    }

    async fn create_new_identity(&self, context_data: Arc<ContextData>) -> Result<(), Error> {
        let identity_name = format!("{}-{}", self.name_any(), self.status.as_ref().unwrap().size + 1);
        context_data.send_event(self, IdentityPoolEventEnum::CreatingIdentity,Some(identity_name.to_owned())).await;
        let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
        let job_name: String = format!("create-identity-{}-{}", &identity_name.to_owned(), random_string.to_ascii_lowercase());
        let namespace: String = self.metadata.namespace.as_ref().unwrap().to_owned();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        let mut labels: BTreeMap<String, String> = utils::common_lables(context_data.config.instance.name.to_owned(), Some(identity_name.to_owned()), Some("job-create-identity".to_owned()));
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "create-identity".to_owned());
        labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), self.name_any());
        let create_identity_args: Vec<String> = vec![format!("curl {}/create-identity.sh -s | bash", constants::OPERATOR_JOB_SCRIPT_URL.to_owned())];
        let create_resource_args: Vec<String> = vec!["/app/hoprd-identity-created/create-resource.sh".to_owned()];
        let env_vars: Vec<EnvVar> = vec![
            EnvVar {
                name: constants::IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY.to_owned(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        key: constants::IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY.to_owned(),
                        name: Some(self.spec.secret_name.to_owned()),
                        ..SecretKeySelector::default()
                    }),
                    ..EnvVarSource::default()
                }),
                ..EnvVar::default()
            },
            EnvVar {
                name: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        key: constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY.to_owned(),
                        name: Some(self.spec.secret_name.to_owned()),
                        ..SecretKeySelector::default()
                    }),
                    ..EnvVarSource::default()
                }),
                ..EnvVar::default()
            },
            EnvVar {
                name: "JOB_SCRIPT_URL".to_owned(),
                value: Some(format!(
                    "{}/create-resource.sh",
                    constants::OPERATOR_JOB_SCRIPT_URL
                )),
                ..EnvVar::default()
            },
            EnvVar {
                name: "JOB_NAMESPACE".to_owned(),
                value: Some(namespace.to_owned()),
                ..EnvVar::default()
            },
            EnvVar {
                name: "IDENTITY_POOL_NAME".to_owned(),
                value: Some(self.name_any()),
                ..EnvVar::default()
            },
            EnvVar {
                name: "IDENTITY_NAME".to_owned(),
                value: Some(identity_name.to_owned()),
                ..EnvVar::default()
            },
            EnvVar {
                name: constants::HOPRD_NETWORK.to_owned(),
                value: Some(self.spec.network.to_owned()),
                ..EnvVar::default()
            },
        ];
        let volumes: Vec<Volume> = vec![Volume {
            name: "hoprd-identity-created".to_owned(),
            empty_dir: Some(EmptyDirVolumeSource::default()),
            ..Volume::default()
        }];
        let volume_mounts: Vec<VolumeMount> = vec![VolumeMount {
            name: "hoprd-identity-created".to_owned(),
            mount_path: "/app/hoprd-identity-created".to_owned(),
            ..VolumeMount::default()
        }];

        // Definition of the Job
        let create_node_job: Job = Job {
            metadata: ObjectMeta {
                name: Some(job_name.to_owned()),
                namespace: Some(namespace.to_owned()),
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
                            image: Some(context_data.config.hopli_image.to_owned()),
                            image_pull_policy: Some("Always".to_owned()),
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: Some(create_identity_args),
                            env: Some(env_vars.to_owned()),
                            volume_mounts: Some(volume_mounts.to_owned()),
                            resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                            ..Container::default()
                        }]),
                        containers: vec![Container {
                            name: "kubectl".to_owned(),
                            image: Some("registry.hub.docker.com/bitnami/kubectl:1.24".to_owned()),
                            image_pull_policy: Some("IfNotPresent".to_owned()),
                            command: Some(vec!["/bin/sh".to_owned(), "-c".to_owned()]),
                            args: Some(create_resource_args),
                            env: Some(env_vars),
                            volume_mounts: Some(volume_mounts),
                            resources: Some(HoprdDeploymentSpec::get_resource_requirements(None)),
                            ..Container::default()
                        }],
                        service_account_name: Some(self.name_any()),
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
        let api: Api<Job> = Api::namespaced(context_data.client.clone(), &namespace);
        api.create(&PostParams::default(), &create_node_job).await.unwrap();
        let job_completed = await_condition(api, &job_name, conditions::is_job_completed());
        match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT), job_completed).await
        {
            Ok(job_option) => match job_option.unwrap() {
                Some(job) => {
                    if job.status.unwrap().failed.is_none() {
                        Ok(info!("Job {} completed successfully", &job_name.to_owned()))
                    } else {
                        Err(Error::JobExecutionError(format!("Job pod execution for {} failed", &job_name.to_owned()).to_owned()))
                    }
                }
                None => Err(Error::JobExecutionError(format!("Job execution for {} failed", &job_name.to_owned()).to_owned()))
            },
            Err(_error) => Err(Error::JobExecutionError(format!("Job timeout for {}", &job_name.to_owned()).to_owned()))
        }
    }

    async fn check_wallet(&self, client: Client) -> Result<bool,Error> {
        let api: Api<Secret> = Api::namespaced(client, &self.namespace().unwrap());
        if let Some(wallet) = api.get_opt(&self.spec.secret_name).await? {
            if let Some(wallet_data) = wallet.data {
                if wallet_data.contains_key(constants::IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY) && 
                    wallet_data.contains_key(constants::IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY) && 
                    wallet_data.contains_key(constants::IDENTITY_POOL_API_TOKEN_REF_KEY)
                {
                    Ok(true)
                } else {
                    error!("IdentityPool {} has a secret {} with some missing data", self.name_any(), self.spec.secret_name);
                    Ok(false)
                }
            } else {
                error!("IdentityPool {} has a secret {} with empty data", self.name_any(), self.spec.secret_name);
                Ok(false)
            }
        } else {
            error!("IdentityPool {} cannot find secret {} in namespace {}", self.name_any(), self.spec.secret_name, self.namespace().unwrap());
            Ok(false)
        }
    }

}
