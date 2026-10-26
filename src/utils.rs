use crate::constants;
use crate::model::Error;
use k8s_openapi::serde::de::DeserializeOwned;
use kube::api::{DeleteParams, Patch, PatchParams};
use kube::runtime::{conditions, wait::await_condition};
use kube::{Api, Resource, ResourceExt};
use std::collections::BTreeMap;
use crate::context_data::ContextData;
use kube::runtime::controller::Action;
use std::fmt::{Debug, Display};
use std::sync::Arc;
use std::time::Duration;
use serde::Serialize;
use serde_json::json;
use tracing::{error, info};

/// Turns the absence of a value into `Error::MissingField` so callers can escalate it with `?` instead of unwrapping.
pub trait OptionExt<T> {
    fn or_missing(self, what: &str) -> Result<T, Error>;
}

impl<T> OptionExt<T> for Option<T> {
    fn or_missing(self, what: &str) -> Result<T, Error> {
        self.ok_or_else(|| Error::MissingField(what.to_owned()))
    }
}

/// Fallible accessors for the identity of a Kubernetes resource, replacing `namespace().unwrap()`.
pub trait ResourceIdentity: ResourceExt {
    fn require_namespace(&self) -> Result<String, Error> {
        self.namespace().ok_or_else(|| Error::MissingField(format!("namespace of {}", self.name_any())))
    }
    fn require_name(&self) -> Result<String, Error> {
        self.meta().name.clone().ok_or_else(|| Error::MissingField("metadata.name".to_owned()))
    }
    fn require_uid(&self) -> Result<String, Error> {
        self.uid().ok_or_else(|| Error::MissingField(format!("metadata.uid of {}", self.name_any())))
    }
}

impl<K: ResourceExt> ResourceIdentity for K {}

/// Error policy shared by all the controllers: logs the failure and requeues only the offending resource.
pub fn on_error<K: Resource<DynamicType = ()> + Debug>(resource: Arc<K>, error: &Error, _context: Arc<ContextData>) -> Action {
    error!("[{}] Reconciliation error:\n{:?}.\n{:?}", K::kind(&()), error, resource);
    Action::requeue(Duration::from_secs(constants::RECONCILE_SHORT_FREQUENCY))
}

/// Logs the outcome of a reconciliation, skipping the known noise described in https://github.com/kube-rs/kube/issues/712
pub fn log_reconciliation_result<T, E: Debug + Display>(kind: &str, result: Result<T, E>) {
    if let Err(reconciliation_err) = result {
        let err_string = reconciliation_err.to_string();
        if !err_string.contains("that was not found in local store") && !err_string.contains("event queue error") {
            error!("[{kind}] Reconciliation error: {:?}", reconciliation_err)
        }
    }
}

/// Merge-patches the `status` subresource of the named resource.
///
/// A failure is returned instead of being logged and ignored, so the reconciliation is requeued and the status is retried.
pub async fn patch_status<K, S>(api: &Api<K>, name: &str, status: &S) -> Result<K, Error>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned,
    S: Serialize,
{
    let patch = Patch::Merge(json!({ "status": status }));
    api.patch_status(name, &PatchParams::default(), &patch).await.map_err(|error| {
        let kind = K::kind(&());
        error!("Could not update status on {kind} {name}: {:?}", error);
        Error::HoprdStatusError(format!("Failed to update status for {kind} '{name}': {error}"))
    })
}

/// Deletes the named resource if it exists and waits until it is gone.
///
/// Returns `true` when the resource existed and has been deleted, and `false` when there was nothing to delete.
pub async fn delete_and_wait<K>(api: Api<K>, name: &str) -> Result<bool, Error>
where
    K: Resource<DynamicType = ()> + Clone + Debug + DeserializeOwned + Send + 'static,
{
    let kind = K::kind(&());
    match api.get_opt(name).await? {
        Some(resource) => {
            api.delete(name, &DeleteParams::default()).await?;
            await_deleted(api, &resource).await?;
            info!("{kind} {name} successfully deleted");
            Ok(true)
        }
        None => {
            info!("{kind} {name} about to delete not found");
            Ok(false)
        }
    }
}

/// Waits until the given resource, already requested to be deleted, is gone from the cluster.
///
/// The wait is bounded by `OPERATOR_JOB_TIMEOUT`, so a stuck finalizer can neither hang a reconciler forever nor panic it:
/// it becomes an `Error` and the resource is requeued.
pub async fn await_deleted<K>(api: Api<K>, resource: &K) -> Result<(), Error>
where
    K: Resource + Clone + Debug + DeserializeOwned + Send + 'static,
{
    let name = resource.name_any();
    let uid = resource.require_uid()?;
    let timeout = Duration::from_secs(constants::OPERATOR_JOB_TIMEOUT);
    match tokio::time::timeout(timeout, await_condition(api, &name, conditions::is_deleted(&uid))).await {
        Ok(result) => result.map(|_| ()).map_err(Error::from),
        Err(_) => Err(Error::JobExecutionError(format!("Timed out waiting for the deletion of {name}"))),
    }
}

pub fn common_lables(name: String, instance: Option<String>, component: Option<String>) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(constants::LABEL_KUBERNETES_NAME.to_owned(), name);
    if let Some(instance) = instance {
        labels.insert(constants::LABEL_KUBERNETES_INSTANCE.to_owned(), instance);
    }
    if let Some(component) = component {
        labels.insert(constants::LABEL_KUBERNETES_COMPONENT.to_owned(), component);
    }
    labels
}

/// Runs a reconciliation turning any panic into an error, so a malformed resource can never take down the shared operator.
///
/// Without this guard, a panic inside a reconciler propagates through the `select!` in `main.rs` and stops all the
/// controllers. With it, the panic becomes an `Error` that kube-rs handles like any other failed reconciliation:
/// it calls `on_error` and requeues only the offending resource.
///
/// # Generic parameters
/// - `T`: the value returned by a successful reconciliation (`Action` for all the controllers). The function does
///   not care about it, it only hands it back untouched, so it is generic and can wrap any reconciler.
/// - `F`: the type of the reconciliation, which is an `async fn` call, so a `Future` that has not been run yet.
///   `Future<Output = Result<T, Error>>` states that, once awaited, it produces the same `Result<T, Error>` that
///   a reconciler returns. That is what lets this function return the original result on the happy path.
///
/// # Arguments
/// - `kind`: name of the resource kind (`Hoprd`, `IdentityPool`...), only used to build the error message.
/// - `reconciliation`: the future of the reconciler to protect, for example `reconcile(hoprd, context)`.
///
/// # Returns
/// - The result of the reconciliation, if it finished normally (either `Ok` or its own `Err`).
/// - `Error::ReconcilerPanic`, if the reconciliation panicked.
pub async fn catch_panics<T, F: std::future::Future<Output = Result<T, Error>>>(kind: &str, reconciliation: F) -> Result<T, Error> {
    use futures::FutureExt;
    // `catch_unwind` requires the future to be `UnwindSafe`, which the compiler cannot prove for the references
    // held by the reconcilers. `AssertUnwindSafe` is a promise that it is fine here: after a panic the future is
    // dropped and nothing it touched is reused, as the next reconciliation starts from scratch.
    match std::panic::AssertUnwindSafe(reconciliation).catch_unwind().await {
        // Finished without panic: `catch_unwind` wraps the output in `Ok`, so the reconciler's own `Result` is returned as is
        Ok(result) => result,
        // Panicked: the payload is discarded and replaced by an error that requeues the resource
        Err(_) => Err(Error::ReconcilerPanic(format!("The reconciliation of a {kind} panicked"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hoprd::hoprd_resource::Hoprd;
    use kube::api::ObjectMeta;

    fn hoprd(name: Option<&str>, namespace: Option<&str>, uid: Option<&str>) -> Hoprd {
        let mut hoprd = Hoprd::default();
        hoprd.metadata = ObjectMeta {
            name: name.map(str::to_owned),
            namespace: namespace.map(str::to_owned),
            uid: uid.map(str::to_owned),
            ..ObjectMeta::default()
        };
        hoprd
    }

    #[test]
    fn common_labels_include_the_optional_ones_only_when_present() {
        let labels = common_lables("pool".to_owned(), Some("node-1".to_owned()), Some("node".to_owned()));
        assert_eq!(labels.get(constants::LABEL_KUBERNETES_NAME), Some(&"pool".to_owned()));
        assert_eq!(labels.get(constants::LABEL_KUBERNETES_INSTANCE), Some(&"node-1".to_owned()));
        assert_eq!(labels.get(constants::LABEL_KUBERNETES_COMPONENT), Some(&"node".to_owned()));

        let labels = common_lables("pool".to_owned(), None, None);
        assert_eq!(labels.len(), 1);
    }

    #[test]
    fn or_missing_returns_the_value_or_a_missing_field_error() {
        assert_eq!(Some(7).or_missing("value").ok(), Some(7));
        match None::<i32>.or_missing("some.field") {
            Err(Error::MissingField(field)) => assert_eq!(field, "some.field"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn resource_identity_returns_the_metadata_when_present() {
        let resource = hoprd(Some("node"), Some("ns"), Some("uid-1"));
        assert_eq!(resource.require_name().ok(), Some("node".to_owned()));
        assert_eq!(resource.require_namespace().ok(), Some("ns".to_owned()));
        assert_eq!(resource.require_uid().ok(), Some("uid-1".to_owned()));
    }

    #[test]
    fn resource_identity_fails_when_the_metadata_is_missing() {
        let resource = hoprd(None, None, None);
        assert!(matches!(resource.require_name(), Err(Error::MissingField(_))));
        assert!(matches!(resource.require_namespace(), Err(Error::MissingField(_))));
        assert!(matches!(resource.require_uid(), Err(Error::MissingField(_))));
    }

    #[tokio::test]
    async fn catch_panics_returns_the_result_of_a_finished_reconciliation() {
        assert_eq!(catch_panics("Test", async { Ok::<_, Error>(5) }).await.ok(), Some(5));
        let failed = catch_panics("Test", async { Err::<i32, _>(Error::UserInputError("bad".to_owned())) }).await;
        assert!(matches!(failed, Err(Error::UserInputError(_))));
    }

    #[tokio::test]
    async fn catch_panics_turns_a_panic_into_an_error() {
        let result = catch_panics("Hoprd", async {
            if true {
                panic!("boom");
            }
            Ok::<_, Error>(())
        })
        .await;
        match result {
            Err(Error::ReconcilerPanic(message)) => assert!(message.contains("Hoprd")),
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
