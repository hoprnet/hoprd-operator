use std::env;
use std::sync::Arc;

use kube::Resource;
use kube::ResourceExt;
use kube::{
    client::Client, runtime::controller::Action };
use tokio::time::Duration;

use crate::constants;
use crate::model::{Hoprd, HoprdSpec, EnablingFlag, OperatorConfig};
use crate::hoprd_hoprd;
use crate::hoprd_ingress;
use crate::hoprd_secret;
use crate::hoprd_service_monitor;
use crate::{hoprd_deployment, operator::ContextData, hoprd_service};
use serde_yaml;

async fn get_config () -> OperatorConfig {
    let operator_environment= env::var(constants::OPERATOR_ENVIRONMENT).unwrap();
    let config_path = if operator_environment.eq("production") {
        let path = "/app/config/config.yaml".to_owned();
        path
    } else {
        let mut path = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
        path.push_str("/sample_config.yaml");
        path
    };
    let config_file = std::fs::File::open(&config_path).expect("Could not open config file.");
    let config: OperatorConfig = serde_yaml::from_reader(config_file).expect("Could not read contents of config file.");
    return config;
}


/// Steps to perform when a creation of hoprd resource is detected
/// 
/// # Arguments
/// - `client`: A K8s client reference
/// - `hoprd_name`: The name of the resource
/// - `namespace`: The namespace of the resource
/// - `hoprd_spec`: Spec about the hoprd resource
async fn do_action_create_hoprd(client: &Client, hoprd_name: &String, hoprd_namespace: &String, hoprd_spec: &HoprdSpec) -> Result<Action, Error> {
    println!("[INFO] Starting to create hoprd node {hoprd_name} in namespace {hoprd_namespace}");
    let config: OperatorConfig = get_config().await;
    // Creates a deployment, but applies a finalizer first.
    // Finalizer is applied first, as the operator might be shut down and restarted
    // at any time, leaving subresources in intermediate state. This prevents leaks on
    // the `Hoprd` resource deletion.
    hoprd_hoprd::add_finalizer(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    // Invoke creation of a Kubernetes resources
    let mut spec: HoprdSpec = hoprd_spec.clone();
    hoprd_secret::create_secret(client.clone(), &hoprd_name, &hoprd_namespace, &mut spec, &config.instance).await?;
    hoprd_deployment::create_deployment(client.clone(), &hoprd_name, &hoprd_namespace, &spec).await?;
    hoprd_service::create_service(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    if hoprd_spec.ingress.is_some() && hoprd_spec.ingress.as_ref().unwrap().enabled {
        hoprd_ingress::create_ingress(client.clone(), &hoprd_name, &hoprd_namespace,&config.ingress).await?;
    }
    if spec.monitoring.as_ref().unwrap_or(&EnablingFlag {enabled: constants::MONITORING_ENABLED}).enabled {
        hoprd_service_monitor::create_service_monitor(client.clone(), &hoprd_name, &hoprd_namespace, &spec).await?;
    }
    println!("[INFO] Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully created");
    Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
}

/// Steps to perform when a delete of hoprd resource is detected
/// 
/// # Arguments
/// - `client`: A K8s client reference
/// - `hoprd_name`: The name of the resource
/// - `namespace`: The namespace of the resource
/// - `hoprd_spec`: Spec about the hoprd resource
async fn do_action_delete_hoprd(client: &Client, hoprd_name: &String, hoprd_namespace: &String, hoprd_spec: &HoprdSpec)  -> Result<Action, Error> {
    println!("[INFO] Starting to delete hoprd node {hoprd_name} from namespace {hoprd_namespace}");
    // Deletes any subresources related to this `Hoprd` resources. If and only if all subresources
    // are deleted, the finalizer is removed and Kubernetes is free to remove the `Hoprd` resource.
    let config: OperatorConfig = get_config().await;
    if hoprd_spec.monitoring.as_ref().unwrap_or(&EnablingFlag {enabled: constants::MONITORING_ENABLED}).enabled {
        hoprd_service_monitor::delete_service_monitor(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    }
    hoprd_ingress::delete_ingress(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    hoprd_service::delete_service(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    hoprd_deployment::delete_depoyment(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    let mut spec: HoprdSpec = hoprd_spec.clone();
    hoprd_secret::unlock_secret(client.clone(), hoprd_name, &hoprd_namespace,&mut spec, &config.instance.namespace).await?;
    // Once all the resources are successfully removed, remove the finalizer to make it possible
    // for Kubernetes to delete the `Hoprd` resource.
    hoprd_hoprd::delete_finalizer(client.clone(), &hoprd_name, &hoprd_namespace).await?;
    println!("[INFO] Hoprd node {hoprd_name} in namespace {hoprd_namespace} has been successfully deleted");
    Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
}

/// Things to do when there is been no change for the resource
async fn do_action_no_op() -> Result<Action, Error> {
    Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `Hoprd` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `HoprdAction` enum.
///
/// # Arguments
/// - `hoprd`: A reference to `Hoprd` being reconciled to decide next action upon.
fn determine_action(hoprd: &Hoprd) -> HoprdAction {
    return if hoprd.meta().deletion_timestamp.is_some() {
        HoprdAction::Delete
    } else if hoprd
        .meta()
        .finalizers
        .as_ref()
        .map_or(true, |finalizers| finalizers.is_empty())
    {
        HoprdAction::Create
    } else {
        HoprdAction::NoOp
    };
}

/// Actions to be taken when a reconciliation is requested.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `hoprd`: The Hoprd resource involved in the reconcilation.
/// - `context`: Context Data "injected" automatically by kube-rs to get connectivity to K8s client.
pub async fn reconcile(hoprd: Arc<Hoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
    let client: Client = context.client.clone(); // The `Client` is shared -> a clone from the reference is obtained

    // The resource of `Hoprd` kind is required to have a namespace set. However, it is not guaranteed
    // the resource will have a `namespace` set. Therefore, the `namespace` field on object's metadata
    // is optional and Rust forces the programmer to check for it's existence first.
    let namespace: String = match hoprd.namespace() {
        None => {
            // If there is no namespace to deploy to defined, reconciliation ends with an error immediately.
            return Err(Error::UserInputError(
                "Expected Hoprd resource to be namespaced. Can't deploy to an unknown namespace."
                    .to_owned(),
            ));
        }
        // If namespace is known, proceed. In a more advanced version of the operator, perhaps
        // the namespace could be checked for existence first.
        Some(namespace) => namespace,
    };
    let name = hoprd.name_any(); // Name of the Hoprd resource is used to name the subresources as well.
    let spec: &HoprdSpec = &hoprd.spec;

    // Performs action as decided by the `determine_action` function.
    return match determine_action(&hoprd) {
        HoprdAction::Create => do_action_create_hoprd(&client.clone(), &name, &namespace, &spec).await,
        HoprdAction::Delete => do_action_delete_hoprd(&client.clone(), &name, &namespace, &spec).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        HoprdAction::NoOp => do_action_no_op().await,
    };
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
pub fn on_error(hoprd: Arc<Hoprd>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!("[ERROR] Reconciliation error:\n{:?}.\n{:?}", error, hoprd);
    Action::requeue(Duration::from_secs(5))
}

/// All errors possible to occur during reconciliation
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    },
    /// Error in user input or Hoprd resource definition, typically missing fields.
    #[error("Invalid Hoprd CRD: {0}")]
    UserInputError(String),

    /// The secret is in an Unknown status
    #[error("Invalid Hoprd Secret status: {0}")]
    SecretStatusError(String),

    /// The Job execution did not complete successfully
    #[error("Job Execution failed: {0}")]
    JobExecutionError(String),
}

/// Action to be taken upon an `Hoprd` resource during reconciliation
enum HoprdAction {
    /// Create the subresources, this includes spawning `n` pods with Hoprd service
    Create,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `Hoprd` resource is in desired state and requires no actions to be taken
    NoOp,
}