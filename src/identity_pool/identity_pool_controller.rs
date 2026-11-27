use futures::StreamExt;
use crate::utils::ResourceIdentity;
use k8s_openapi::api::batch::v1::CronJob;
use kube::{
    api::Api,
    client::Client,
    runtime::{
        controller::{Action, Controller},
        reflector::{store::Writer, ObjectRef},
        watcher::{self, Config},
        WatchStreamExt,
    },
    Resource, ResourceExt, Result,
};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::error;

use crate::{
    constants::{self},
    context_data::ContextData,
    identity_hoprd::identity_hoprd_resource::IdentityHoprd,
    identity_pool::identity_pool_resource::{IdentityPool, IdentityPoolPhaseEnum},
    model::Error,
    utils,
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
    /// The owned auto-funding `CronJob` is missing (e.g. manually deleted) and needs to be recreated
    RecreateCronJobFaucet,
    /// This `IdentityPool` resource is in desired state and requires no actions to be taken
    NoOp,
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `IdentityPool` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `IdentityPoolAction` enum.
///
/// # Arguments
/// - `identity_hoprd`: A reference to `IdentityPool` being reconciled to decide next action upon.
/// - `context`: Context Data used to query the current state of owned resources like the auto-funding `CronJob`.
async fn determine_action(identity_pool: &IdentityPool, context: &Arc<ContextData>) -> Result<IdentityPoolAction, Error> {
    if identity_pool.meta().deletion_timestamp.is_some() {
        return Ok(IdentityPoolAction::Delete);
    } else if identity_pool.meta().finalizers.as_ref().is_none_or(|finalizers| finalizers.is_empty()) {
        return Ok(IdentityPoolAction::Create);
    } else if identity_pool.status.as_ref().is_some_and(|status| status.phase.eq(&IdentityPoolPhaseEnum::OutOfSync)) {
        return Ok(IdentityPoolAction::Sync);
    }
    if identity_pool.spec.funding.is_some() {
        let cron_job_name = format!("auto-funding-{}", identity_pool.name_any());
        let cron_job_ref = ObjectRef::new(&cron_job_name).within(&identity_pool.require_namespace()?);
        if context.cron_job_faucet_store.get(&cron_job_ref).is_none() {
            // Confirm against the API server, as the cache may not be populated yet after the operator startup
            let cron_job_api: Api<CronJob> = Api::namespaced(context.client.clone(), &identity_pool.require_namespace()?);
            if cron_job_api.get_opt(&cron_job_name).await?.is_none() {
                return Ok(IdentityPoolAction::RecreateCronJobFaucet);
            }
        }
    }
    let current_generation = identity_pool.meta().generation.unwrap_or(0);
    let observed_generation = identity_pool.status.as_ref().map_or(0, |status| status.observed_generation);
    if observed_generation < current_generation {
        Ok(IdentityPoolAction::Modify)
    } else {
        Ok(IdentityPoolAction::NoOp)
    }
}

async fn reconciler(identity_pool: Arc<IdentityPool>, context: Arc<ContextData>) -> Result<Action, Error> {
    utils::catch_panics("IdentityPool", reconcile(identity_pool, context)).await
}

async fn reconcile(identity_pool: Arc<IdentityPool>, context: Arc<ContextData>) -> Result<Action, Error> {
    let mut identity_pool_cloned = identity_pool.clone();
    let identity_pool_mutable: &mut IdentityPool = Arc::<IdentityPool>::make_mut(&mut identity_pool_cloned);
    // Performs action as decided by the `determine_action` function.
    match determine_action(identity_pool_mutable, &context).await? {
        IdentityPoolAction::Create => identity_pool_mutable.create(context.clone()).await,
        IdentityPoolAction::Modify => identity_pool_mutable.modify(context.clone()).await,
        IdentityPoolAction::Sync => identity_pool_mutable.sync(context.clone()).await,
        IdentityPoolAction::Delete => identity_pool_mutable.delete(context.clone()).await,
        IdentityPoolAction::RecreateCronJobFaucet => identity_pool_mutable.recreate_cron_job_faucet(context.clone()).await,
        // The resource is already in desired state, do nothing and re-check after 10 seconds
        IdentityPoolAction::NoOp => Ok(Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY))),
    }
}

/// Initialize the controller
pub async fn run(client: Client, context_data: Arc<ContextData>, cron_job_faucet_writer: Writer<CronJob>) {
    let owned_api: Api<IdentityPool> = Api::<IdentityPool>::all(client.clone());
    let identity_hoprd = Api::<IdentityHoprd>::all(client.clone());
    let cron_job_faucet = Api::<CronJob>::all(client.clone());

    // Keep the in-memory cache of identity pool auto-funding CronJobs up to date, so that the
    // reconciliation loop can check for their existence without querying the API server
    let cron_job_faucet_reflector = watcher::watcher(cron_job_faucet.clone(), Config::default().labels(constants::LABEL_KUBERNETES_IDENTITY_POOL))
        .default_backoff()
        .reflect(cron_job_faucet_writer)
        .applied_objects();
    tokio::spawn(async move {
        futures::pin_mut!(cron_job_faucet_reflector);
        while let Some(event) = cron_job_faucet_reflector.next().await {
            if let Err(watcher_error) = event {
                error!("[IdentityPool] CronJob faucet reflector error: {:?}", watcher_error);
            }
        }
    });

    Controller::new(owned_api, Config::default())
        .owns(identity_hoprd, Config::default())
        .owns(cron_job_faucet, Config::default())
        .shutdown_on_signal()
        .run(reconciler, utils::on_error, context_data)
        .for_each(|reconciliation_result| async move { utils::log_reconciliation_result("IdentityPool", reconciliation_result) })
        .await;
}
