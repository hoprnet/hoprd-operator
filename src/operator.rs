use std::sync::Arc;

use futures::stream::StreamExt;
use kube::api::ListParams;
use kube::{
    client::Client, runtime::Controller, Api,
};

use crate::actions;
use crate::crd::Hoprd;

pub async fn run_operator() {
    // First, a Kubernetes client must be obtained using the `kube` crate
    // The client will later be moved to the custom controller
    let kubernetes_client: Client = Client::try_default()
        .await
        .expect("Expected a valid KUBECONFIG environment variable.");

    // Preparation of resources used by the `kube_runtime::Controller`
    let crd_api: Api<Hoprd> = Api::all(kubernetes_client.clone());
    let context: Arc<ContextData> = Arc::new(ContextData::new(kubernetes_client.clone()));

    // The controller comes from the `kube_runtime` crate and manages the reconciliation process.
    // It requires the following information:
    // - `kube::Api<T>` this controller "owns". In this case, `T = Hoprd`, as this controller owns the `Hoprd` resource,
    // - `kube::api::ListParams` to select the `Hoprd` resources with. Can be used for Hoprd filtering `Hoprd` resources before reconciliation,
    // - `reconcile` function with reconciliation logic to be called each time a resource of `Hoprd` kind is created/updated/deleted,
    // - `on_error` function to call whenever reconciliation fails.
    Controller::new(crd_api.clone(), ListParams::default())
        .run(actions::reconcile, actions::on_error, context)
        .for_each(|reconciliation_result| async move {
            match reconciliation_result {
                Ok(hoprd_resource) => {
                    println!("Reconciliation successful. Resource: {:?}", hoprd_resource);
                }
                Err(reconciliation_err) => {
                    eprintln!("Reconciliation error: {:?}", reconciliation_err)
                }
            }
        })
        .await;
}

/// Context injected with each `reconcile` and `on_error` method invocation.
pub struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    pub client: Client,
}

impl ContextData {
    /// Constructs a new instance of ContextData.
    ///
    /// # Arguments:
    /// - `client`: A Kubernetes client to make Kubernetes REST API requests with. Resources
    /// will be created and deleted with this client.
    pub fn new(client: Client) -> Self {
        ContextData { client }
    }
}