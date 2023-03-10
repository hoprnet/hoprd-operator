use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap}, fmt::{self, Display}};
use k8s_openapi::ByteString;
use crate::constants;




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
    Unsync
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
            HoprdStatusEnum::Unsync => write!(f, "Unsync")
        }
    }
}

/// Struct corresponding to the details of the secret which contains the sensitive data to run the node
#[derive(Serialize, Debug, Default, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Secret {

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

/// Struct used to fill the contents of a Secret
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
#[serde(deny_unknown_fields)]
pub struct SecretContent {
    pub identity: String,
    pub password: String,
    pub api_token: String,
    pub address: String,
    pub peer_id: String,
    pub secret_name: String
}

impl SecretContent {

    pub fn get_encoded_data(&self) -> BTreeMap<String, ByteString> {
        let mut data: BTreeMap<String, ByteString> = BTreeMap::new();
        data.insert(constants::HOPRD_IDENTITY.to_owned(), ByteString(self.identity.to_owned().into_bytes()));
        data.insert(constants::HOPRD_PASSWORD.to_owned(), ByteString(self.password.to_owned().into_bytes()));
        data.insert(constants::HOPRD_API_TOKEN.to_owned(), ByteString(self.api_token.to_owned().into_bytes()));
        data.insert(constants::HOPRD_METRICS_PASSWORD.to_owned(), ByteString("".to_owned().into_bytes()));
        return data;
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorConfig {
    pub instance: OperatorInstance,
    pub ingress: IngressConfig
}


#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Hash)]
pub struct OperatorInstance {
    pub name: String,
    pub namespace: String
}


#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Hash)]
pub struct IngressConfig {
    pub ingress_class_name: String,
    pub dns_domain: String,
    pub annotations: Option<BTreeMap<String, String>>
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

    /// The Job execution did not complete successfully
    #[error("Job Execution failed: {0}")]
    JobExecutionError(String),
}