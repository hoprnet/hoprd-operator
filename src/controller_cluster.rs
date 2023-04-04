
use futures::StreamExt;
use k8s_openapi::api::{apps::v1::Deployment, networking::v1::Ingress, core::v1::{Service, Secret}, batch::v1::Job};
use kube::{
    api::{Api, ListParams},
    client::Client,
    runtime::{
        controller::{Action, Controller}
    },
    Resource, Result, Error
};

use std::{sync::Arc, collections::hash_map::DefaultHasher, hash::{Hash, Hasher}};
use tokio::{ time::Duration};

use crate::{ constants::{self}, cluster::{ClusterHoprd, ClusterHoprdSpec}, servicemonitor::ServiceMonitor, context_data::ContextData};

/// Action to be taken upon an `Hoprd` resource during reconciliation
enum ClusterHoprdAction {
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
/// The finite set of possible actions is represented by the `ClusterHoprdAction` enum.
///
/// # Arguments
/// - `hoprd`: A reference to `Hoprd` being reconciled to decide next action upon.
fn determine_action(cluster_hoprd: &ClusterHoprd) -> ClusterHoprdAction {
    return if cluster_hoprd.meta().deletion_timestamp.is_some() {
        ClusterHoprdAction::Delete
    } else if cluster_hoprd
        .meta()
        .finalizers
        .as_ref()
        .map_or(true, |finalizers| finalizers.is_empty())
    {
        ClusterHoprdAction::Create
    } else {
        let mut hasher: DefaultHasher = DefaultHasher::new();
        let cluster_hoprd_spec: ClusterHoprdSpec = cluster_hoprd.spec.clone();
        cluster_hoprd_spec.clone().hash(&mut hasher);
        let hash: String = hasher.finish().to_string();
        let current_checksum = format!("checksum-{}",hash.to_string());
        let previous_checksum: String = cluster_hoprd.status.as_ref().map_or("0".to_owned(), |status| status.checksum.to_owned());
        // When the resource is created, does not have previous checksum and needs to be skip the modification because it's being handled already by the creation operation
        if previous_checksum.eq(&"0".to_owned()) || current_checksum.eq(&previous_checksum) {
            ClusterHoprdAction::NoOp
        } else {
            ClusterHoprdAction::Modify
        }
    };
}

async fn reconciler(hoprd: Arc<ClusterHoprd>, context: Arc<ContextData>) -> Result<Action> {
    // Performs action as decided by the `determine_action` function.
    return match determine_action(&hoprd) {
        ClusterHoprdAction::Create => hoprd.create(context.clone()).await,
        ClusterHoprdAction::Modify => hoprd.modify(context.clone()).await,
        ClusterHoprdAction::Delete => hoprd.delete(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        ClusterHoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))),
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
pub fn on_error(hoprd: Arc<ClusterHoprd>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!("[ERROR] [ClusterHoprd] Reconciliation error:\n{:?}.\n{:?}", error, hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))
}


/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<ClusterHoprd> = Api::<ClusterHoprd>::all(client.clone());
    let job = Api::<Job>::all(client.clone());
    let deployment = Api::<Deployment>::all(client.clone());
    let secret = Api::<Secret>::all(client.clone());
    let service = Api::<Service>::all(client.clone());
    let service_monitor = Api::<ServiceMonitor>::all(client.clone());
    let ingress = Api::<Ingress>::all(client.clone());

    Controller::new(owned_api, ListParams::default())
        .owns(job, ListParams::default())
        .owns(deployment, ListParams::default())
        .owns(secret, ListParams::default())
        .owns(service, ListParams::default())
        .owns(service_monitor, ListParams::default())
        .owns(ingress, ListParams::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") {
                        // https://github.com/kube-rs/kube/issues/712
                            eprintln!("[ERROR] [ClusterHoprd] Reconciliation error: {:?}", reconciliation_err)
                    }
                }
            }
        })
        .await;
}
