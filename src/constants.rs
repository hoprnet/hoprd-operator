use std::fmt::{Display, Formatter};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Operator Constants
pub const RECONCILE_SHORT_FREQUENCY: u64 = 10;
pub const RECONCILE_LONG_FREQUENCY: u64 = 30;
pub const OPERATOR_ENVIRONMENT: &str = "OPERATOR_ENVIRONMENT";
pub const OPERATOR_FINALIZER: &str = "hoprds.hoprnet.org/finalizer";
pub const OPERATOR_JOB_TIMEOUT: u64 = 300;
// This value `OPERATOR_NODE_SYNC_TIMEOUT` should be lower than 295
pub const OPERATOR_NODE_SYNC_TIMEOUT: u32 = 290;
pub const IDENTITY_POOL_WALLET_DEPLOYER_PRIVATE_KEY_REF_KEY: &str = "DEPLOYER_PRIVATE_KEY";
pub const IDENTITY_POOL_WALLET_PRIVATE_KEY_REF_KEY: &str = "PRIVATE_KEY";
pub const IDENTITY_POOL_IDENTITY_PASSWORD_REF_KEY: &str = "IDENTITY_PASSWORD";
pub const IDENTITY_POOL_API_TOKEN_REF_KEY: &str = "HOPRD_API_TOKEN";

// Annotations
pub const ANNOTATION_LAST_CONFIGURATION: &str = "kubectl.kubernetes.io/last-applied-configuration";
pub const ANNOTATION_EXTERNAL_DNS_HOSTNAME: &str = "external-dns.alpha.kubernetes.io/hostname";

// Labels
pub const LABEL_KUBERNETES_NAME: &str = "app.kubernetes.io/name";
pub const LABEL_KUBERNETES_INSTANCE: &str = "app.kubernetes.io/instance";
pub const LABEL_KUBERNETES_COMPONENT: &str = "app.kubernetes.io/component";

pub const LABEL_KUBERNETES_IDENTITY_POOL: &str = "hoprds.hoprnet.org/identitypool";
pub const LABEL_NODE_PEER_ID: &str = "hoprds.hoprnet.org/peerId";
pub const LABEL_NODE_NATIVE_ADDRESS: &str = "hoprds.hoprnet.org/nativeAddress";
pub const LABEL_NODE_SAFE_ADDRESS: &str = "hoprds.hoprnet.org/safeAddress";
pub const LABEL_NODE_MODULE_ADDRESS: &str = "hoprds.hoprnet.org/moduleAddress";
pub const LABEL_NODE_NETWORK: &str = "hoprds.hoprnet.org/network";
pub const LABEL_NODE_CLUSTER: &str = "hoprds.hoprnet.org/cluster";

// Kubernetes Specs
pub const HOPR_DOCKER_REGISTRY: &str = "europe-west3-docker.pkg.dev";
pub const HOPR_DOCKER_IMAGE_NAME: &str = "hoprassociation/docker-images/hoprd";
pub const HOPR_DOCKER_METRICS_IMAGE_NAME: &str = "hoprassociation/docker-images/hoprd-operator-metrics";

// HOPRD Arguments
pub const HOPRD_IDENTITY_FILE: &str = "HOPRD_IDENTITY_FILE";
pub const HOPRD_CONFIGURATION: &str = "HOPRD_CONFIGURATION";
pub const HOPRD_SAFE_ADDRESS: &str = "HOPRD_SAFE_ADDRESS";
pub const HOPRD_MODULE_ADDRESS: &str = "HOPRD_MODULE_ADDRESS";
pub const HOPRD_HOST: &str = "HOPRD_HOST";
pub const HOPRD_API: &str = "HOPRD_API";
pub const HOPRD_SESSION_PORT_RANGE: &str = "HOPRD_SESSION_PORT_RANGE";

#[derive(Serialize, Deserialize, Debug, PartialEq, Default, Clone, Hash, Copy, JsonSchema)]
pub enum SupportedReleaseEnum {
    #[default]
    #[serde(rename = "saint-louis")]
    SaintLouis,
    #[serde(rename = "kaunas")]
    Kaunas,
}

impl Display for SupportedReleaseEnum {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            SupportedReleaseEnum::SaintLouis => write!(f, "saint-louis"),
            SupportedReleaseEnum::Kaunas => write!(f, "kaunas"),

        }
    }
}
