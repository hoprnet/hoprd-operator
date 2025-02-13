use futures::StreamExt;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
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
    constants::{self},
    context_data::ContextData,
    identity_hoprd::identity_hoprd_resource::IdentityHoprd,
    model::Error,
};

/// Action to be taken upon an `IdentityHoprd` resource during reconciliation
enum IdentityHoprdAction {
    /// Validate the data on-chain
    Create,
    /// Modify the IdentityHoprd resource and validate data on-chain
    Modify,
    /// Delete the IdentityHoprd resource
    Delete,
    /// This `IdentityHoprd` resource is in desired state and requires no actions to be taken
    NoOp,
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `IdentityHoprd` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `IdentityHoprdAction` enum.
///
/// # Arguments
/// - `identity_hoprd`: A reference to `IdentityHoprd` being reconciled to decide next action upon.
fn determine_action(identity_hoprd: &IdentityHoprd) -> IdentityHoprdAction {
    return if identity_hoprd.meta().deletion_timestamp.is_some() {
        IdentityHoprdAction::Delete
    } else if identity_hoprd.meta().finalizers.as_ref().map_or(true, |finalizers| finalizers.is_empty()) {
        IdentityHoprdAction::Create
    } else {
        let current_checksum = identity_hoprd.get_checksum();
        let previous_checksum: String = identity_hoprd.status.as_ref().map_or("0".to_owned(), |status| status.checksum.to_owned());
        // When the resource is created, does not have previous checksum and needs to be skip the modification because it's being handled already by the creation operation
        if previous_checksum.eq(&"0".to_owned()) || current_checksum.eq(&previous_checksum) {
            IdentityHoprdAction::NoOp
        } else {
            IdentityHoprdAction::Modify
        }
    };
}

async fn reconciler(identity_hoprd: Arc<IdentityHoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
    let mut identity_hoprd_cloned = identity_hoprd.clone();
    let identity_hoprd_mutable: &mut IdentityHoprd = Arc::<IdentityHoprd>::make_mut(&mut identity_hoprd_cloned);
    // Performs action as decided by the `determine_action` function.
    match determine_action(identity_hoprd_mutable) {
        IdentityHoprdAction::Create => identity_hoprd_mutable.create(context.clone()).await,
        IdentityHoprdAction::Modify => identity_hoprd_mutable.modify(context.clone()).await,
        IdentityHoprdAction::Delete => identity_hoprd_mutable.delete(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        IdentityHoprdAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY))),
    }
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `identity_hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
pub fn on_error(identity_hoprd: Arc<IdentityHoprd>, error: &Error, _context: Arc<ContextData>) -> Action {
    error!("[IdentityHoprd] Reconciliation error:\n{:?}.\n{:?}", error, identity_hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY))
}

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<IdentityHoprd> = Api::<IdentityHoprd>::all(client.clone());
    let pvc = Api::<PersistentVolumeClaim>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(pvc, Config::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_identity_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") && !err_string.contains("event queue error") {
                        // https://github.com/kube-rs/kube/issues/712
                        error!("[IdentityHoprd] Reconciliation error: {:?}", reconciliation_err)
                    }
                }
            }
        })
        .await;
}
