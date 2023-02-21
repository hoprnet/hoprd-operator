use std::sync::Arc;

use kube::Resource;
use kube::ResourceExt;
use kube::{
    client::Client, runtime::controller::Action };
use tokio::time::Duration;

use crate::constants;
use crate::crd::Hoprd;
use crate::crd::HoprdSpec;
use crate::crd::Monitoring;
use crate::finalizer;
use crate::hoprd;
use crate::operator::ContextData;


/// Steps to perform when a creation of hoprd resource is detected
/// 
/// # Arguments
/// - `client`: A K8s client reference
/// - `name`: The name of the resource
/// - `namespace`: The namespace of the resource
/// - `hoprd_spec`: Spec about the hoprd resource
async fn do_action_create_hoprd(client: &Client, name: &String, namespace: &String, hoprd_spec: &HoprdSpec) -> Result<Action, Error> {
    println!("Creating {name} Hoprd resource");
    // Creates a deployment, but applies a finalizer first.
    // Finalizer is applied first, as the operator might be shut down and restarted
    // at any time, leaving subresources in intermediate state. This prevents leaks on
    // the `Hoprd` resource deletion.

    // Apply the finalizer first. If that fails, the `?` operator invokes automatic conversion
    // of `kube::Error` to the `Error` defined in this crate.
    finalizer::add(client.clone(), &name, &namespace).await?;
    // Invoke creation of a Kubernetes resources
    hoprd::create_deployment(client.clone(), &name, &hoprd_spec, &namespace).await?;
    hoprd::create_service(client.clone(), &name, &namespace).await?;
    if hoprd_spec.monitoring.as_ref().unwrap_or(&Monitoring {enabled: constants::MONITORING_ENABLED}).enabled {
        hoprd::create_service_monitor(client.clone(), &name, &hoprd_spec, &namespace).await?;
    }
    Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY)))
}

/// Steps to perform when a delete of hoprd resource is detected
/// 
/// # Arguments
/// - `client`: A K8s client reference
/// - `name`: The name of the resource
/// - `namespace`: The namespace of the resource
/// - `hoprd_spec`: Spec about the hoprd resource
async fn do_action_delete_hoprd(client: &Client, name: &String, namespace: &String, hoprd_spec: &HoprdSpec)  -> Result<Action, Error> {
    {
        println!("Deleting {name} resource Hoprd");
        // Deletes any subresources related to this `Hoprd` resources. If and only if all subresources
        // are deleted, the finalizer is removed and Kubernetes is free to remove the `Hoprd` resource.

        if hoprd_spec.monitoring.as_ref().unwrap_or(&Monitoring {enabled: constants::MONITORING_ENABLED}).enabled {
            hoprd::delete_service_monitor(client.clone(), &name, &namespace).await?;
        }
        hoprd::delete_service(client.clone(), &name, &namespace).await?;
        hoprd::delete_depoyment(client.clone(), &name, &namespace).await?;

        // Once all the resources are successfully removed, remove the finalizer to make it possible
        // for Kubernetes to delete the `Hoprd` resource.
        finalizer::delete(client.clone(), &name, &namespace).await?;
        Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
    }
}

/// Things to do when there is been no change for the resource
async fn do_action_no_op() -> Result<Action, Error> {
    println!("No operation for resource Hoprd");
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
    eprintln!("Reconciliation error:\n{:?}.\n{:?}", error, hoprd);
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