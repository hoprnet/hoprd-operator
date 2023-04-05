use k8s_openapi::{api::{core::v1::Secret}, apimachinery::pkg::apis::meta::v1::OwnerReference, ByteString};
use kube::{Api, Client, api::{ Patch, ListParams, PatchParams, DeleteParams, PostParams}, ResourceExt, Resource, core::ObjectMeta};
use serde_json::{json};
use std::{collections::{BTreeMap}, sync::Arc};

use rand::{distributions::Alphanumeric, Rng};
use async_recursion::async_recursion;
use crate::{
    model::{ Secret as HoprdSecret, Error, OperatorInstance}, utils, constants, hoprd_jobs, hoprd::Hoprd, hoprd::HoprdSpec, context_data::ContextData
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
    Ready
}

/// Evaluates the existence of the secret required labels and its correctness
///
/// # Arguments
/// - `secret_labels` - Labels assigned to the secret
/// - `hoprd_spec` - Details about the hoprd configuration node
///
fn check_secret_labels(secret_labels: &BTreeMap<String, String>, hoprd_spec: &HoprdSpec) -> Result<(),Error> {
    let secret_name: String = hoprd_spec.secret.as_ref().unwrap().secret_name.to_owned();
    if secret_labels.contains_key(constants::LABEL_NODE_NETWORK) {
        let network_label: String = secret_labels.get_key_value(constants::LABEL_NODE_NETWORK).unwrap().1.parse().unwrap();
        if ! network_label.eq(&hoprd_spec.network.to_owned()) {
            return Err(Error::SecretStatusError(
                format!("[ERROR] The secret specified {secret_name} belongs to '{network_label}' network which is different from the specified '{}' network", hoprd_spec.network)
                    .to_owned()
            ));
        }
    } else {
        return Err(Error::SecretStatusError(
            format!("[ERROR] The secret specified {secret_name} does not contain label {} which is mandatory", constants::LABEL_NODE_NETWORK)
                .to_owned()
        ));
    }
    Ok(())
}

/// Gets the first secret that is ready to be used
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `hoprd_spec` - Details about the hoprd configuration node
///
async fn get_first_secret_ready(client: Client, network: &str, operator_namespace: &str) -> Result<Option<Secret>, Error> {
    let api: Api<Secret> = Api::namespaced(client, operator_namespace);
    let label_selector: String = format!("{}={},{}={}",
    constants::LABEL_NODE_NETWORK, network,
    constants::LABEL_NODE_LOCKED, "false");
    let lp = ListParams::default().labels(&label_selector);
    let secrets = api.list(&lp).await?;
    Ok(secrets.items.first().map(|secret| secret.to_owned()))
}

/// Gets the first secret that is ready to be used
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `hoprd_spec` - Details about the hoprd configuration node
///
pub async fn get_secret_used_by(client: Client, network: &str, hoprd_name: &str, operator_namespace: &str) -> Result<Option<Secret>, Error> {
    let api: Api<Secret> = Api::namespaced(client, operator_namespace);
    let label_selector: String = format!("{}={},{}={}",
    constants::LABEL_NODE_NETWORK, network,
    constants::LABEL_NODE_LOCKED, "true");
    let lp = ListParams::default().labels(&label_selector);
    let secrets = api.list(&lp).await?;
    let secret = secrets
        .iter()
        .find(|secret| { 
            let empty_references = &Vec::new();
            let reference = secret.metadata.owner_references.as_ref().unwrap_or(empty_references).first();
            reference.is_some() && reference.unwrap().name == hoprd_name
        })
        .map(|secret| secret.to_owned());
    Ok(secret)
}

/// Gets the wallet secret linked to the hoprd-operator
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `operator_instance` - Info about operator instance
///
pub async fn get_wallet_secret(client: Client, operator_instance: &OperatorInstance) -> Result<Secret, Error> {
    let api: Api<Secret> = Api::namespaced(client, &operator_instance.namespace);
    if let Some(wallet_secret) = api.get_opt(&operator_instance.name).await? {
        Ok(wallet_secret)
    } else {
        Err(Error::SecretStatusError("[ERROR] Could not get wallet secret".to_owned()))
    }
}

/// Evaluates the status of the secret based on `SecretStatus` to determine later which actions need to be taken
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
///
async fn determine_secret_status(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<SecretStatus,Error> {
    return if hoprd.spec.secret.is_none() {
        println!("[INFO] The secret has not been specified");
        Ok(SecretStatus::NotSpecified)
    } else {
        let client: Client = context.client.clone();
        let operator_namespace = &context.config.instance.namespace.to_owned();
        let hoprd_secret: &HoprdSecret = hoprd.spec.secret.as_ref().unwrap();
        let api_secrets: Api<Secret> = Api::namespaced(client.clone(), &operator_namespace);
        let secret_name = hoprd_secret.secret_name.to_owned();


        if let Some(secret) = api_secrets.get_opt(&secret_name).await? {
            let empty_map = &BTreeMap::new();
            let secret_annotations: &BTreeMap<String, String> = secret.metadata.annotations.as_ref().unwrap_or_else(|| empty_map);
            let secret_labels: &BTreeMap<String, String> = secret.metadata.labels.as_ref().unwrap_or_else(|| empty_map);
            check_secret_labels(secret_labels, &hoprd.spec).unwrap();
            if secret_annotations.contains_key(constants::ANNOTATION_HOPRD_NETWORK_REGISTRY) {
                let network_registry_annotation: bool = secret_annotations.get_key_value(constants::ANNOTATION_HOPRD_NETWORK_REGISTRY).unwrap().1.parse().unwrap();
                if ! network_registry_annotation {
                    println!("[INFO] The secret exists but is not registered");
                    return Ok(SecretStatus::NotRegistered)
                }
            } else {
                println!("[INFO] The secret exists but is not registered");
                return Ok(SecretStatus::NotRegistered)
            }
            if secret_annotations.contains_key(constants::ANNOTATION_HOPRD_FUNDED) {
                let node_funded_annotation: bool = secret_annotations.get_key_value(constants::ANNOTATION_HOPRD_FUNDED).unwrap().1.parse().unwrap();
                if ! node_funded_annotation {
                    println!("[INFO] The secret {secret_name} exists but is not funded");
                    return Ok(SecretStatus::NotFunded)
                }
            } else {
                println!("[INFO] The secret {secret_name} exists but is not funded");
                return Ok(SecretStatus::NotFunded)
            }
            if secret_labels.contains_key(constants::LABEL_NODE_LOCKED) {
                let node_locked_annotation = secret_labels.get_key_value(constants::LABEL_NODE_LOCKED).unwrap().1.parse().unwrap();
                if node_locked_annotation {
                    return Ok(SecretStatus::Locked);
                }
            }
            println!("[INFO] The secret {secret_name} is ready to be used");
            return Ok(SecretStatus::Ready);
        } else {
            println!("[INFO] The secret is specified but does not exists yet");
            return Ok(SecretStatus::NotExists);
        };
    };
}

/// Creates a new secret for storing sensitive data of the hoprd node,
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
///
#[async_recursion]
pub async fn create_secret(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    return match determine_secret_status(context.clone(), hoprd).await? {
        SecretStatus::NotSpecified => do_status_not_specified(context.clone(), hoprd).await,
        SecretStatus::NotExists => do_status_not_exists(context.clone(), hoprd).await,
        SecretStatus::NotRegistered => do_status_not_registered(context.clone(), hoprd).await,
        SecretStatus::NotFunded => do_status_not_funded(context.clone(), hoprd).await,
        SecretStatus::Locked => do_status_locked(hoprd).await,
        SecretStatus::Ready => do_status_ready(context.clone(), hoprd).await
    }
}

/// Unlocks a given secret from a Hoprd node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
///
pub async fn unlock_secret(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<(), Error> {
    let client: Client = context.client.clone();
    let operator_namespace = &context.config.instance.namespace.to_owned();
    let api: Api<Secret> = Api::namespaced(client.clone(), &operator_namespace);
    if let Some(secret) = get_secret_used_by(client.clone(), &hoprd.spec.network, &hoprd.name_any(), operator_namespace).await? {
        let secret_name = &secret.metadata.name.unwrap();
        utils::update_secret_label(&api.clone(), &secret_name, constants::LABEL_NODE_LOCKED, &"false".to_string()).await?;
        utils::delete_secret_annotations(&api.clone(), &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES).await?;
        let wallet_secret = get_wallet_secret(client.clone(), &context.config.instance).await.unwrap();
        let owner_references: Option<Vec<OwnerReference>> = Some(vec![wallet_secret.controller_owner_ref(&()).unwrap()]);
        let patch = Patch::Merge(json!({
                    "metadata": {
                        "ownerReferences": owner_references
                    }
            }));
        let _secret = match api.patch(secret_name, &PatchParams::default(), &patch).await {
            Ok(secret) => Ok(secret),
            Err(error) => {
                println!("[ERROR]: {:?}", error);
                Err(Error::HoprdStatusError(format!("Could not delete secret owned references for '{secret_name}'.").to_owned()))
            }
        };
        let api_secrets: Api<Secret> = Api::namespaced(client.clone(), &hoprd.namespace().unwrap());
        if let Some(_secret) = api_secrets.get_opt(&secret_name).await? {
            api_secrets.delete(&secret_name, &DeleteParams::default()).await?;
        }
        Ok(println!("[INFO] The secret '{secret_name}' has been unlocked"))
    } else {
        Ok(println!("[WARN] The hoprd node did not own a secret '{:?}' ", &hoprd.name_any()))
    }
}

/// The secret has not been specified in the config. The config of the node will be updated with the parameters for a new secret
/// 
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_specified(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_namespace = &context.config.instance.namespace.to_owned();
    match get_first_secret_ready(client.clone(), &hoprd.spec.network, operator_namespace).await {
        Ok(secret) => { 
            match secret {
                Some(secret) => {
                    let secret_name = secret.metadata.name.unwrap();
                    hoprd.spec.secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                    return create_secret(context, hoprd).await;
                }
                None => {
                    let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
                    let mut secret_name = String::from("hoprd-node-");
                    secret_name.push_str(&hoprd.spec.network.replace("_", "-"));
                    secret_name.push_str(&"-");
                    secret_name.push_str(&random_string.to_lowercase());
                    hoprd.spec.secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                    return do_status_not_exists(context, hoprd).await;
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

/// The secret does not exists yet but has been specified in the config. A Job will be triggered to get the 5 elements needed for running node:
/// 
///     - HOPRD_IDENTITY
///     - HOPRD_PASSWORD
///     - HOPRD_API_TOKEN
///     - HOPRD_ADDRESS
///     - HOPRD_PEER_ID
/// 
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_exists(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_instance = &context.config.instance;
    hoprd.create_event(context.clone(), crate::model::HoprdStatusEnum::Creating).await?;
    hoprd.update_status(context.clone(), crate::model::HoprdStatusEnum::Creating).await?;
    let secret = create_secret_resource(client.clone(), &operator_instance.namespace, &hoprd.spec.secret.as_ref().unwrap().secret_name, &hoprd.spec.network).await.unwrap();
    let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
    hoprd_jobs::execute_job_create_node(client.clone(), &hoprd,  &context.config, owner_reference).await?;
    do_status_not_registered(context, hoprd).await
}

/// The secret exists but can not be used yet as it is not registered. Before using it will trigger a Job to register the node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_registered(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api: Api<Secret> = Api::namespaced(client.clone(), &context.config.instance.namespace);
    let secret = api.get(&secret_name).await.unwrap();
    let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
    hoprd.create_event(context.clone(), crate::model::HoprdStatusEnum::RegisteringInNetwork).await?;
    hoprd.update_status(context.clone(), crate::model::HoprdStatusEnum::RegisteringInNetwork).await?;
    hoprd_jobs::execute_job_registering_node(client.clone(), &hoprd, &context.config, owner_reference).await?;
    utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_NETWORK_REGISTRY, "true").await?;
    do_status_not_funded(context, hoprd).await
}

/// The secret exists but can not be used yet as it is not funded. Before using it will trigger a Job to fund the node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_funded(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api: Api<Secret> = Api::namespaced(client.clone(), &context.config.instance.namespace);
    let secret = api.get(&secret_name).await.unwrap();
    let owner_reference: Option<Vec<OwnerReference>> = Some(vec![secret.controller_owner_ref(&()).unwrap()]);
    hoprd.create_event(context.clone(), crate::model::HoprdStatusEnum::Funding).await?;
    hoprd.update_status(context.clone(), crate::model::HoprdStatusEnum::Funding).await?;
    hoprd_jobs::execute_job_funding_node(client.clone(), &hoprd, &context.config, owner_reference).await?;
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api: Api<Secret> = Api::namespaced(client.clone(), &context.config.instance.namespace);
    utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_FUNDED, "true").await?;
    return do_status_ready(context, hoprd).await;
}

/// The secret exists but it is locked by other node. It will raise an error specifying that the secret reference needs to be updated to an other secret or just remove it to create a new one.
///
/// # Arguments
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_locked(hoprd: &Hoprd) -> Result<Secret, Error> {
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    return Err(Error::SecretStatusError(
        format!("The secret {secret_name} in namespace {} is already locked by other hoprd node. See details above.", hoprd.namespace().unwrap())
            .to_owned()
    ));
}

/// The secret exists and is ready to be used by the hoprd node. It will create the annotations and labels for locking the secret
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_ready(context: Arc<ContextData>, hoprd: &Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_namespace = &&context.config.instance.namespace;
    let hoprd_namespace = &hoprd.namespace().unwrap();
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api_secret: Api<Secret> = Api::namespaced(client.clone(), operator_namespace);
    utils::update_secret_annotations(&api_secret, &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES, hoprd_namespace).await?;
    hoprd.create_event(context.clone(), crate::model::HoprdStatusEnum::Running).await?;
    hoprd.update_status(context.clone(), crate::model::HoprdStatusEnum::Running).await?;
    utils::update_secret_label(&api_secret, &secret_name, constants::LABEL_NODE_LOCKED, &"true".to_string()).await?;
    let owner_reference: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).unwrap()]);
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
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `operator_namespace` - Operator namespace
/// - `secret_name` - Name of the secret
/// - `network` - Name of the network
async fn create_secret_resource(client: Client, operator_namespace: &str, secret_name: &str, network: &str) -> Result<Secret, Error> {
    let mut labels: BTreeMap<String, String> = utils::common_lables(&secret_name.to_owned());
    labels.insert(constants::LABEL_NODE_NETWORK.to_owned(), network.to_owned());
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
    let api: Api<Secret> = Api::namespaced(client, operator_namespace);
    Ok(api.create(&PostParams::default(), &deployment).await?)
}