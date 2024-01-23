use futures::StreamExt;
use kube::{
    api::Api,
    client::Client,
    runtime::{
        controller::{Action, Controller},
        watcher::Config,
    },
    Resource, Result,
};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::error;

use crate::{
    cluster::{ClusterHoprd, ClusterHoprdPhaseEnum},
    constants::{self},
    context_data::ContextData,
    hoprd::Hoprd,
    model::Error,
};

/// Action to be taken upon an `ClusterHoprd` resource during reconciliation
enum ClusterHoprdAction {
    /// Create the subresources, this includes spawning multiple `Hoprd` resources
    Create,
    /// Modify ClusterHoprd resource
    Modify,
    /// Sync ClusterHoprd resources
    Rescale,
    /// Delete all subresources created in the `Create` phase
    Delete,
    /// This `ClusterHoprd` resource is in desired state and requires no actions to be taken
    NoOp,
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `ClusterHoprd` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `ClusterHoprdAction` enum.
///
/// # Arguments
/// - `cluster_hoprd`: A reference to `ClusterHoprd` being reconciled to decide next action upon.
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
    } else if cluster_hoprd.status.as_ref().unwrap().phase == ClusterHoprdPhaseEnum::NotScaled || cluster_hoprd.status.as_ref().unwrap().phase == ClusterHoprdPhaseEnum::Scaling {
        ClusterHoprdAction::Rescale
    } else if cluster_hoprd.status.as_ref().unwrap().phase == ClusterHoprdPhaseEnum::Deleting {
        ClusterHoprdAction::NoOp
    } else {
        let current_checksum = cluster_hoprd.get_checksum();
        let previous_checksum: String = cluster_hoprd.status.as_ref().map_or("0".to_owned(), |status| status.checksum.to_owned());
        // When the resource is created, does not have previous checksum and needs to be skip the modification because it's being handled already by the creation operation
        if previous_checksum.eq(&"0".to_owned()) || current_checksum.eq(&previous_checksum) {
            ClusterHoprdAction::NoOp
        } else {
            ClusterHoprdAction::Modify
        }
    };
}

async fn reconciler(
    cluster_hoprd: Arc<ClusterHoprd>,
    context: Arc<ContextData>,
) -> Result<Action, Error> {
    // Performs action as decided by the `determine_action` function.
    match determine_action(&cluster_hoprd) {
        ClusterHoprdAction::Create => cluster_hoprd.create(context.clone()).await,
        ClusterHoprdAction::Modify => cluster_hoprd.modify(context.clone()).await,
        ClusterHoprdAction::Delete => cluster_hoprd.delete(context.clone()).await,
        ClusterHoprdAction::Rescale => cluster_hoprd.rescale(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        ClusterHoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(
            constants::RECONCILE_FREQUENCY,
        ))),
    }
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `cluster_hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
pub fn on_error(
    cluster_hoprd: Arc<ClusterHoprd>,
    error: &Error,
    _context: Arc<ContextData>,
) -> Action {
    error!("[ClusterHoprd] Reconciliation error:\n{:?}.\n{:?}",error, cluster_hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))
}

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<ClusterHoprd> = Api::<ClusterHoprd>::all(client.clone());
    let hoprd = Api::<Hoprd>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(hoprd, Config::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_cluster_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") {
                        // https://github.com/kube-rs/kube/issues/712
                        error!("[ClusterHoprd] Reconciliation error: {:?}",reconciliation_err)
                    }
                }
            }
        })
        .await;
}
