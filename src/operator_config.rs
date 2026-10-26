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

#[cfg(test)]
mod tests {
    use super::*;

    fn ingress(class: &str, namespace: Option<&str>, deployment: Option<&str>, port_min: u16, port_max: u16) -> IngressConfig {
        IngressConfig {
            ingress_class_name: class.to_owned(),
            dns_domain: "example.org".to_owned(),
            namespace: namespace.map(str::to_owned),
            annotations: None,
            loadbalancer_ip: "1.2.3.4".to_owned(),
            port_min,
            port_max,
            deployment_name: deployment.map(str::to_owned),
        }
    }

    #[test]
    fn nginx_requires_namespace_and_deployment_name() {
        assert!(ingress("nginx", Some("ingress-nginx"), Some("controller"), 9000, 10000).validate().is_ok());
        assert!(ingress("nginx", None, Some("controller"), 9000, 10000).validate().is_err());
        assert!(ingress("nginx", Some("ingress-nginx"), None, 9000, 10000).validate().is_err());
    }

    #[test]
    fn other_ingress_classes_do_not_require_them() {
        assert!(ingress("traefik", None, None, 9000, 10000).validate().is_ok());
    }

    #[test]
    fn port_range_must_be_ordered() {
        assert!(ingress("traefik", None, None, 10000, 10000).validate().is_err());
        assert!(ingress("traefik", None, None, 10001, 10000).validate().is_err());
    }

    #[test]
    fn accessors_fail_when_the_field_is_missing() {
        let config = ingress("traefik", None, None, 9000, 10000);
        assert!(config.namespace().is_err());
        assert!(config.deployment_name().is_err());
        let config = ingress("nginx", Some("ns"), Some("dep"), 9000, 10000);
        assert_eq!(config.namespace().ok(), Some("ns"));
        assert_eq!(config.deployment_name().ok(), Some("dep"));
    }

    #[test]
    fn sample_configs_are_valid() {
        for file in ["test-data/sample_config-prod.yaml", "test-data/sample_config-staging.yaml"] {
            let content = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), file)).expect("sample config is readable");
            let config: OperatorConfig = serde_yml::from_str(&content).expect("sample config is parseable");
            assert!(config.ingress.validate().is_ok(), "{file}");
        }
    }
}
