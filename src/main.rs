use std::sync::Arc;

use futures::stream::StreamExt;
use kube::Resource;
use kube::ResourceExt;
use kube::{
    api::ListParams, client::Client, runtime::controller::Action, runtime::Controller, Api,
};
use tokio::time::Duration;

use crate::crd::Hoprd;

pub mod crd;
mod hoprd;
mod servicemonitor;
mod utils;
mod finalizer;
mod constants;

#[tokio::main]
async fn main() {
    // First, a Kubernetes client must be obtained using the `kube` crate
    // The client will later be moved to the custom controller
    let kubernetes_client: Client = Client::try_default()
        .await
        .expect("Expected a valid KUBECONFIG environment variable.");

    // Preparation of resources used by the `kube_runtime::Controller`
    let crd_api: Api<Hoprd> = Api::all(kubernetes_client.clone());
    let context: Arc<ContextData> = Arc::new(ContextData::new(kubernetes_client.clone()));

    // The controller comes from the `kube_runtime` crate and manages the reconciliation process.
    // It requires the following information:
    // - `kube::Api<T>` this controller "owns". In this case, `T = Hoprd`, as this controller owns the `Hoprd` resource,
    // - `kube::api::ListParams` to select the `Hoprd` resources with. Can be used for Hoprd filtering `Hoprd` resources before reconciliation,
    // - `reconcile` function with reconciliation logic to be called each time a resource of `Hoprd` kind is created/updated/deleted,
    // - `on_error` function to call whenever reconciliation fails.
    Controller::new(crd_api.clone(), ListParams::default())
        .run(reconcile, on_error, context)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(hoprd_resource) => {
                    println!("Reconciliation successful. Resource: {:?}", hoprd_resource);
                }
                Err(reconciliation_err) => {
                    eprintln!("Reconciliation error: {:?}", reconciliation_err)
                }
            }
        })
        .await;
}

/// Context injected with each `reconcile` and `on_error` method invocation.
struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    client: Client,
}

impl ContextData {
    /// Constructs a new instance of ContextData.
    ///
    /// # Arguments:
    /// - `client`: A Kubernetes client to make Kubernetes REST API requests with. Resources
    /// will be created and deleted with this client.
    pub fn new(client: Client) -> Self {
        ContextData { client }
    }
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

async fn reconcile(hoprd: Arc<Hoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
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

    // Performs action as decided by the `determine_action` function.
    return match determine_action(&hoprd) {
        HoprdAction::Create => {
            println!("Creating resource Hoprd");
            // Creates a deployment with `n` Hoprd service pods, but applies a finalizer first.
            // Finalizer is applied first, as the operator might be shut down and restarted
            // at any time, leaving subresources in intermediate state. This prevents leaks on
            // the `Hoprd` resource deletion.

            // Apply the finalizer first. If that fails, the `?` operator invokes automatic conversion
            // of `kube::Error` to the `Error` defined in this crate.
            finalizer::add(client.clone(), &name, &namespace).await?;
            // Invoke creation of a Kubernetes built-in resource named deployment with `n` hoprd service pods.
            hoprd::create_deployment(client.clone(), &name, &hoprd.spec, &namespace).await?;
            hoprd::create_service(client.clone(), &name, &namespace).await?;
            if hoprd.spec.enable_monitoring.unwrap_or(true) {
                hoprd::create_service_monitor(client.clone(), &name, &hoprd.spec, &namespace).await?;
            }
            Ok(Action::requeue(Duration::from_secs(10)))
        }
        HoprdAction::Delete => {
            println!("Deleting resource Hoprd");
            // Deletes any subresources related to this `Hoprd` resources. If and only if all subresources
            // are deleted, the finalizer is removed and Kubernetes is free to remove the `Hoprd` resource.

            //First, delete the deployment. If there is any error deleting the deployment, it is
            // automatically converted into `Error` defined in this crate and the reconciliation is ended
            // with that error.
            // Note: A more advanced implementation would check for the Deployment's existence.
            if hoprd.spec.enable_monitoring.unwrap_or(true) {
                hoprd::delete_service_monitor(client.clone(), &name, &namespace).await?;
            }
            hoprd::delete_service(client.clone(), &name, &namespace).await?;
            hoprd::delete_depoyment(client.clone(), &name, &namespace).await?;

            // Once the deployment is successfully removed, remove the finalizer to make it possible
            // for Kubernetes to delete the `Hoprd` resource.
            finalizer::delete(client.clone(), &name, &namespace).await?;
            Ok(Action::await_change()) // Makes no sense to delete after a successful delete, as the resource is gone
        }
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        HoprdAction::NoOp => {
            println!("No operation for resource Hoprd");
            Ok(Action::requeue(Duration::from_secs(10)))
        },
    };
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

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
fn on_error(hoprd: Arc<Hoprd>, error: &Error, _context: Arc<ContextData>) -> Action {
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
