use crate::model::Hoprd;
use kube::api::{Patch, PatchParams};
use kube::{Api, Client, Error};
use serde_json::{json, Value};

/// Adds a finalizer record into an `Hoprd` kind of resource. If the finalizer already exists,
/// this action has no effect.
///
/// # Arguments:
/// - `client` - Kubernetes client to modify the `Hoprd` resource with.
/// - `name` - Name of the `Hoprd` resource to modify. Existence is not verified
/// - `namespace` - Namespace where the `Hoprd` resource with given `name` resides.
///

pub async fn add_finalizer(client: Client, name: &str, namespace: &str) -> Result<Hoprd, Error> {
    let api: Api<Hoprd> = Api::namespaced(client, namespace);
    let finalizer: Value = json!({
        "metadata": {
            "finalizers": ["hoprds.hoprnet.org/finalizer"]
        }
    });

    let patch: Patch<&Value> = Patch::Merge(&finalizer);
    Ok(api.patch(name, &PatchParams::default(), &patch).await?)
}

/// Removes all finalizers from an `Hoprd` resource. If there are no finalizers already, this
/// action has no effect.
///
/// # Arguments:
/// - `client` - Kubernetes client to modify the `Hoprd` resource with.
/// - `name` - Name of the `Hoprd` resource to modify. Existence is not verified
/// - `namespace` - Namespace where the `Hoprd` resource with given `name` resides.
///
pub async fn delete_finalizer(client: Client, name: &str, namespace: &str) -> Result<Hoprd, Error> {
    let api: Api<Hoprd> = Api::namespaced(client, namespace);
    let finalizer: Value = json!({
        "metadata": {
            "finalizers": null
        }
    });

    let patch: Patch<&Value> = Patch::Merge(&finalizer);
    Ok(api.patch(name, &PatchParams::default(), &patch).await?)
}
