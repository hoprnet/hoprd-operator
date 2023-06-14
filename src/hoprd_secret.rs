use k8s_openapi::{api::{core::v1::Secret}, apimachinery::pkg::apis::meta::v1::OwnerReference, ByteString};
use kube::{Api, Client, api::{ Patch, ListParams, PatchParams, DeleteParams, PostParams}, ResourceExt, Resource, core::{ObjectMeta}};
use serde_json::{json};
use std::{collections::{BTreeMap}, sync::Arc};
use tracing::{debug, info, warn, error};
use rand::{distributions::Alphanumeric, Rng};
use async_recursion::async_recursion;
use crate::{
    model::{ HoprdSecret, Error}, operator_config::OperatorConfig, utils, constants, hoprd_jobs::{HoprdJob}, hoprd::Hoprd,  context_data::ContextData
};

/// Action to be taken upon an `Hoprd` resource during reconciliation
enum SecretStatus {
    /// The secret details has not been provided within the Hoprd configuration
    NotSpecified,
    /// The secret has been specified but does not exist yet
    NotExists,
    /// The secret exists and but it is not ready to be used because is not registered in the NetworkRegistry
    NotRegistered,
    /// The secret exists and but it is not ready to be used because is not funded
    NotFunded,
    /// The secret exists and it is currently being used by other existing node
    Locked,
    /// The secret exists and it is ready to be used
    Ready,
    /// The secret is in unknown status
    Unknown
}

pub struct SecretManager {
    context: Arc<ContextData>,
    client: Client,
    operator_config: OperatorConfig,
    hoprd: Hoprd,
    pub hoprd_secret: Option<HoprdSecret>,
    job_manager: HoprdJob

}

impl SecretManager {

    pub fn new(context: Arc<ContextData>, hoprd: Hoprd) -> Self {
        let client = context.client.clone();
        let operator_config = context.config.clone();
        let job_manager = HoprdJob::new(client.clone(), operator_config.clone(), hoprd.clone());
        Self { context, client, operator_config: operator_config, hoprd, hoprd_secret: None, job_manager }
    }

    /// Evaluates the existence of the secret required labels and its correctness
    ///
    /// # Arguments
    /// - `secret_labels` - Labels assigned to the secret
    ///
    fn check_secret_labels(&self, secret_labels: &BTreeMap<String, String>) -> Result<bool,Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        if secret_labels.contains_key(constants::LABEL_NODE_NETWORK) {
            let network_label: String = secret_labels.get_key_value(constants::LABEL_NODE_NETWORK).unwrap().1.parse().unwrap();
            if ! network_label.eq(&self.hoprd.spec.network.to_owned()) {
                error!("The secret specified {secret_name} belongs to '{network_label}' network which is different from the specified '{}' network", self.hoprd.spec.network);
                return Ok(false);
            }
        } else {
            error!("The secret specified {secret_name} does not contain label {} which is mandatory", constants::LABEL_NODE_NETWORK);
            return Ok(false);
        }
        Ok(true)
    }

    /// Gets the first secret that is ready to be used
    async fn get_first_secret_ready(&self) -> Result<Option<Secret>, Error> {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        let label_selector: String = format!("{}={},{}={},{}",
        constants::LABEL_NODE_NETWORK, self.hoprd.spec.network,
        constants::LABEL_NODE_LOCKED, "false",
        constants::LABEL_NODE_PEER_ID);
        let lp = ListParams::default().labels(&label_selector);
        let secrets = api.list(&lp).await?;
        Ok(secrets.items.first().map(|secret| secret.to_owned()))
    }

    /// Gets the Kubernetes secret linked to the Hoprd node by its OwnedReferences
    pub async fn get_hoprd_secret(&self) -> Result<Option<Secret>, Error> {
        let api: Api<Secret> = Api::namespaced(self.client.clone(),& self.operator_config.instance.namespace);
        let label_selector: String = format!("{}={},{}={}",
        constants::LABEL_NODE_NETWORK, self.hoprd.spec.network,
        constants::LABEL_NODE_LOCKED, "true");
        let lp = ListParams::default().labels(&label_selector);
        let secrets = api.list(&lp).await?;
        let secret = secrets
            .iter()
            .find(|secret| { 
                let empty_references = &Vec::new();
                let reference = secret.metadata.owner_references.as_ref().unwrap_or(empty_references).first();
                reference.is_some() && reference.unwrap().name == self.hoprd.name_any()
            })
            .map(|secret| secret.to_owned());
        Ok(secret)
    }

    /// Gets the Kubernetes secret used by the hoprd-operator to register and fund nodes 
    pub async fn get_wallet_secret(&self) -> Result<Secret, Error> {
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        if let Some(wallet_secret) = api.get_opt(&self.operator_config.instance.secret_name).await? {
            Ok(wallet_secret)
        } else {
            Err(Error::SecretStatusError("[ERROR] Could not get wallet secret".to_owned()))
        }
    }

    /// Evaluates the status of the secret based on `SecretStatus` to determine later which actions need to be taken
    async fn determine_secret_status(&mut self) -> Result<SecretStatus,Error> {
        return if self.hoprd.spec.secret.is_none() && self.hoprd_secret.is_none() {
            info!("Hoprd node {:?} has not specified a secret in its spec", self.hoprd.name_any());
            Ok(SecretStatus::NotSpecified)
        } else {
            let client: Client = self.client.clone();
            let operator_namespace = &self.operator_config.instance.namespace.to_owned();
            let hoprd_secret = match self.hoprd.spec.secret.as_ref() {
                Some(secret) => { 
                    self.hoprd_secret = Some(secret.to_owned());
                    secret
                },
                None => self.hoprd_secret.as_ref().unwrap()
            };
            let api_secrets: Api<Secret> = Api::namespaced(client.clone(), &operator_namespace);
            let secret_name = hoprd_secret.secret_name.to_owned();


            if let Some(secret) = api_secrets.get_opt(&secret_name).await? {
                let empty_map = &BTreeMap::new();
                let secret_annotations: &BTreeMap<String, String> = secret.metadata.annotations.as_ref().unwrap_or_else(|| empty_map);
                let secret_labels: &BTreeMap<String, String> = secret.metadata.labels.as_ref().unwrap_or_else(|| empty_map);
                if ! self.check_secret_labels(secret_labels).unwrap() {
                    return Ok(SecretStatus::Unknown)
                }
                if secret_annotations.contains_key(constants::ANNOTATION_HOPRD_NETWORK_REGISTRY) {
                    let network_registry_annotation: bool = secret_annotations.get_key_value(constants::ANNOTATION_HOPRD_NETWORK_REGISTRY).unwrap().1.parse().unwrap();
                    if ! network_registry_annotation {
                        info!("The secret {} exists but is not registered", secret_name);
                        return Ok(SecretStatus::NotRegistered)
                    }
                } else {
                    info!("The secret {} exists but is not registered", secret_name);
                    return Ok(SecretStatus::NotRegistered)
                }
                if secret_annotations.contains_key(constants::ANNOTATION_HOPRD_FUNDED) {
                    let node_funded_annotation: bool = secret_annotations.get_key_value(constants::ANNOTATION_HOPRD_FUNDED).unwrap().1.parse().unwrap();
                    if ! node_funded_annotation {
                        info!("The secret {secret_name} exists but is not funded");
                        return Ok(SecretStatus::NotFunded)
                    }
                } else {
                    info!("The secret {secret_name} exists but is not funded");
                    return Ok(SecretStatus::NotFunded)
                }
                if secret_labels.contains_key(constants::LABEL_NODE_LOCKED) {
                    let node_locked_annotation = secret_labels.get_key_value(constants::LABEL_NODE_LOCKED).unwrap().1.parse().unwrap();
                    if node_locked_annotation {
                        return Ok(SecretStatus::Locked);
                    }
                }
                info!("Hoprd node {:?} is ready to use the available secret {secret_name}", self.hoprd.name_any());
                return Ok(SecretStatus::Ready);
            } else {
                info!("Hoprd node {:?} has specified a secret {secret_name} which does not exists yet", self.hoprd.name_any());
                return Ok(SecretStatus::NotExists);
            };
        };
    }

    /// Creates a new secret for storing sensitive data of the hoprd node,
    #[async_recursion]
    pub async fn create_secret(&mut self) -> Result<Secret, Error> {
        return match self.determine_secret_status().await? {
            SecretStatus::NotSpecified => self.do_status_not_specified().await,
            SecretStatus::NotExists => self.do_status_not_exists().await,
            SecretStatus::NotRegistered => self.do_status_not_registered().await,
            SecretStatus::NotFunded => self.do_status_not_funded().await,
            SecretStatus::Locked => self.do_status_locked().await,
            SecretStatus::Ready => self.do_status_ready().await,
            SecretStatus::Unknown => Err(Error::HoprdStatusError(format!("The secret is in unknown status").to_owned()))
        }
    }

    /// Unlocks a given secret from a Hoprd node
    pub async fn unlock_secret(&self) -> Result<(), Error> {
        let client: Client = self.client.clone();
        let operator_namespace = &self.operator_config.instance.namespace.to_owned();
        let api: Api<Secret> = Api::namespaced(client.clone(), &operator_namespace);
        if let Some(secret) = self.get_hoprd_secret().await? {
            let secret_name = &secret.metadata.name.unwrap();
            utils::update_secret_label(&api.clone(), &secret_name, constants::LABEL_NODE_LOCKED, &"false".to_string()).await?;
            utils::delete_secret_annotations(&api.clone(), &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES).await?;
            let wallet_secret = self.get_wallet_secret().await.unwrap();
            let owner_references: Option<Vec<OwnerReference>> = Some(vec![wallet_secret.controller_owner_ref(&()).unwrap()]);
            let patch = Patch::Merge(json!({
                        "metadata": {
                            "ownerReferences": owner_references
                        }
                }));
            return match api.patch(secret_name, &PatchParams::default(), &patch).await {
                Ok(_) => {
                    let api_secrets: Api<Secret> = Api::namespaced(client.clone(), &self.hoprd.namespace().unwrap().to_owned());
                    if let Some(_secret) = api_secrets.get_opt(&secret_name).await? {
                        api_secrets.delete(&secret_name, &DeleteParams::default()).await?;
                    }
                    Ok(info!("The secret '{secret_name}' has been unlocked"))
                },
                Err(error) => {
                    error!("Could not delete secret owned references for '{secret_name}': {:?}", error);
                    Err(Error::HoprdStatusError(format!("Could not delete secret owned references for '{secret_name}'.").to_owned()))
                }
            };
        } else {
            Ok(warn!("The hoprd node did not own a secret {:?}", &self.hoprd.name_any().to_owned()))
        }
    }

    /// The secret has not been specified in the config. The config of the node will be updated with the parameters for a new secret
    async fn do_status_not_specified(&mut self) -> Result<Secret, Error> {
        match self.get_first_secret_ready().await {
            Ok(secret) => { 
                match secret {
                    Some(secret) => {
                        let secret_name = secret.metadata.name.unwrap();
                        self.hoprd_secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                        return self.create_secret().await;
                    }
                    None => {
                        let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
                        let mut secret_name = String::from("hoprd-node-");
                        secret_name.push_str(&self.hoprd.spec.network.replace("_", "-"));
                        secret_name.push_str(&"-");
                        secret_name.push_str(&random_string.to_lowercase());
                        self.hoprd_secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                        return self.do_status_not_exists().await;
                    }
                }
            }
            Err(_err) => {
                println!("[ERROR]: {:?}", _err);
                return Err(Error::SecretStatusError(
                    format!("Could not retrieve a previous existing secret")
                        .to_owned()
                ));
            }
        }
    }

    /// The secret does not exists yet but has been specified in the config. A Job will be triggered to get the elements needed for running node:
    async fn do_status_not_exists(&mut self) -> Result<Secret, Error> {
        self.hoprd.create_event(self.context.clone(), crate::model::HoprdStatusEnum::Creating).await?;
        self.hoprd.update_status(self.context.clone(), crate::model::HoprdStatusEnum::Creating).await?;
        let secret = self.create_secret_resource().await.unwrap();
        let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
        match self.job_manager.execute_job_create_node(&self.hoprd_secret.as_ref().unwrap(), owner_reference).await {
            Ok(_) => self.do_status_not_registered().await,
            Err(err) => {
                self.delete_finalizer().await?;
                let api_secret: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
                api_secret.delete(&secret.name_any(), &DeleteParams::default()).await?
                    .map_left(|_| info!("Deleting empty secret: {:?}", &secret.name_any()))
                    .map_right(|_| info!("Deleted  empty secret: {:?}", &secret.name_any()));
                Err(err)
            }
        }
    }

    /// The secret exists but can not be used yet as it is not registered. Before using it will trigger a Job to register the node
    async fn do_status_not_registered(&mut self) -> Result<Secret, Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        let secret = api.get(&secret_name).await.unwrap();
        let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
        self.hoprd.create_event(self.context.clone(), crate::model::HoprdStatusEnum::RegisteringInNetwork).await?;
        self.hoprd.update_status(self.context.clone(), crate::model::HoprdStatusEnum::RegisteringInNetwork).await?;
        self.job_manager.execute_job_registering_node( &self.hoprd_secret.as_ref().unwrap(), owner_reference).await?;
        utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_NETWORK_REGISTRY, "true").await?;
        self.do_status_not_funded().await
    }

    /// The secret exists but can not be used yet as it is not funded. Before using it will trigger a Job to fund the node
    async fn do_status_not_funded(&mut self) -> Result<Secret, Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        let secret = api.get(&secret_name).await.unwrap();
        let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
        self.hoprd.create_event(self.context.clone(), crate::model::HoprdStatusEnum::Funding).await?;
        self.hoprd.update_status(self.context.clone(), crate::model::HoprdStatusEnum::Funding).await?;
        self.job_manager.execute_job_funding_node( &self.hoprd_secret.as_ref().unwrap(),  owner_reference).await?;
        let api: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_FUNDED, "true").await?;
        return self.do_status_ready().await;
    }

    /// The secret exists but it is locked by other node. It will raise an error specifying that the secret reference needs to be updated to an other secret or just remove it to create a new one.
    async fn do_status_locked(&mut self) -> Result<Secret, Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        return Err(Error::SecretStatusError(
            format!("The secret {secret_name} in namespace {} is already locked by other hoprd node. See details above.", self.hoprd.namespace().unwrap())
                .to_owned()
        ));
    }

    /// The secret exists and is ready to be used by the hoprd node. It will create the annotations and labels for locking the secret
    async fn do_status_ready(&mut self) -> Result<Secret, Error> {
        let hoprd_namespace = &self.hoprd.namespace().unwrap();
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        let api_secret: Api<Secret> = Api::namespaced(self.client.clone(), &self.operator_config.instance.namespace);
        utils::update_secret_annotations(&api_secret, &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES, hoprd_namespace).await?;
        self.hoprd.create_event(self.context.clone(), crate::model::HoprdStatusEnum::Running).await?;
        self.hoprd.update_status(self.context.clone(), crate::model::HoprdStatusEnum::Running).await?;
        utils::update_secret_label(&api_secret, &secret_name, constants::LABEL_NODE_LOCKED, &"true".to_string()).await?;
        let owner_reference: Option<Vec<OwnerReference>> = Some(vec![self.hoprd.controller_owner_ref(&()).unwrap()]);
        let patch = Patch::Merge(json!({
                    "metadata": {
                        "ownerReferences": owner_reference 
                    }
        }));
        match api_secret.patch(&secret_name, &PatchParams::default(), &patch).await {
            Ok(secret) => Ok(secret),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                Err(Error::HoprdStatusError(format!("Could not update secret owned references for '{secret_name}'.").to_owned()))
            }
        }
    }

    /// Creates the underlying Kubernetes Secret resource
    async fn create_secret_resource(&mut self) -> Result<Secret, Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        let operator_namespace = &self.operator_config.instance.namespace;
        let mut labels: BTreeMap<String, String> = utils::common_lables(&secret_name.to_owned());
        labels.insert(constants::LABEL_NODE_NETWORK.to_owned(), self.hoprd.spec.network.to_owned());
        labels.insert(constants::LABEL_NODE_LOCKED.to_owned(), "false".to_owned());
        let mut data: BTreeMap<String, ByteString> = BTreeMap::new();
        data.insert(constants::HOPRD_METRICS_PASSWORD.to_owned(), ByteString("".to_owned().into_bytes()));

        let deployment: Secret = Secret {
            metadata: ObjectMeta {
                name: Some(secret_name.to_owned()),
                namespace: Some(operator_namespace.to_owned()),
                labels: Some(labels.clone()),
                finalizers: Some(vec![constants::FINALIZER_SECRET.to_owned()]),
                ..ObjectMeta::default()
            },
            data: Some(data),
            type_: Some("Opaque".to_owned()),
            ..Secret::default()
        };

        // Create the secret defined above
        let api: Api<Secret> = Api::namespaced(self.client.clone(), operator_namespace);
        Ok(api.create(&PostParams::default(), &deployment).await?)
    }

    /// Removes all finalizers from the secret
    async fn delete_finalizer(&self) -> Result<(), Error> {
        let secret_name: String = self.hoprd_secret.as_ref().unwrap().secret_name.to_owned();
        let operator_namespace = &self.operator_config.instance.namespace;
        let api: Api<Secret> = Api::namespaced(self.client.clone(), operator_namespace);
        let patch = &Patch::Merge(json!({
           "metadata": {
                "finalizers": null
            }
        }));
        if let Some(_) = api.get_opt(&secret_name).await? {
            match api.patch(&secret_name, &PatchParams::default(), patch).await {
                Ok(_hopr) => Ok(()),
                Err(error) => {
                    println!("[ERROR]: {:?}", error);
                    return Err(Error::HoprdStatusError(format!("Could not delete finalizer on secret {secret_name}.").to_owned()));
                }
            }
        } else {
            Ok(debug!("Secret {secret_name} has already been deleted"))
        }
    }

}

