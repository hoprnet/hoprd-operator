use futures::StreamExt;
use kube::{
    api::Api,
    client::Client,
    runtime::{
        controller::{Action, Controller}, watcher::Config
    },
    Resource, Result,
};
use std::sync::Arc;
use tokio::time::Duration;

use crate::{
    cluster::cluster_hoprd::{ClusterHoprd, ClusterHoprdPhaseEnum},
    constants::{self},
    context_data::ContextData,
    hoprd::hoprd_resource::Hoprd,
    model::Error,
    utils,
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
    return if cluster_hoprd.meta().deletion_timestamp.is_some() && cluster_hoprd.status.as_ref().is_some_and(|status| status.phase.ne(&ClusterHoprdPhaseEnum::Deleting)) {
        ClusterHoprdAction::Delete
    } else if cluster_hoprd.meta().finalizers.as_ref().map_or(true, |finalizers| finalizers.is_empty()) {
        ClusterHoprdAction::Create
    } else if cluster_hoprd.status.is_none() {
        // A finalizer without status means the resource was not fully initialized: it is created again unless it is going away
        if cluster_hoprd.meta().deletion_timestamp.is_some() {
            ClusterHoprdAction::NoOp
        } else {
            ClusterHoprdAction::Create
        }
    } else if cluster_hoprd.status.as_ref().is_some_and(|status| matches!(status.phase, ClusterHoprdPhaseEnum::NotScaled | ClusterHoprdPhaseEnum::Scaling)) {
        ClusterHoprdAction::Rescale
    } else if cluster_hoprd.status.as_ref().is_some_and(|status| status.phase == ClusterHoprdPhaseEnum::Deleting) {
        ClusterHoprdAction::NoOp
    } else {
        let current_generation = cluster_hoprd.meta().generation.unwrap_or(0);
        let observed_generation = cluster_hoprd.status.as_ref().map_or(0, |status| status.observed_generation);
        if observed_generation < current_generation {
            ClusterHoprdAction::Modify
        } else {
            ClusterHoprdAction::NoOp
        }
    }
}

async fn reconciler(cluster_hoprd: Arc<ClusterHoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
    utils::catch_panics("ClusterHoprd", reconcile(cluster_hoprd, context)).await
}

async fn reconcile(cluster_hoprd: Arc<ClusterHoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
    // Performs action as decided by the `determine_action` function.
    match determine_action(&cluster_hoprd) {
        ClusterHoprdAction::Create => cluster_hoprd.create(context.clone()).await,
        ClusterHoprdAction::Modify => cluster_hoprd.modify(context.clone()).await,
        ClusterHoprdAction::Delete => cluster_hoprd.delete(context.clone()).await,
        ClusterHoprdAction::Rescale => cluster_hoprd.rescale(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        ClusterHoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY))),
    }
}

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<ClusterHoprd> = Api::<ClusterHoprd>::all(client.clone());
    let hoprd = Api::<Hoprd>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(hoprd, Config::default())
        .shutdown_on_signal()
        .run(reconciler, utils::on_error, context_data)
        .for_each(|reconciliation_result| async move { utils::log_reconciliation_result("ClusterHoprd", reconciliation_result) })
        .await;
}
