use k8s_openapi::{api::{core::v1::Secret}};
use kube::{Api, Client, core::ObjectMeta, api::{PostParams, Patch, ListParams, PatchParams, DeleteParams}, ResourceExt};
use serde_json::{Value, json};
use std::{collections::{BTreeMap}, sync::Arc};
use std::env;
use rand::{distributions::Alphanumeric, Rng};
use async_recursion::async_recursion;
use crate::{
    model::{ Secret as HoprdSecret, SecretContent, Error}, utils, constants, hoprd_jobs, hoprd::Hoprd, hoprd::HoprdSpec, controller::ContextData
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
    /// The secret exists and it is locked but the associated node does not exist anymore
    Orphaned,
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
    if secret_labels.contains_key(constants::LABEL_NODE_ENVIRONMENT_NAME) {
        let environment_name_label: String = secret_labels.get_key_value(constants::LABEL_NODE_ENVIRONMENT_NAME).unwrap().1.parse().unwrap();
        if ! environment_name_label.eq(&hoprd_spec.environment_name.to_owned()) {
            return Err(Error::SecretStatusError(
                format!("[ERROR] The secret specified {secret_name} belongs to '{environment_name_label}' environment which is different from the specified '{}' environment", hoprd_spec.environment_name)
                    .to_owned()
            ));
        }
    } else {
        return Err(Error::SecretStatusError(
            format!("[ERROR] The secret specified {secret_name} does not contain label {} which is mandatory", constants::LABEL_NODE_ENVIRONMENT_NAME)
                .to_owned()
        ));
    }
    if secret_labels.contains_key(constants::LABEL_NODE_ENVIRONMENT_TYPE) {
        let environment_type_label: String = secret_labels.get_key_value(constants::LABEL_NODE_ENVIRONMENT_TYPE).unwrap().1.parse().unwrap();
        if ! environment_type_label.eq(&hoprd_spec.environment_type.to_owned()) {
            return Err(Error::SecretStatusError(
                format!("[ERROR] The secret specified {secret_name} belongs to '{environment_type_label}' environment type which is different from the specified '{}' environment type", hoprd_spec.environment_type)
                    .to_owned()
            ));
        }
    } else {
            return Err(Error::SecretStatusError(
                format!("[ERROR] The secret specified {secret_name} does not contain label {} which is mandatory", constants::LABEL_NODE_ENVIRONMENT_TYPE)
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
async fn get_first_secret_ready(client: Client, hoprd_spec: &HoprdSpec, operator_namespace: &str) -> Result<Option<Secret>, Error> {
    let api: Api<Secret> = Api::namespaced(client, operator_namespace);
    let label_selector: String = format!("{}={},{}={},{}={}",
    constants::LABEL_NODE_ENVIRONMENT_NAME, &hoprd_spec.environment_name,
    constants::LABEL_NODE_ENVIRONMENT_TYPE, &hoprd_spec.environment_type,
    constants::LABEL_NODE_LOCKED, "false");
    let lp = ListParams::default().labels(&label_selector);
    let secrets = api.list(&lp).await?;
    match secrets.items.first() {
        Some(first_secret) => { return Ok(Some(first_secret.clone()));}
        None => { Ok(None)}
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
        println!("[INFO] The secret has not been specified in the hoprd_spec");
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
                    println!("[INFO] The secret exists but is not funded");
                    return Ok(SecretStatus::NotFunded)
                }
            } else {
                println!("[INFO] The secret exists but is not funded");
                return Ok(SecretStatus::NotFunded)
            }
            if secret_labels.contains_key(constants::LABEL_NODE_LOCKED) {
                let node_locked_annotation = secret_labels.get_key_value(constants::LABEL_NODE_LOCKED).unwrap().1.parse().unwrap();
                if node_locked_annotation {
                    let locked_by_annotation: &String = secret_annotations.get_key_value(constants::ANNOTATION_HOPRD_LOCKED_BY).unwrap().1;
                    let api_hoprd: Api<Hoprd> = Api::namespaced(client.clone(), &operator_namespace);
                    match api_hoprd.get(&locked_by_annotation).await {
                        Ok(_locked) => {
                            println!("[INFO] Secret '{:?}' is already locked by hoprd node '{:?}'", &secret_name, locked_by_annotation);
                            return Ok(SecretStatus::Locked);
                        }
                        Err(_orphan) => {
                            println!("[WARN] Secret '{:?}' is orphan as hoprd node '{:?}' does not exist anymore", &secret_name, locked_by_annotation);
                            return Ok(SecretStatus::Orphaned);
                        }};
                }
            }
            println!("[INFO] The secret is ready to be used");
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
        SecretStatus::Orphaned => do_status_orphaned(context.clone(), hoprd).await,
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
    if hoprd.spec.secret.as_ref().is_some() {
        let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
        if let Some(_secret) = api.get_opt(&secret_name).await? {
            utils::update_secret_label(&api.clone(), &secret_name, constants::LABEL_NODE_LOCKED, &"false".to_string()).await?;
            utils::delete_secret_annotations(&api.clone(), &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES).await?;
        }
        let api_secrets: Api<Secret> = Api::namespaced(client.clone(), &hoprd.namespace().unwrap());
        if let Some(_secret) = api_secrets.get_opt(&secret_name).await? {
            api_secrets.delete(&secret_name, &DeleteParams::default()).await?;
        }
        Ok(println!("[INFO] The secret '{secret_name}' has been unlocked"))
    } else {
        Ok(println!("[WARN] The node '{}' does not have a secret associated", hoprd.name_any()))
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
    let hoprd_name = &hoprd.name_any();
    let hoprd_namespace = &hoprd.namespace().unwrap();
    match get_first_secret_ready(client.clone(), &hoprd.spec, operator_namespace).await {
        Ok(secret) => { 
            match secret {
                Some(secret) => {
                    let secret_name = secret.metadata.name.unwrap();
                    let patch = Patch::Merge(json!({
                        "spec": {
                            "secret": 
                                { "secretName" : secret_name }
                        }
                    }));
                    hoprd.spec.secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                    let hoprd_api: Api<Hoprd> = Api::namespaced(client.clone(), hoprd_namespace);                    
                    hoprd_api.patch(&hoprd_name, &PatchParams::default(), &patch).await?;
                    return create_secret(context, hoprd).await;
                }
                None => {
                    let random_string: String = rand::thread_rng().sample_iter(&Alphanumeric).take(5).map(char::from).collect();
                    let mut secret_name = String::from("hoprd-node-");
                    secret_name.push_str(&hoprd.spec.environment_name.replace("_", "-"));
                    secret_name.push_str(&"-");
                    secret_name.push_str(&random_string.to_lowercase());
                    let config_added: Value = json!({
                        "spec": {
                            "secret": 
                                { "secretName" : secret_name }
                        }
                    });
                    hoprd.spec.secret = Some(HoprdSecret { secret_name: secret_name.to_owned(), ..HoprdSecret::default() });
                    let patch: Patch<&Value> = Patch::Merge(&config_added);
                    let hoprd_api: Api<Hoprd> = Api::namespaced(client.clone(), hoprd_namespace);
                    hoprd_api.patch(&hoprd_name, &PatchParams::default(), &patch).await?;
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
async fn do_status_not_exists(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_instance = &context.config.instance;
    let hoprd_name = &hoprd.name_any();
    utils::update_status(context.clone(), hoprd, crate::model::HoprdStatusEnum::Creating).await?;
    match hoprd_jobs::execute_job_create_node(client.clone(), &hoprd_name, &hoprd.spec, operator_instance).await {
        Ok(_job) => {
            let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
            let operator_environment= env::var(constants::OPERATOR_ENVIRONMENT).unwrap();
            let secret_name_path = if operator_environment.eq("production") {
                format!("/app/node_secrets/{secret_name}/{secret_name}.json")
            } else {
                let mut path = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
                path.push_str("/sample_secret_content.json");
                path.as_str().to_owned()
            };
            let secret_text_content = std::fs::read_to_string(&secret_name_path).unwrap();
            match serde_json::from_str::<SecretContent>(&secret_text_content) {
                Ok(mut secret_content) => {
                    if operator_environment.eq("development") {
                        secret_content.secret_name = secret_name.to_owned()
                    }
                    match create_secret_resource(client.clone(), &operator_instance.namespace, &secret_content).await {
                        Ok(_secret) => {
                            let api: Api<Secret> = Api::namespaced(client.clone(), &operator_instance.namespace);
                            utils::update_secret_label(&api, &secret_name, constants::LABEL_NODE_PEER_ID, &secret_content.peer_id).await?;
                            utils::update_secret_label(&api, &secret_name, constants::LABEL_NODE_ADDRESS, &secret_content.address).await?;
                            utils::update_secret_label(&api, &secret_name, constants::LABEL_NODE_ENVIRONMENT_NAME, &hoprd.spec.environment_name).await?;
                            utils::update_secret_label(&api, &secret_name, constants::LABEL_NODE_ENVIRONMENT_TYPE, &hoprd.spec.environment_type).await?;
                            return do_status_not_registered(context, hoprd).await;
                        },
                        Err(_err) => {
                            println!("[ERROR]: {:?}", _err);
                            return Err(Error::JobExecutionError(
                                format!("Could not create the secret with this content: {secret_text_content}.")
                                    .to_owned()
                            ));
                        }
                    }
                },
                Err(_err) => {
                    return Err(Error::JobExecutionError(
                        format!("Could not parse the content of the Job output: {secret_text_content}.")
                            .to_owned()
                    ));
                }
            }
        },
        Err(error) => {
            println!("[ERROR]: {:?}", error);
            return Err(Error::JobExecutionError(
                format!("The creation node job failed and the node {hoprd_name} cannot be fully configured.")
                    .to_owned()
            ));
        }
    }
}

/// The secret exists but can not be used yet as it is not registered. Before using it will trigger a Job to register the node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_registered(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_instance = &context.config.instance;
    let hoprd_name = &hoprd.name_any();
    utils::update_status(context.clone(), hoprd, crate::model::HoprdStatusEnum::RegisteringInNetwork).await?;
    match hoprd_jobs::execute_job_registering_node(client.clone(), &hoprd_name, &hoprd.spec, &operator_instance).await {
        Ok(_job) => {
            let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
            let api: Api<Secret> = Api::namespaced(client.clone(), &operator_instance.namespace);
            utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_NETWORK_REGISTRY, "true").await?;
            do_status_not_funded(context, hoprd).await
        },
        Err(_err) => {
            println!("[ERROR]: {:?}", _err);
            return Err(Error::JobExecutionError(
                format!("The registration node job failed and the node {hoprd_name} cannot be fully configured.")
                    .to_owned()
            ));
        }
    }
}

/// The secret exists but can not be used yet as it is not funded. Before using it will trigger a Job to fund the node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_not_funded(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_instance = &context.config.instance;
    let hoprd_name = &hoprd.name_any();
    utils::update_status(context.clone(), hoprd, crate::model::HoprdStatusEnum::Funding).await?;
    match hoprd_jobs::execute_job_funding_node(client.clone(), &hoprd_name,  &hoprd.spec, &operator_instance).await {
        Ok(_job) => {
            let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
            let api: Api<Secret> = Api::namespaced(client.clone(), &operator_instance.namespace);
            utils::update_secret_annotations(&api, &secret_name,constants::ANNOTATION_HOPRD_FUNDED, "true").await?;
            return do_status_ready(context, hoprd).await;
        },
        Err(_err) => {
            return Err(Error::JobExecutionError(
                format!("The funding job failed and the node {hoprd_name} cannot be fully configured.")
                    .to_owned()
            ));
        }
    }
}

/// The secret exists but it is locked by other node. It will raise an error specifying that the secret reference needs to be updated to an other secret or just remove it to create a new one.
///
/// # Arguments
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_locked(hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    return Err(Error::SecretStatusError(
        format!("The secret {secret_name} in namespace {} is already locked by other hoprd node. See details above.", hoprd.namespace().unwrap())
            .to_owned()
    ));
}

/// The secret exists and is orphaned as the associated node does not exist anymore. It will update the lockBy annotation of the secret with this new node
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_orphaned(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_namespace = &&context.config.instance.namespace;
    let hoprd_name = &hoprd.name_any();
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api: Api<Secret> = Api::namespaced(client.clone(), operator_namespace);
    utils::update_secret_annotations(&api, &secret_name, &constants::ANNOTATION_HOPRD_LOCKED_BY, &hoprd_name).await?;
    Ok(api.get(&secret_name).await?)
}

/// The secret exists and is ready to be used by the hoprd node. It will create the annotations and labels for locking the secret
///
/// # Arguments
/// - `context` - Operator context
/// - `hoprd` - Details about the hoprd configuration node
async fn do_status_ready(context: Arc<ContextData>, hoprd: &mut Hoprd) -> Result<Secret, Error> {
    let client: Client = context.client.clone();
    let operator_namespace = &&context.config.instance.namespace;
    let hoprd_name = &hoprd.name_any();
    let hoprd_namespace = &hoprd.namespace().unwrap();
    let secret_name: String = hoprd.spec.secret.as_ref().unwrap().secret_name.to_owned();
    let api_secret: Api<Secret> = Api::namespaced(client.clone(), operator_namespace);
    utils::update_secret_annotations(&api_secret, &secret_name, constants::ANNOTATION_HOPRD_LOCKED_BY, hoprd_name).await?;
    utils::update_secret_annotations(&api_secret, &secret_name, constants::ANNOTATION_REPLICATOR_NAMESPACES, hoprd_namespace).await?;
    utils::update_status(context.clone(), hoprd, crate::model::HoprdStatusEnum::Running).await?;
    Ok(utils::update_secret_label(&api_secret, &secret_name, constants::LABEL_NODE_LOCKED, &"true".to_string()).await?)
}

/// Creates the underlying Kubernetes Secret resource
///
/// # Arguments
/// - `client` - A Kubernetes client.
/// - `operator_namespace` - Operator namespace
/// - `contents` - Details about the secrets
async fn create_secret_resource(client: Client, operator_namespace: &str, contents: &SecretContent) -> Result<Secret, Error> {
    let labels: BTreeMap<String, String> = utils::common_lables(&contents.secret_name.to_owned());

    let deployment: Secret = Secret {
        metadata: ObjectMeta {
            name: Some(contents.secret_name.to_owned()),
            namespace: Some(operator_namespace.to_owned()),
            labels: Some(labels.clone()),
            ..ObjectMeta::default()
        },
        data: Some(contents.get_encoded_data()),
        type_: Some("Opaque".to_owned()),
        ..Secret::default()
    };

    // Create the secret defined above
    let api: Api<Secret> = Api::namespaced(client, operator_namespace);
    Ok(api.create(&PostParams::default(), &deployment).await?)
}