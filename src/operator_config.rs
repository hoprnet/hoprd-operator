use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorConfig {
    pub instance: OperatorInstance,
    pub ingress: IngressConfig,
    pub hopli_image: String,
    pub hopli_rpc_provider_url: String,
    pub persistence: PersistenceConfig,
    pub webhook: WebhookConfig,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorInstance {
    pub name: String,
    pub namespace: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct IngressConfig {
    pub ingress_class_name: String,
    pub dns_domain: String,
    pub namespace: Option<String>,
    pub annotations: Option<BTreeMap<String, String>>,
    pub loadbalancer_ip: String,
    pub port_min: u16,
    pub port_max: u16,
    pub deployment_name: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct PersistenceConfig {
    pub size: String,
    pub storage_class_name: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct WebhookConfig {
    pub crt_file: String,
    pub key_file: String,
}
