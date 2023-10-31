
use k8s_openapi::api::core::v1::{ PersistentVolumeClaim, PersistentVolumeClaimSpec, ResourceRequirements};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{ ObjectMeta, PostParams};
use kube::{Api, ResourceExt, Resource};
use std::collections::BTreeMap;
use std::sync::Arc;
use crate::context_data::ContextData;

use crate::identity_hoprd::IdentityHoprd;
use crate::utils;


/// Creates the Persitence Volume Claim
pub async fn create_pvc(context: Arc<ContextData>, identity_hoprd: &IdentityHoprd) -> Result<PersistentVolumeClaim, kube::Error> {
    let client = context.client.clone();
    let namespace: String = identity_hoprd.namespace().unwrap();
    let name: String= identity_hoprd.name_any();
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![identity_hoprd.controller_owner_ref(&()).unwrap()]);
    let labels: Option<BTreeMap<String, String>> = Some(utils::common_lables(context.config.instance.name.to_owned(),Some(name.to_owned()), None));
    let mut resource: BTreeMap<String, Quantity> = BTreeMap::new();
    resource.insert("storage".to_string(), Quantity(context.config.persistence.size.to_owned()));

    // Definition of the deployment. Alternatively, a YAML representation could be used as well.
    let pvc: PersistentVolumeClaim = PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels,
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".to_string()]),
            resources: Some(ResourceRequirements {
                requests: Some(resource),
                ..ResourceRequirements::default()
            }),
            storage_class_name: Some(context.config.persistence.storage_class_name.to_owned()),
            ..PersistentVolumeClaimSpec::default()
        }),
        ..PersistentVolumeClaim::default()
    };

    // Create the deployment defined above
    let api: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), &namespace);
    api.create(&PostParams::default(), &pvc).await
}
