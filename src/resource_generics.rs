use k8s_openapi::NamespaceResourceScope;
use kube::{client::Client, Api, api::{PatchParams, Patch}, ResourceExt};
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::{error, debug};
use crate::constants;
use std::fmt::Debug;

/// Adds a finalizer record into the K8s resource
pub async fn add_finalizer<K : kube::Resource<Scope = NamespaceResourceScope, DynamicType = ()> + Clone + Debug + DeserializeOwned + Default>(client: Client, resource: &K) {
    let name = resource.name_any();
    let namespace = resource.namespace().unwrap();
    let api: Api<K> = Api::namespaced(client.clone(), &namespace.to_owned());
    let patch = Patch::Merge(json!({
    "metadata": {
            "finalizers": [constants::OPERATOR_FINALIZER]
        }
    }));
    match api.patch(&name, &PatchParams::default(), &patch).await
    {
        Ok(_) => (),
        Err(error) => {
            error!("Could not add finalizer on resource {name}: {:?}",error);
        }
    };
}

 /// Removes all finalizers from the resource
pub async fn delete_finalizer<K : kube::Resource<Scope = NamespaceResourceScope, DynamicType = ()> + Clone + Debug + DeserializeOwned + Default>(client: Client, resource: &K) {
    let name = resource.name_any();
    let namespace = resource.namespace().unwrap();
    let api: Api<K> = Api::namespaced(client.clone(), &namespace.to_owned());
     let patch = Patch::Merge(json!({
        "metadata": {
             "finalizers": null
         }
     }));
     if let Some(_) = api.get_opt(&name).await.unwrap_or(None) {
         match api.patch(&name, &PatchParams::default(), &patch).await
         {
             Ok(_hopr) => (),
             Err(error) => error!("Could not delete finalizer on Hoprd node {name}: {:?}", error)
         }
     } else {
         debug!("Hoprd node {name} already deleted")
     }
 }
