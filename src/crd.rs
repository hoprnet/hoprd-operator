use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap}};
use k8s_openapi::ByteString;

use crate::constants;

/// Struct corresponding to the Specification (`spec`) part of the `Hoprd` resource, directly
/// reflects context of the `hoprds.hoprnet.org.yaml` file to be found in this repository.
/// The `Hoprd` struct will be generated by the `CustomResource` derive macro.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "hoprnet.org",
    version = "v1alpha",
    kind = "Hoprd",
    plural = "hoprds",
    derive = "PartialEq",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct HoprdSpec {
    pub environment_name: String,
    pub environment_type: String,
    pub version: String,
    pub secret: Option<Secret>,
    pub monitoring: Option<Monitoring>,
    pub resources: Option<Resource>,
    pub announce: Option<bool>,
    pub provider: Option<String>,
    pub default_strategy: Option<String>,
    pub max_auto_channels: Option<i32>,
    pub auto_redeem_tickets: Option<bool>,
    pub check_unrealized_balance: Option<bool>,
    pub allow_private_node_connections: Option<bool>,
    pub test_announce_local_address: Option<bool>,
    pub heartbeat_interval: Option<i32>,
    pub heartbeat_threshold: Option<i32>,
    pub heartbeat_variance: Option<i32>,
    pub on_chain_confirmations: Option<i32>,
    pub network_quality_threshold: Option<f32>
}

/// Struct corresponding to the details of the secret which contains the sensitive data to run the node
#[derive(Serialize, Debug, Default, Deserialize,  PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Secret {

    pub secret_name: String,

    pub password_ref_key: Option<String>,

    pub api_token_ref_key: Option<String>,

    pub identity_ref_key: Option<String>,

    pub metrics_password_ref_key: Option<String>
}

/// Struct to map Pod resources
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema)]
pub struct Resource {
    pub limits: ResourceTypes,
    pub requests: ResourceTypes
}

/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema)]
pub struct ResourceTypes {
    pub cpu: String,
    pub memory: String
}


/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema)]
pub struct Monitoring {
    pub enabled: bool
}


/// Struct used to fill the contents of a Secret
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema)]
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

