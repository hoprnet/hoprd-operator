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

impl IngressConfig {
    /// Checks at boot the fields that the nginx integration cannot work without, and that `port_min` is lower than
    /// `port_max` for every ingress class, so a misconfiguration stops the operator at startup instead of failing every
    /// reconciliation later.
    pub fn validate(&self) -> Result<(), String> {
        if self.ingress_class_name == "nginx" {
            if self.namespace.is_none() {
                return Err("ingress.namespace is required when ingress_class_name is 'nginx'".to_owned());
            }
            if self.deployment_name.is_none() {
                return Err("ingress.deployment_name is required when ingress_class_name is 'nginx'".to_owned());
            }
        }
        if self.port_min >= self.port_max {
            return Err("ingress.port_min must be lower than ingress.port_max".to_owned());
        }
        Ok(())
    }

    /// The ingress namespace. It is guaranteed by `validate` for nginx, so this only fails for other ingress classes.
    pub fn namespace(&self) -> Result<&str, crate::model::Error> {
        self.namespace.as_deref().ok_or_else(|| crate::model::Error::MissingField("ingress.namespace".to_owned()))
    }

    /// The ingress controller deployment name. It is guaranteed by `validate` for nginx, so this only fails for other ingress classes.
    pub fn deployment_name(&self) -> Result<&str, crate::model::Error> {
        self.deployment_name.as_deref().ok_or_else(|| crate::model::Error::MissingField("ingress.deployment_name".to_owned()))
    }
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
