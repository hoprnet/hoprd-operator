use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{EnvVar, EnvVarSource, SecretKeySelector, Volume, ConfigMapVolumeSource, EmptyDirVolumeSource, VolumeMount, PodTemplateSpec, PodSpec, Container};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::{Resource, ResourceExt};
use kube::api::{ListParams, PostParams};
use kube::core::ObjectMeta;
use kube::runtime::conditions;
use kube::runtime::wait::await_condition;
use tracing::{debug, info, error, warn};
use crate::hoprd_deployment_spec::HoprdDeploymentSpec;
use crate::{identity_pool_service_monitor, utils, identity_pool_service_account};
use crate::identity_hoprd::{IdentityHoprd, IdentityHoprdStatusEnum};
use crate::{constants, context_data::ContextData};
use crate::model::Error;
use chrono::Utc;
use kube::runtime::events::Recorder;
use schemars::JsonSchema;
use serde::{Serialize, Deserialize};
use serde_json::json;
use kube::{
    api::{Api, Patch, PatchParams},
    client::Client,
    runtime::{controller::Action, events::{Event, EventType}
    },
    CustomResource, Result
};

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha",
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
    pub min_ready_identities: i32
}

/// The status object of `Hoprd`
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPoolStatus {
    pub update_timestamp: String,
    pub status: IdentityPoolStatusEnum,
    pub size: i32,
    pub locked: i32,
    pub checksum: String,
}

impl Default for IdentityPoolStatus {
    fn default() -> Self {
        Self {
            update_timestamp: Utc::now().to_rfc3339(),
            status: IdentityPoolStatusEnum::Initialized,
            size: 0,
            locked: 0,
            checksum: "init".to_owned()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum IdentityPoolStatusEnum {
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
    // Event that represent when the IdentityPool is locking an identity
    Locking,
    // Event that represent when the IdentityPool has locked an identity
    Locked,
    // Event that represent when the IdentityPool is unlocking an identity
    Unlocking,
    // Event that represent when the IdentityPool has unlocked an identity
    Unlocked,
    // Event that represents when the IdentityPool is syncronizing by creating new required identities
    CreatingIdentity,
    // Event that represents when the IdentityPool has created a new identity
    IdentityCreated

}

impl Display for IdentityPoolStatusEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            IdentityPoolStatusEnum::Initialized => write!(f, "Initialized"),
            IdentityPoolStatusEnum::Failed => write!(f, "Failed"),
            IdentityPoolStatusEnum::OutOfSync => write!(f, "OutOfSync"),
            IdentityPoolStatusEnum::Ready => write!(f, "Ready"),
            IdentityPoolStatusEnum::Deleting => write!(f, "Deleting"),
            IdentityPoolStatusEnum::Locking => write!(f, "Locking"),
            IdentityPoolStatusEnum::Locked => write!(f, "Locked"),
            IdentityPoolStatusEnum::Unlocking => write!(f, "Unlocking"),
            IdentityPoolStatusEnum::Unlocked => write!(f, "Unlocked"),
            IdentityPoolStatusEnum::CreatingIdentity => write!(f, "CreatingIdentity"),
            IdentityPoolStatusEnum::IdentityCreated => write!(f, "IdentityCreated"),

        }
    }
}

impl IdentityPool {

    /// Handle the creation of IdentityPool resource
    pub async fn create(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let client: Client = context.client.clone();
        let identity_pool_namespace: String = self.namespace().unwrap();
        let identity_pool_name: String= self.name_any();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        info!("[IdentityPool] Starting to create identity {identity_pool_name} in namespace {identity_pool_namespace}");
        self.add_finalizer(client.clone(), &identity_pool_name, &identity_pool_namespace).await.unwrap();
        identity_pool_service_monitor::create_service_monitor(client.clone(), &identity_pool_name, &identity_pool_namespace,  &self.spec.secret_name, owner_references.to_owned()).await?;
        identity_pool_service_account::create_rbac(context.clone(), &identity_pool_namespace, &identity_pool_name, owner_references.to_owned()).await?;
        // TODO: Validate data
        // - Check that the secret exist and contains the required keys
        // - Does the wallet private key have permissions in Network to register new nodes and create safes ?
        // - Does the wallet private key have enough funds to work ?
        self.create_event(context.clone(), IdentityPoolStatusEnum::Initialized).await?;
        self.update_status(context.clone(), IdentityPoolStatusEnum::Initialized).await?;
        info!("[IdentityPool] Identity {identity_pool_name} in namespace {identity_pool_namespace} has been successfully created");
        if self.spec.min_ready_identities == 0 {
            self.create_event(context.clone(), IdentityPoolStatusEnum::Ready).await?;
            self.update_status(context.clone(), IdentityPoolStatusEnum::Ready).await?;
        } else {
            self.create_event(context.clone(), IdentityPoolStatusEnum::OutOfSync).await?;
            self.update_status(context.clone(), IdentityPoolStatusEnum::OutOfSync).await?;
            info!("[IdentityPool] Identity {identity_pool_name} in namespace {identity_pool_namespace} requires to create {} new identities", self.spec.min_ready_identities);
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    /// Handle the modification of IdentityPool resource
    pub async fn modify(&self) -> Result<Action, Error> {
        error!("[IdentityPool] The resource cannot be modified");
        Err(Error::OperationNotSupported(format!("[IdentityPool] The resource cannot be modified").to_owned()))
    }

    // Handle the deletion of IdentityPool resource
    pub async fn delete(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let identity_pool_name = self.name_any();
        let identity_pool_namespace = self.namespace().unwrap();
        let client: Client = context.client.clone();
        self.update_status(context.clone(), IdentityPoolStatusEnum::Deleting).await?;
        self.create_event(context.clone(),  IdentityPoolStatusEnum::Deleting).await?;
        info!("[IdentityPool] Starting to delete identity {identity_pool_name} from namespace {identity_pool_namespace}");
        identity_pool_service_monitor::delete_service_monitor(client.clone(), &identity_pool_name, &identity_pool_namespace).await?;
        identity_pool_service_account::delete_rbac(client.clone(), &identity_pool_namespace, &identity_pool_name).await?;
        self.delete_finalizer(client.clone(), &identity_pool_name, &identity_pool_namespace).await?;
        info!("[IdentityPool] Identity {identity_pool_name} in namespace {identity_pool_namespace} has been successfully deleted");
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }

    pub async fn sync(&self, context: Arc<ContextData>) -> Result<Action, Error> {
        let mut current_ready_identities = self.status.as_ref().unwrap().size - self.status.as_ref().unwrap().locked;
        let mut iterations = (self.spec.min_ready_identities - current_ready_identities) * 2;
        while current_ready_identities < self.spec.min_ready_identities || iterations > 0 {
            iterations -= 1;
            // Invoke Job
            self.create_new_identity(context.clone()).await.unwrap();
            current_ready_identities += 1;
        }
        if current_ready_identities >= self.spec.min_ready_identities {
            self.create_event(context.clone(), IdentityPoolStatusEnum::Ready).await?;
            self.update_status(context.clone(), IdentityPoolStatusEnum::Ready).await?;
        } else {
            self.create_event(context.clone(), IdentityPoolStatusEnum::OutOfSync).await?;
            self.update_status(context.clone(), IdentityPoolStatusEnum::OutOfSync).await?;
            info!("[IdentityPool] Identity {} in namespace {} failed to create required identities", self.name_any(), self.namespace().unwrap());
        }
        Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
    }

    /// Adds a finalizer in IdentityPool to prevent deletion of the resource by Kubernetes API and allow the controller to safely manage its deletion 
    async fn add_finalizer(&self, client: Client, identity_name: &str, identity_namespace: &str) -> Result<(), Error> {
        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &identity_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": [constants::OPERATOR_FINALIZER]
            }
        }));
        match api.patch(&identity_name, &PatchParams::default(), &patch).await {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("[IdentityPool] Could not add finalizer on {identity_name}: {:?}", error);
                return Err(Error::HoprdStatusError(format!("[IdentityPool] Could not add finalizer on {identity_name}.").to_owned()));
            }
        }
    }

    /// Deletes the finalizer of IdentityPool resource, so the resource can be freely deleted by Kubernetes API
    async fn delete_finalizer(&self, client: Client, identity_name: &str, identity_namespace: &str) -> Result<(), Error> {
        let api: Api<IdentityPool> = Api::namespaced(client.clone(), &identity_namespace.to_owned());
        let patch = Patch::Merge(json!({
           "metadata": {
                "finalizers": null
            }
        }));
        if let Some(_) = api.get_opt(&identity_name).await? {
            match api.patch(&identity_name, &PatchParams::default(), &patch).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    Ok(error!("[IdentityPool] Could not delete finalizer on {identity_name}: {:?}", error))
                }
            }
        } else {
            Ok(debug!("[IdentityPool] Identity {identity_name} already deleted"))
        }
    }

    /// Creates an event for IdentityPool given the new IdentityPoolStatusEnum
    pub async fn create_event(&self, context: Arc<ContextData>, status: IdentityPoolStatusEnum) -> Result<(), Error> {
        let client: Client = context.client.clone();   
        let ev: Event = match status {
            IdentityPoolStatusEnum::Initialized => Event {
                        type_: EventType::Normal,
                        reason: "Initialized".to_string(),
                        note: Some("Initializing identity pool".to_owned()),
                        action: "The service monitor has been created".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Failed => Event {
                        type_: EventType::Warning,
                        reason: "Failed".to_string(),
                        note: Some("Failed to bootstrap identity pool".to_owned()),
                        action: "Identity pool bootstrap validations have failed".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::OutOfSync => Event {
                        type_: EventType::Normal,
                        reason: "OutOfSync".to_string(),
                        note: Some("The identity pool is out of sync".to_owned()),
                        action: "The identity pool need to create more identities".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Ready => Event {
                        type_: EventType::Normal,
                        reason: "Ready".to_string(),
                        note: Some("Identity pool ready to be used".to_owned()),
                        action: "Identity pool is ready to be used by a Hoprd node".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Deleting => Event {
                        type_: EventType::Normal,
                        reason: "Deleting".to_string(),
                        note: Some("Identity pool is being deleted".to_owned()),
                        action: "Identity pool deletion started".to_string(),
                        secondary: None
            },
            IdentityPoolStatusEnum::Locking => Event {
                        type_: EventType::Normal,
                        reason: "Locking".to_string(),
                        note: Some("Locking identity from pool".to_owned()),
                        action: "Locking a ready identity from pool".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Locked => Event {
                        type_: EventType::Normal,
                        reason: "Locked".to_string(),
                        note: Some("Identity locked from pool".to_owned()),
                        action: "Identity sucessfully locked from pool".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Unlocking => Event {
                        type_: EventType::Normal,
                        reason: "Unlocking".to_string(),
                        note: Some("Unlocking identity from pool".to_owned()),
                        action: "Unlocking a InUse identity from pool".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::Unlocked => Event {
                        type_: EventType::Normal,
                        reason: "Unlocked".to_string(),
                        note: Some("Identity unlocked from pool".to_owned()),
                        action: "Identity successfully unlocked from pool".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::CreatingIdentity => Event {
                        type_: EventType::Normal,
                        reason: "CreatingIdentity".to_string(),
                        note: Some("Starting to sync an identity pool".to_owned()),
                        action: "Synching failed identity pool".to_string(),
                        secondary: None
                    },
            IdentityPoolStatusEnum::IdentityCreated => Event {
                        type_: EventType::Normal,
                        reason: "IdentityCreated".to_string(),
                        note: Some("Starting to sync an identity pool".to_owned()),
                        action: "Synching failed identity pool".to_string(),
                        secondary: None
                    },
        };
        let recorder: Recorder = context.state.read().await.generate_identity_pool_event(client.clone(), self);
        Ok(recorder.publish(ev).await?)
    }

    /// Updates the status of IdentityPool
    pub async fn update_status(&self, context: Arc<ContextData>, status: IdentityPoolStatusEnum) -> Result<(), Error> {
    let client: Client = context.client.clone();
    let identity_hoprd_name = self.metadata.name.as_ref().unwrap().to_owned();    
    let hoprd_namespace = self.metadata.namespace.as_ref().unwrap().to_owned();
    let mut identity_pool_status = self.status.as_ref().unwrap_or(&IdentityPoolStatus::default()).to_owned();

    let api: Api<IdentityPool> = Api::namespaced(client.clone(), &hoprd_namespace.to_owned());
    if status.eq(&IdentityPoolStatusEnum::Deleting) {
        Ok(())
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        self.spec.clone().hash(&mut hasher);
        let checksum: String = hasher.finish().to_string();
        if status.eq(&IdentityPoolStatusEnum::IdentityCreated) 
        { 
            if (identity_pool_status.size - identity_pool_status.locked + 1) >= self.spec.min_ready_identities {
                identity_pool_status.status = IdentityPoolStatusEnum::Ready;
            } else {
                identity_pool_status.status = IdentityPoolStatusEnum::OutOfSync;
            }
            identity_pool_status.size = identity_pool_status.size + 1
        };

        if status.eq(&IdentityPoolStatusEnum::Locked)
        {
            if (identity_pool_status.size - identity_pool_status.locked - 1) >= self.spec.min_ready_identities {
                identity_pool_status.status = IdentityPoolStatusEnum::Ready;
            } else {
                identity_pool_status.status = IdentityPoolStatusEnum::OutOfSync;
            }
            identity_pool_status.locked = identity_pool_status.locked + 1 
        } 
        else if status.eq(&IdentityPoolStatusEnum::Unlocked) 
        { 
            if (identity_pool_status.size - identity_pool_status.locked + 1) >= self.spec.min_ready_identities {
                identity_pool_status.status = IdentityPoolStatusEnum::Ready;
            } else {
                identity_pool_status.status = IdentityPoolStatusEnum::OutOfSync;
            }
            identity_pool_status.locked = identity_pool_status.locked - 1 
        };
        identity_pool_status.update_timestamp = Utc::now().to_rfc3339();
        identity_pool_status.checksum = checksum;
        identity_pool_status.status = status;

        let patch = Patch::Merge(json!({
                "status": identity_pool_status
        }));
        //self.status = Some(identity_pool_status);
        match api.patch(&identity_hoprd_name, &PatchParams::default(), &patch).await {
            Ok(_identity) => Ok(()),
            Err(error) => {
                Ok(error!("[IdentityPool] Could not update status on {identity_hoprd_name}: {:?}", error))
            }
        }
    }
}

    // pub async fn get_identities(&self, client: Client, status: &IdentityHoprdStatusEnum) -> Result<Vec<IdentityHoprd>,Error> {
    //     let api: Api<IdentityHoprd> = Api::namespaced(client.clone(), &self.namespace().unwrap().to_owned());
    //     let identities = api.list(&ListParams::default()).await.unwrap();
    //     let filtered_identities: Vec<IdentityHoprd> = identities.iter()
    //     .filter(|&identity| 
    //         identity.metadata.owner_references.as_ref().unwrap().first().unwrap().name.eq(&self.name_any()) &&
    //         identity.status.as_ref().unwrap().status.eq(status))
    //     .map(|i| i.clone())
    //     .collect::<Vec<IdentityHoprd>>();
    //     Ok(filtered_identities)
    // }

    /// Gets the first identity in ready status
    pub async fn lock_identity(&self, context: Arc<ContextData>, identity_name: &str) -> Result<Option<IdentityHoprd>,Error> {
        self.create_event(context.clone(), IdentityPoolStatusEnum::Locking).await?;
        let api: Api<IdentityHoprd> = Api::namespaced(context.client.clone(), &self.namespace().unwrap().to_owned());
        let identities = api.list(&ListParams::default()).await.unwrap();
        let first_identity = identities.iter()
        .filter(|&identity| 
            identity.metadata.owner_references.as_ref().unwrap().first().unwrap().name.eq(&self.name_any()))
        .next();
        return match first_identity {
            Some(identity) => {
                 if identity.to_owned().status.unwrap().status.eq(&IdentityHoprdStatusEnum::Ready) {
                    identity.create_event(context.clone(), IdentityHoprdStatusEnum::InUse, Some(identity_name.to_owned())).await?;
                    identity.update_status(context.clone(), IdentityHoprdStatusEnum::InUse, Some(identity_name.to_owned())).await?;
                    self.create_event(context.clone(), IdentityPoolStatusEnum::Locked).await?;
                    self.update_status(context.clone(), IdentityPoolStatusEnum::Locked).await?;
                    Ok(Some(identity.to_owned()))
                } else {
                     warn!("The identity {} is in state {} and might be used by {}", identity_name, identity.status.as_ref().unwrap().status, identity.status.as_ref().unwrap().hoprd_node_name.as_ref().unwrap());
                    Ok(None)
                }
        },
            None => Err(Error::IdentityIssue(format!("The identity {} does not exist", identity_name).to_owned()))
        };
        
    }

    async fn create_new_identity(&self, context: Arc<ContextData>) -> Result<(), Error> {
        self.create_event(context.clone(), IdentityPoolStatusEnum::CreatingIdentity).await?;
        let identity_name = format!("{}-{}", self.name_any(),  self.status.as_ref().unwrap().size + 1);
        let job_name: String = format!("create-identity-{}",&identity_name.to_owned());
        let namespace: String = self.metadata.namespace.as_ref().unwrap().to_owned();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![self.controller_owner_ref(&()).unwrap()]);
        let mut labels: BTreeMap<String, String> = utils::common_lables(context.config.instance.name.to_owned(),Some(identity_name.to_owned()), Some("job-create-identity".to_owned()));
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "create-identity".to_owned());
        let create_identity_args: Vec<String> = vec![format!("curl {}/create-identity.sh -s | bash", constants::OPERATOR_JOB_SCRIPT_URL.to_owned())];
        let create_resource_args: Vec<String> = vec![format!("curl {}/create-resource.sh -s | bash", constants::OPERATOR_JOB_SCRIPT_URL.to_owned())];
        let env_vars: Vec<EnvVar> = vec![EnvVar {
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
            name: constants::IDENTITY_POOL_WALLET_PRIVATE_KEY_REF_KEY.to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: constants::IDENTITY_POOL_WALLET_PRIVATE_KEY_REF_KEY.to_owned(),
                    name: Some(self.spec.secret_name.to_owned()),
                    ..SecretKeySelector::default()
                }),
                ..EnvVarSource::default()
            }),
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
            value: Some(identity_name),
            ..EnvVar::default()
        },
        EnvVar {
            name: constants::HOPRD_NETWORK.to_owned(),
            value: Some(self.spec.network.to_owned()),
            ..EnvVar::default()
        }];
        let volumes: Vec<Volume> = vec![
            Volume {
                name: "hoprd-identity-created".to_owned(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Volume::default()
            }];
        let volume_mounts: Vec<VolumeMount> = vec![
            VolumeMount {
                name: "hoprd-identity-created".to_owned(),
                mount_path: "/app/hoprd-identity-created".to_owned(),
                ..VolumeMount::default()
            }
        ];

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
                            image: Some(context.config.hopli_image.to_owned()),
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
                            command: Some(vec!["/bin/bash".to_owned(), "-c".to_owned()]),
                            args: Some(create_resource_args),
                            env: Some(env_vars),
                            volume_mounts: Some(volume_mounts),
                            resources:Some(HoprdDeploymentSpec::get_resource_requirements(None)),
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
        let api: Api<Job> = Api::namespaced(context.client.clone(), &namespace);
        api.create(&PostParams::default(), &create_node_job).await.unwrap();
        let job_completed = await_condition(api, &job_name, conditions::is_job_completed());
        match tokio::time::timeout(std::time::Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT), job_completed).await {
            Ok(_) => {
                self.create_event(context.clone(), IdentityPoolStatusEnum::IdentityCreated).await?;
                self.update_status(context.clone(), IdentityPoolStatusEnum::IdentityCreated).await?;
                Ok(info!("Job {} completed successfully", &job_name.to_owned()))
            },
            Err(_error) => {
                Err(Error::JobExecutionError(format!(" Job execution for {} failed", &job_name.to_owned()).to_owned()))
            }
        }
    }

}


