// Operator Constants
pub const RECONCILE_FREQUENCY: u64 = 10;
pub const OPERATOR_ENVIRONMENT: &str = "OPERATOR_ENVIRONMENT";
pub const OPERATOR_FINALIZER: &str = "hoprds.hoprnet.org/finalizer";
pub const OPERATOR_JOB_TIMEOUT: u64 = 180;
// This value `OPERATOR_NODE_SYNC_TIMEOUT` should be lower than 295
pub const OPERATOR_NODE_SYNC_TIMEOUT: u32 = 290;
pub const OPERATOR_INSTANCE_NAMESPACE: &str = "OPERATOR_INSTANCE_NAMESPACE";
pub const SECRET_NAME: &str = "SECRET_NAME";

// Annotations
pub const ANNOTATION_HOPRD_NETWORK_REGISTRY: &str = "hoprds.hoprnet.org/network_registry";
pub const ANNOTATION_HOPRD_FUNDED: &str = "hoprds.hoprnet.org/funded";
pub const ANNOTATION_REPLICATOR_NAMESPACES: &str = "replicator.v1.mittwald.de/replicate-to";
pub const ANNOTATION_LAST_CONFIGURATION: &str = "kubectl.kubernetes.io/last-applied-configuration";

// Labels
pub const LABEL_KUBERNETES_COMPONENT: &str = "app.kubernetes.io/component";
pub const LABEL_KUBERNETES_NAME: &str = "app.kubernetes.io/name";
pub const LABEL_KUBERNETES_INSTANCE: &str = "app.kubernetes.io/instance";
pub const LABEL_NODE_PEER_ID: &str = "hoprds.hoprnet.org/peerId";
pub const LABEL_NODE_ADDRESS: &str = "hoprds.hoprnet.org/address";
pub const LABEL_NODE_NETWORK: &str = "hoprds.hoprnet.org/network";
pub const LABEL_NODE_LOCKED: &str = "hoprds.hoprnet.org/locked";
pub const LABEL_NODE_CLUSTER: &str = "hoprds.hoprnet.org/cluster";

// FINALIZERS
pub const FINALIZER_SECRET: &str = "hoprds.hoprnet.org/secret";

// Kubernetes Specs
pub const HOPR_DOCKER_REGISTRY: &str = "gcr.io";
pub const HOPR_DOCKER_IMAGE_NAME: &str = "hoprassociation/hoprd";
pub const HOPR_PRIVATE_KEY: &str = "PRIVATE_KEY";
pub const HOPRD_PEER_ID: &str = "HOPRD_PEER_ID";
pub const HOPRD_ADDRESS: &str = "HOPRD_ADDRESS";
pub const HOPRD_METRICS_PASSWORD: &str = "HOPRD_METRICS_PASSWORD";
pub const HOPLI_ETHERSCAN_API_KEY: &str = "ETHERSCAN_API_KEY";

// HOPRD Arguments
pub const HOPRD_PASSWORD: &str = "HOPRD_PASSWORD";
pub const HOPRD_API_TOKEN: &str = "HOPRD_API_TOKEN";
pub const HOPRD_ENVIRONMENT: &str = "HOPRD_ENVIRONMENT";
pub const HOPRD_ANNOUNCE: &str = "HOPRD_ANNOUNCE";
pub const HOPRD_PROVIDER: &str = "HOPRD_PROVIDER";
pub const HOPRD_DEFAULT_STRATEGY: &str = "HOPRD_DEFAULT_STRATEGY";
pub const HOPRD_MAX_AUTOCHANNELS: &str = "HOPRD_MAX_AUTOCHANNELS";
pub const HOPRD_AUTO_REDEEM_TICKETS: &str = "HOPRD_AUTO_REDEEM_TICKETS";
pub const HOPRD_CHECK_UNREALIZED_BALANCE: &str = "HOPRD_CHECK_UNREALIZED_BALANCE";
pub const HOPRD_ALLOW_PRIVATE_NODE_CONNECTIONS: &str = "HOPRD_ALLOW_PRIVATE_NODE_CONNECTIONS";
pub const HOPRD_TEST_ANNOUNCE_LOCAL_ADDRESSES: &str = "HOPRD_TEST_ANNOUNCE_LOCAL_ADDRESSES";
pub const HOPRD_HEARTBEAT_INTERVAL: &str = "HOPRD_HEARTBEAT_INTERVAL";
pub const HOPRD_HEARTBEAT_THRESHOLD: &str = "HOPRD_HEARTBEAT_THRESHOLD";
pub const HOPRD_HEARTBEAT_VARIANCE: &str = "HOPRD_HEARTBEAT_VARIANCE";
pub const HOPRD_ON_CHAIN_CONFIRMATIONS: &str = "HOPRD_ON_CHAIN_CONFIRMATIONS";
pub const HOPRD_NETWORK_QUALITY_THRESHOLD: &str = "HOPRD_NETWORK_QUALITY_THRESHOLD";
pub const HOPRD_IDENTITY: &str = "HOPRD_IDENTITY";
pub const HOPRD_DATA: &str = "HOPRD_DATA";
pub const HOPRD_API: &str = "HOPRD_API";
pub const HOPRD_API_HOST: &str = "HOPRD_API_HOST";
pub const HOPRD_INIT: &str = "HOPRD_INIT";
pub const HOPRD_HEALTH_CHECK: &str = "HOPRD_HEALTH_CHECK";
pub const HOPRD_HEALTH_CHECK_HOST: &str = "HOPRD_HEALTH_CHECK_HOST";
pub const ENABLED: bool= true;
