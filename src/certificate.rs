//! Minimal typed view of the cert-manager `Certificate` resource: only the fields the operator uses.
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Default)]
#[kube(group = "cert-manager.io", version = "v1", kind = "Certificate", plural = "certificates", namespaced)]
#[serde(rename_all = "camelCase")]
pub struct CertificateSpec {
    /// Name of the Secret that receives the signed certificate and its private key
    pub secret_name: String,
    /// DNS names of the certificate
    pub dns_names: Vec<String>,
    /// Issuer that signs the certificate
    pub issuer_ref: CertificateIssuerRef,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct CertificateIssuerRef {
    pub name: String,
    pub kind: String,
    pub group: String,
}
