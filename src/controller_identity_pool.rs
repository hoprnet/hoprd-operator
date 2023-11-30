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
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};
use tokio::time::Duration;
use tracing::error;

use crate::{
    constants::{self},
    context_data::ContextData,
    identity_pool::{IdentityPool, IdentityPoolSpec},
    model::Error,
    servicemonitor::ServiceMonitor, identity_hoprd::IdentityHoprd,
};

/// Action to be taken upon an `IdentityPool` resource during reconciliation
enum IdentityPoolAction {
    /// Validate the data on-chain
    Create,
    /// Synchronize the identity pool
    Sync,
    /// Modify the IdentityPool resource and validate data on-chain
    Modify,
    /// Delete the IdentityPool resource
    Delete,
    /// This `IdentityPool` resource is in desired state and requires no actions to be taken
    NoOp,
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `IdentityPool` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `IdentityPoolAction` enum.
///
/// # Arguments
/// - `identity_hoprd`: A reference to `IdentityPool` being reconciled to decide next action upon.
fn determine_action(identity_pool: &IdentityPool) -> IdentityPoolAction {
    return if identity_pool.meta().deletion_timestamp.is_some() {
        IdentityPoolAction::Delete
    } else if identity_pool.meta().finalizers.as_ref().map_or(true, |finalizers| finalizers.is_empty())
    {
        IdentityPoolAction::Create
    } else if identity_pool.status.as_ref().unwrap().phase.eq(&crate::identity_pool::IdentityPoolPhaseEnum::OutOfSync)
    {
        IdentityPoolAction::Sync
    } else {
        let current_checksum = identity_pool.get_checksum();
        let previous_checksum: String = identity_pool.status.as_ref()
            .map_or("0".to_owned(), |status| status.checksum.to_owned());
        // When the resource is created, does not have previous checksum and needs to be skip the modification because it's being handled already by the creation operation
        if previous_checksum.eq(&"0".to_owned()) || current_checksum.eq(&previous_checksum) {
            IdentityPoolAction::NoOp
        } else {
            IdentityPoolAction::Modify
        }
    };
}

async fn reconciler(identity_pool: Arc<IdentityPool>, context: Arc<ContextData>) -> Result<Action, Error> {
    let mut identity_pool_cloned = identity_pool.clone();
    let identity_pool_mutable:  &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_cloned);
    // Performs action as decided by the `determine_action` function.
    return match determine_action(identity_pool_mutable) {
        IdentityPoolAction::Create => identity_pool_mutable.create(context.clone()).await,
        IdentityPoolAction::Modify => identity_pool_mutable.modify(context.clone()).await,
        IdentityPoolAction::Sync => identity_pool_mutable.sync(context.clone()).await,
        IdentityPoolAction::Delete => identity_pool_mutable.delete(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        IdentityPoolAction::NoOp => Ok(Action::requeue(Duration::from_secs(
            constants::RECONCILE_FREQUENCY,
        ))),
    };
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `identity_hoprd`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
pub fn on_error(
    identity_hoprd: Arc<IdentityPool>,
    error: &Error,
    _context: Arc<ContextData>,
) -> Action {
    error!("[IdentityPool] Reconciliation error:\n{:?}.\n{:?}",error, identity_hoprd);
    Action::requeue(Duration::from_secs(constants::RECONCILE_FREQUENCY))
}

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<IdentityPool> = Api::<IdentityPool>::all(client.clone());
    let service_monitor = Api::<ServiceMonitor>::all(client.clone());
    let identity_hoprd = Api::<IdentityHoprd>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(service_monitor, Config::default())
        .owns(identity_hoprd, Config::default())
        .shutdown_on_signal()
        .run(reconciler, on_error, context_data)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(_identity_hoprd_resource) => {}
                Err(reconciliation_err) => {
                    let err_string = reconciliation_err.to_string();
                    if !err_string.contains("that was not found in local store") {
                        // https://github.com/kube-rs/kube/issues/712
                        error!("[IdentityPool] Reconciliation error: {:?}",reconciliation_err)
                    }
                }
            }
        })
        .await;
}
