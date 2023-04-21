use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt::{self, Display}};


#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum HoprdStatusEnum {
    // The node is not yet created
    Initializing,
    // The node repo is being created
    Creating,
    // The node is not registered
    RegisteringInNetwork,
    /// The node is not funded
    Funding,
    /// The node is stopped
    Stopped,
    /// The node is running
    Running,
    /// The node is reconfigured
    Reloading,
    /// The node is being deleted
    Deleting,
    /// The node is deleted
    Deleted,
    /// The node is not sync
    OutOfSync
}

impl Display for HoprdStatusEnum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HoprdStatusEnum::Initializing => write!(f, "Initializing"),
            HoprdStatusEnum::Creating => write!(f, "Creating"),
            HoprdStatusEnum::RegisteringInNetwork => write!(f, "RegisteringInNetwork"),
            HoprdStatusEnum::Funding => write!(f, "Funding"),
            HoprdStatusEnum::Stopped => write!(f, "Stopped"),
            HoprdStatusEnum::Running => write!(f, "Running"),
            HoprdStatusEnum::Reloading => write!(f, "Reloading"),
            HoprdStatusEnum::Deleting => write!(f, "Deleting"),
            HoprdStatusEnum::Deleted => write!(f, "Deleted"),
            HoprdStatusEnum::OutOfSync => write!(f, "OutOfSync")
        }
    }
}


#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Copy)]
pub enum ClusterHoprdStatusEnum {
    // The HoprdCluster is initializing the nodes
    Initializing,
    // The HoprdCluster is synching with its nodes
    Synching,
    // The HoprdCluster is synchronized with its nodes
    InSync,
    /// The HoprdCluster is being deleted
    Deleting,
    /// The HoprdCluster is not synchronized with its nodes
    OutOfSync
}

impl Display for ClusterHoprdStatusEnum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ClusterHoprdStatusEnum::Initializing => write!(f, "Initializing"),
            ClusterHoprdStatusEnum::Synching => write!(f, "Synching"),
            ClusterHoprdStatusEnum::InSync => write!(f, "InSync"),
            ClusterHoprdStatusEnum::Deleting => write!(f, "Deleting"),
            ClusterHoprdStatusEnum::OutOfSync => write!(f, "OutOfSync")
        }
    }
}

/// Struct corresponding to the details of the secret which contains the sensitive data to run the node
#[derive(Serialize, Debug, Default, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdSecret {

    pub secret_name: String,

    pub password_ref_key: Option<String>,

    pub api_token_ref_key: Option<String>,

    pub identity_ref_key: Option<String>,

    pub metrics_password_ref_key: Option<String>
}

/// Struct to map Pod resources
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
pub struct DeploymentResource {
    pub limits: ResourceTypes,
    pub requests: ResourceTypes
}

/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
pub struct ResourceTypes {
    pub cpu: String,
    pub memory: String
}

#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
pub struct EnablingFlag {
    pub enabled: bool
}


/// All errors possible to occur during reconciliation
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    },
    /// Error in user input or Hoprd resource definition, typically missing fields.
    #[error("Invalid Hoprd CRD: {0}")]
    UserInputError(String),

    /// The secret is in an Unknown status
    #[error("Invalid Hoprd Secret status: {0}")]
    SecretStatusError(String),

    /// The hoprd is in an Unknown status
    #[error("Invalid Hoprd status: {0}")]
    HoprdStatusError(String),

    /// The hoprd configuration is invalid
    #[error("Invalid Hoprd configuration: {0}")]
    HoprdConfigError(String),

    /// The hoprd configuration is invalid
    #[error("ClusterHoprd synch error: {0}")]
    ClusterHoprdSynchError(String),

    /// The Job execution did not complete successfully
    #[error("Job Execution failed: {0}")]
    JobExecutionError(String),
}