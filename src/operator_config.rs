use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorConfig {
    pub instance: OperatorInstance,
    pub ingress: IngressConfig,
    pub hopli_image: String,
    pub persistence: PersistenceConfig,
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
    pub loadbalancer_ip: Option<String>,
    pub p2p_port_min: Option<String>,
    pub p2p_port_max: Option<String>,
    pub deployment_name: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct PersistenceConfig {
    pub size: String,
    pub storage_class_name: String,
}
