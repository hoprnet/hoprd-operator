use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorConfig {
    pub instance: OperatorInstance,
    pub gateway: GatewayConfig,
    /// Optional OTLP settings. When present, they are injected into every hoprd node.
    #[serde(default)]
    pub otlp: Option<OtlpConfig>,
    pub hopli_image: String,
    pub hopli_rpc_provider_url: String,
    pub kubectl_image: String,
    pub persistence: PersistenceConfig,
    pub webhook: WebhookConfig,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorInstance {
    pub name: String,
    pub namespace: String,
}

/// Configuration of the built-in Gateway and of the way the nodes are exposed through it
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct GatewayConfig {
    /// Name of the built-in Gateway created by the Helm chart of the operator
    pub gateway_name: String,
    /// Namespace of the built-in Gateway
    pub gateway_namespace: String,
    /// DNS suffix of the hostnames of the nodes
    pub dns_domain: String,
    /// Public IP of the Gateway, used as the host of the nodes attached to the built-in Gateway
    pub loadbalancer_ip: String,
    /// First port of the range the operator can allocate to nodes
    pub port_min: u16,
    /// Last port of the range the operator can allocate to nodes
    pub port_max: u16,
    /// cert-manager issuer used to issue the certificate of every node
    pub certificate_issuer: CertificateIssuerConfig,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct CertificateIssuerConfig {
    pub name: String,
    #[serde(default = "default_issuer_kind")]
    pub kind: String,
}

fn default_issuer_kind() -> String {
    "ClusterIssuer".to_owned()
}

/// OTLP settings forwarded to the hoprd nodes
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct OtlpConfig {
    /// OTLP endpoint of the collector (`HOPRD_OTLP_ENDPOINT`). `http(s)://` selects OTLP/HTTP, anything else gRPC
    pub endpoint: String,
    /// Signals to export (`HOPRD_OTEL_SIGNALS`), e.g. `traces,logs,metrics`
    #[serde(default)]
    pub signals: Option<String>,
    /// Metric export interval (`HOPRD_METRIC_EXPORT_INTERVAL`), e.g. `15000,hopr_session=1000`
    #[serde(default)]
    pub metric_export_interval: Option<String>,
}

impl GatewayConfig {
    /// Checks at boot the fields that the Gateway integration cannot work without, so a misconfiguration stops the
    /// operator at startup instead of failing every reconciliation later.
    pub fn validate(&self) -> Result<(), String> {
        if self.gateway_name.trim().is_empty() {
            return Err("gateway.gateway_name must not be empty".to_owned());
        }
        if self.gateway_namespace.trim().is_empty() {
            return Err("gateway.gateway_namespace must not be empty".to_owned());
        }
        if self.certificate_issuer.name.trim().is_empty() {
            return Err("gateway.certificate_issuer.name must not be empty".to_owned());
        }
        if self.port_min >= self.port_max {
            return Err("gateway.port_min must be lower than gateway.port_max".to_owned());
        }
        Ok(())
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

    fn gateway(port_min: u16, port_max: u16) -> GatewayConfig {
        GatewayConfig {
            gateway_name: "hoprd-operator".to_owned(),
            gateway_namespace: "hoprd-operator".to_owned(),
            dns_domain: "example.org".to_owned(),
            loadbalancer_ip: "1.2.3.4".to_owned(),
            port_min,
            port_max,
            certificate_issuer: CertificateIssuerConfig { name: "issuer".to_owned(), kind: "ClusterIssuer".to_owned() },
        }
    }

    #[test]
    fn a_complete_gateway_config_is_valid() {
        assert!(gateway(9000, 10000).validate().is_ok());
    }

    #[test]
    fn port_range_must_be_ordered() {
        assert!(gateway(10000, 10000).validate().is_err());
        assert!(gateway(10001, 10000).validate().is_err());
    }

    #[test]
    fn names_and_issuer_are_required() {
        let mut config = gateway(9000, 10000);
        config.gateway_name = " ".to_owned();
        assert!(config.validate().is_err());
        let mut config = gateway(9000, 10000);
        config.gateway_namespace = String::new();
        assert!(config.validate().is_err());
        let mut config = gateway(9000, 10000);
        config.certificate_issuer.name = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn optional_fields_have_defaults() {
        let yaml = r#"
gateway_name: gw
gateway_namespace: ns
dns_domain: example.org
loadbalancer_ip: 1.2.3.4
port_min: 9000
port_max: 9100
certificate_issuer:
  name: issuer
"#;
        let config: GatewayConfig = serde_yml::from_str(yaml).expect("the config is parseable");
        assert_eq!(config.certificate_issuer.kind, "ClusterIssuer");
    }

    #[test]
    fn sample_configs_are_valid() {
        for file in ["test-data/sample_config-prod.yaml", "test-data/sample_config-staging.yaml"] {
            let content = std::fs::read_to_string(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), file)).expect("sample config is readable");
            let config: OperatorConfig = serde_yml::from_str(&content).expect("sample config is parseable");
            assert!(config.gateway.validate().is_ok(), "{file}");
        }
    }
}
