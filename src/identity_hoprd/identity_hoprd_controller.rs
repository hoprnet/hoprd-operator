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

use crate::{
    constants::{self},
    context_data::ContextData,
    identity_hoprd::identity_hoprd_resource::IdentityHoprd,
    model::Error,
    utils,
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
        let current_generation = identity_hoprd.meta().generation.unwrap_or(0);
        let observed_generation = identity_hoprd.status.as_ref().map_or(0, |status| status.observed_generation);
        if observed_generation < current_generation {
            IdentityHoprdAction::Modify
        } else {
            IdentityHoprdAction::NoOp
        }
    };
}

async fn reconciler(identity_hoprd: Arc<IdentityHoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
    utils::catch_panics("IdentityHoprd", reconcile(identity_hoprd, context)).await
}

async fn reconcile(identity_hoprd: Arc<IdentityHoprd>, context: Arc<ContextData>) -> Result<Action, Error> {
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

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>) {
    let owned_api: Api<IdentityHoprd> = Api::<IdentityHoprd>::all(client.clone());
    let pvc = Api::<PersistentVolumeClaim>::all(client.clone());

    Controller::new(owned_api, Config::default())
        .owns(pvc, Config::default())
        .shutdown_on_signal()
        .run(reconciler, utils::on_error, context_data)
        .for_each(|reconciliation_result| async move { utils::log_reconciliation_result("IdentityHoprd", reconciliation_result) })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity(finalizers: bool, deleting: bool, generation: i64, observed_generation: Option<i64>) -> IdentityHoprd {
        let mut metadata = json!({ "name": "identity", "namespace": "ns", "generation": generation });
        if finalizers {
            metadata["finalizers"] = json!(["hoprnet.org/finalizer"]);
        }
        if deleting {
            metadata["deletionTimestamp"] = json!("2024-01-01T00:00:00Z");
        }
        let mut resource = json!({
            "apiVersion": "hoprnet.org/v1alpha3",
            "kind": "IdentityHoprd",
            "metadata": metadata,
            "spec": { "identityPoolName": "pool", "identityFile": "", "nodeAddress": "", "safeAddress": "", "moduleAddress": "" },
        });
        if let Some(observed_generation) = observed_generation {
            resource["status"] = json!({ "phase": "Ready", "observedGeneration": observed_generation });
        }
        serde_json::from_value(resource).expect("valid IdentityHoprd")
    }

    #[test]
    fn deletion_wins_over_everything_else() {
        assert!(matches!(determine_action(&identity(true, true, 1, Some(1))), IdentityHoprdAction::Delete));
        assert!(matches!(determine_action(&identity(false, true, 1, None)), IdentityHoprdAction::Delete));
    }

    #[test]
    fn a_resource_without_finalizer_is_created() {
        assert!(matches!(determine_action(&identity(false, false, 1, None)), IdentityHoprdAction::Create));
    }

    #[test]
    fn generation_decides_between_modify_and_noop() {
        assert!(matches!(determine_action(&identity(true, false, 2, Some(1))), IdentityHoprdAction::Modify));
        assert!(matches!(determine_action(&identity(true, false, 2, Some(2))), IdentityHoprdAction::NoOp));
        // Without status the observed generation is 0, so the resource is reconciled
        assert!(matches!(determine_action(&identity(true, false, 1, None)), IdentityHoprdAction::Modify));
    }
}
