use k8s_openapi::{
    api::core::v1::{EnvVar, EnvVarSource, HTTPGetAction, Probe, ResourceRequirements, SecretKeySelector},
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CustomKeyRef {
    key: String,
    name: String
}   

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CustomValueFrom {
    secret_key_ref: CustomKeyRef,
}

impl CustomValueFrom {

    pub fn to_env_var_source(&self) -> EnvVarSource {
        EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                key: self.secret_key_ref.key.to_owned(),
                name: Some(self.secret_key_ref.name.to_owned()),
                ..SecretKeySelector::default()

            }),
            ..EnvVarSource::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct CustomEnvVar {
    name: String,
    value: Option<String>,
    value_from: Option<CustomValueFrom>,
}

impl CustomEnvVar {
    pub fn new_value(name: String, value: String) -> Self {
        Self { name, value: Some(value), value_from: None }
    }

    pub fn to_env_var(&self) -> EnvVar {
        let mut env_var = EnvVar {
            name: self.name.to_owned(),
            ..EnvVar::default()
        };
        if let Some(ref value) = self.value {
            env_var.value = Some(value.clone());
        }
        if let Some(ref value_from) = self.value_from {
            env_var.value_from = Some(value_from.to_env_var_source());
        }
        return env_var;
    }
}

/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdDeploymentSpec {
    pub env: Option<String>,
    pub resources: Option<String>,
    pub startup_probe: Option<String>,
    pub liveness_probe: Option<String>,
    pub readiness_probe: Option<String>,
    pub extra_containers: Option<String>,
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
            CustomEnvVar::new_value("RUST_BACKTRACE".to_owned(), "full".to_owned()),
            CustomEnvVar::new_value("RUST_LOG".to_owned(), "info".to_owned()),
            CustomEnvVar::new_value("HOPRD_LOG_FORMAT".to_owned(), "json".to_owned()),
        ];
        let default_env_string = Some(serde_yaml::to_string(&default_env).unwrap());

        Self {
            resources: Some(resources_spec),
            startup_probe: default_probe_string.clone(),
            liveness_probe: default_probe_string.clone(),
            readiness_probe: default_probe_string.clone(),
            env: default_env_string,
            extra_containers: None,
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
        // Get default env vars
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let default_environment_variables: Vec<CustomEnvVar> = serde_yaml::from_str(default_deployment_spec.env.as_ref().unwrap()).unwrap();
        
        // Get custom env vars
        let custom_environment_variables_string = hoprd_deployment_spec.env.as_ref().unwrap_or(default_deployment_spec.env.as_ref().unwrap());
        let custom_environment_variables: Vec<CustomEnvVar> = serde_yaml::from_str(custom_environment_variables_string).unwrap();

        // Merge default and custom env vars, giving precedence to custom ones in case of name conflicts
        default_environment_variables.iter().filter(| &default_environment_variable| {
            !custom_environment_variables.iter().any(|custom_environment_variable| custom_environment_variable.name == default_environment_variable.name)
        }).chain(custom_environment_variables.iter()).map(|env| env.to_env_var()).collect()

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

    pub fn get_liveness_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>) -> Option<Probe> {
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

    pub fn get_startup_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>, source_node_logs: bool) -> Option<Probe> {
        let period_seconds = if source_node_logs { Some(60) } else { Some(15) };
        let default_startup_probe = HoprdDeploymentSpec::build_probe("/startedz".to_owned(), period_seconds, Some(1), Some(60));
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

    pub fn get_readiness_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>, source_node_logs: bool) -> Option<Probe> {
        let period_seconds = if source_node_logs { Some(60) } else { Some(15) };
        let default_readiness_probe = HoprdDeploymentSpec::build_probe("/readyz".to_owned(), period_seconds, Some(1), Some(60));
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
#[derive(Serialize, Debug, Deserialize, PartialEq, Clone, JsonSchema, Hash)]
pub struct EnablingFlag {
    pub enabled: bool,
}
