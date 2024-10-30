use k8s_openapi::{
    api::core::v1::{EnvVar, HTTPGetAction, Probe, ResourceRequirements},
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::constants::SupportedReleaseEnum;

#[derive(Serialize, Deserialize, Debug)]
struct CustomEnvVar {
    name: String,
    value: String,
}

impl CustomEnvVar {
    pub fn new(name: String, value: String) -> Self {
        Self { name, value }
    }
}

/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdDeploymentSpec {
    env: Option<String>,
    resources: Option<String>,
    startup_probe: Option<String>,
    liveness_probe: Option<String>,
    readiness_probe: Option<String>,
}

impl Default for HoprdDeploymentSpec {
    fn default() -> Self {
        let mut limits: BTreeMap<String, Quantity> = BTreeMap::new();
        let mut requests: BTreeMap<String, Quantity> = BTreeMap::new();
        limits.insert("cpu".to_owned(), Quantity("1500m".to_owned()));
        limits.insert("memory".to_owned(), Quantity("2Gi".to_owned()));
        requests.insert("cpu".to_owned(), Quantity("750m".to_owned()));
        requests.insert("memory".to_owned(), Quantity("512Mi".to_owned()));
        let resources_spec = serde_yaml::to_string(&ResourceRequirements {
            requests: Some(requests),
            limits: Some(limits),
        })
        .unwrap();

        let default_probe = HoprdDeploymentSpec::build_probe("some/path".to_string(), Some(5), Some(1), Some(10));
        let default_probe_string = Some(serde_yaml::to_string(&default_probe).unwrap());

        let default_env = vec![
            CustomEnvVar::new("RUST_BACKTRACE".to_owned(), "full".to_owned()),
            CustomEnvVar::new("RUST_LOG".to_owned(), "info".to_owned()),
            CustomEnvVar::new("HOPRD_LOG_FORMAT".to_owned(), "json".to_owned()),
            CustomEnvVar::new("DEBUG".to_owned(), "hopr*".to_owned()),
        ];
        let default_env_string = Some(serde_yaml::to_string(&default_env).unwrap());

        Self {
            resources: Some(resources_spec),
            startup_probe: default_probe_string.clone(),
            liveness_probe: default_probe_string.clone(),
            readiness_probe: default_probe_string.clone(),
            env: default_env_string,
        }
    }
}

impl HoprdDeploymentSpec {
    pub fn get_resource_requirements(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> ResourceRequirements {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let resource_requirements_string = hoprd_deployment_spec.resources.as_ref().unwrap_or(default_deployment_spec.resources.as_ref().unwrap());
        let resource_requirements: ResourceRequirements = serde_yaml::from_str(resource_requirements_string).unwrap();
        resource_requirements
    }

    pub fn get_environment_variables(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Vec<EnvVar> {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let environment_variables_string = hoprd_deployment_spec.env.as_ref().unwrap_or(default_deployment_spec.env.as_ref().unwrap());
        let environment_variables: Vec<CustomEnvVar> = serde_yaml::from_str(environment_variables_string).unwrap();
        environment_variables
            .iter()
            .map(|env| EnvVar {
                name: env.name.to_owned(),
                value: Some(env.value.to_owned()),
                ..EnvVar::default()
            })
            .collect()
    }

    pub fn build_probe(path: String, period_seconds: Option<i32>, success_threshold: Option<i32>, failure_threshold: Option<i32>) -> Probe {
        Probe {
            http_get: Some(HTTPGetAction {
                path: Some(path.to_string()),
                port: IntOrString::Int(3001),
                ..HTTPGetAction::default()
            }),
            timeout_seconds: Some(5),
            period_seconds,
            success_threshold,
            failure_threshold,
            ..Probe::default()
        }
    }

    pub fn get_liveness_probe(supported_release: SupportedReleaseEnum, hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>) -> Option<Probe> {
        match supported_release {
            SupportedReleaseEnum::Providence => None,
            SupportedReleaseEnum::SaintLouis => {
                let default_liveness_probe = HoprdDeploymentSpec::build_probe("/healthyz".to_owned(), Some(5), Some(1), Some(3));
                if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
                    if let Some(liveness_probe_string) = hoprd_deployment_spec.liveness_probe {
                        Some(serde_yaml::from_str(&liveness_probe_string).unwrap())
                    } else {
                        Some(default_liveness_probe)
                    }
                } else {
                    Some(default_liveness_probe)
                }
            }
        }
    }

    pub fn get_startup_probe(supported_release: SupportedReleaseEnum, hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>) -> Option<Probe> {
        match supported_release {
            SupportedReleaseEnum::Providence => None,
            SupportedReleaseEnum::SaintLouis => {
                let default_startup_probe = HoprdDeploymentSpec::build_probe("/startedz".to_owned(), Some(15), Some(1), Some(8));
                if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
                    if let Some(startup_probe_string) = hoprd_deployment_spec.startup_probe {
                        Some(serde_yaml::from_str(&startup_probe_string).unwrap())
                    } else {
                        Some(default_startup_probe)
                    }
                } else {
                    Some(default_startup_probe)
                }
            }
        }
    }

    pub fn get_readiness_probe(supported_release: SupportedReleaseEnum, hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>) -> Option<Probe> {
        match supported_release {
            SupportedReleaseEnum::Providence => None,
            SupportedReleaseEnum::SaintLouis => {
                let default_readiness_probe = HoprdDeploymentSpec::build_probe("/readyz".to_owned(), Some(10), Some(1), Some(6));
                if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
                    if let Some(readiness_probe_string) = hoprd_deployment_spec.readiness_probe {
                        Some(serde_yaml::from_str(&readiness_probe_string).unwrap())
                    } else {
                        Some(default_readiness_probe)
                    }
                } else {
                    Some(default_readiness_probe)
                }
            }
        }
    }
}
#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
pub struct EnablingFlag {
    pub enabled: bool,
}
