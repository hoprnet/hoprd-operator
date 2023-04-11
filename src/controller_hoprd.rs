
use futures::StreamExt;
use k8s_openapi::api::{apps::v1::Deployment, networking::v1::Ingress, core::v1::{Service, Secret}, batch::v1::Job};
use kube::{
    api::{Api},
    client::Client,
    runtime::{controller::{Action, Controller}, watcher::Config},
    Resource, Result, Error
};

use std::{sync::Arc, collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};
use tokio::{ time::Duration};

use crate::{ constants::{self}, hoprd::{Hoprd, HoprdSpec}, servicemonitor::ServiceMonitor, context_data::ContextData};

/// Action to be taken upon an `Hoprd` resource during reconciliation
enum HoprdAction {
    /// Create the subresources, this includes spawning `n` pods with Hoprd service
    Create,
    /// Modify Hoprd resource
    Modify,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `Hoprd` resource is in desired state and requires no actions to be taken
    NoOp,
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
        let mut hasher: DefaultHasher = DefaultHasher::new();
        let hoprd_spec: HoprdSpec = hoprd.spec.clone();
        hoprd_spec.clone().hash(&mut hasher);
        let hash: String = hasher.finish().to_string();
        let current_checksum = format!("checksum-{}",hash.to_string());
        let previous_checksum: String = hoprd.status.as_ref().map_or("0".to_owned(), |status| status.checksum.to_owned());
        // When the resource is created, does not have previous checksum and needs to be skip the modification because it's being handled already by the creation operation
        if previous_checksum.eq(&"0".to_owned()) || current_checksum.eq(&previous_checksum) {
            HoprdAction::NoOp
        } else {
            HoprdAction::Modify
        } 
    };
}

async fn reconciler(hoprd: Arc<Hoprd>, context: Arc<ContextData>) -> Result<Action> {
    // Performs action as decided by the `determine_action` function.
    return match determine_action(&hoprd) {
        HoprdAction::Create => hoprd.create(context.clone()).await,
        HoprdAction::Modify => hoprd.modify(context.clone()).await,
        HoprdAction::Delete => hoprd.delete(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        HoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))),
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
    eprintln!("[ERROR] [ClusterHoprd] Reconciliation error:\n{:?}.\n{:?}", error, hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))
}


/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<Hoprd> = Api::<Hoprd>::all(client.clone());
    let job = Api::<Job>::all(client.clone());
    let deployment = Api::<Deployment>::all(client.clone());
    let secret = Api::<Secret>::all(client.clone());
    let service = Api::<Service>::all(client.clone());
    let service_monitor = Api::<ServiceMonitor>::all(client.clone());
    let ingress = Api::<Ingress>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(job, Config::default())
        .owns(deployment, Config::default())
        .owns(secret, Config::default())
        .owns(service, Config::default())
        .owns(service_monitor, Config::default())
        .owns(ingress, Config::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") {
                        // https://github.com/kube-rs/kube/issues/712
                            eprintln!("[ERROR] [Hoprd] Reconciliation error: {:?}", reconciliation_err)
                    }
                }
            }
        })
        .await;
}
