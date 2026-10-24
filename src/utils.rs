use crate::constants;
use crate::model::Error;
use std::collections::BTreeMap;

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
