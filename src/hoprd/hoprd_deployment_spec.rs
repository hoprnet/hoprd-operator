use k8s_openapi::{
    api::core::v1::{EnvVar, EnvVarSource, HTTPGetAction, Probe, ResourceRequirements, SecretKeySelector},
    apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::trace;
use std::collections::BTreeMap;

use crate::model::Error;

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
                name: self.secret_key_ref.name.to_owned(),
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
        env_var
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
    pub labels: Option<BTreeMap<String, String>>,
    pub annotations: Option<BTreeMap<String, String>>,
}

impl Default for HoprdDeploymentSpec {
    fn default() -> Self {
        let mut limits: BTreeMap<String, Quantity> = BTreeMap::new();
        let mut requests: BTreeMap<String, Quantity> = BTreeMap::new();
        limits.insert("cpu".to_owned(), Quantity("1500m".to_owned()));
        limits.insert("memory".to_owned(), Quantity("2Gi".to_owned()));
        requests.insert("cpu".to_owned(), Quantity("750m".to_owned()));
        requests.insert("memory".to_owned(), Quantity("512Mi".to_owned()));
        let resources_spec = serde_yml::to_string(&ResourceRequirements {
            requests: Some(requests),
            limits: Some(limits),
            ..Default::default()
        })
        // Serializing these static values cannot fail; if it ever did, the empty default is reported as a parse error by the getters instead of a panic
        .unwrap_or_default();

        let default_probe = HoprdDeploymentSpec::build_probe("some/path".to_string(), Some(5), Some(1), Some(10));
        let default_probe_string = Some(serde_yml::to_string(&default_probe).unwrap_or_default());

        let default_env = vec![
            CustomEnvVar::new_value("RUST_BACKTRACE".to_owned(), "full".to_owned()),
            CustomEnvVar::new_value("RUST_LOG".to_owned(), "info".to_owned()),
            CustomEnvVar::new_value("HOPRD_LOG_FORMAT".to_owned(), "json".to_owned()),
        ];
        let default_env_string = Some(serde_yml::to_string(&default_env).unwrap_or_default());

        Self {
            resources: Some(resources_spec),
            startup_probe: default_probe_string.clone(),
            liveness_probe: default_probe_string.clone(),
            readiness_probe: default_probe_string.clone(),
            env: default_env_string,
            extra_containers: None,
            labels: None,
            annotations: None,
        }
    }
}

impl HoprdDeploymentSpec {
    pub fn get_labels(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> BTreeMap<String, String> {
        hoprd_deployment_spec.and_then(|spec| spec.labels).unwrap_or_default()
    }

    pub fn get_annotations(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> BTreeMap<String, String> {
        hoprd_deployment_spec.and_then(|spec| spec.annotations).unwrap_or_default()
    }

    pub fn get_resource_requirements(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Result<ResourceRequirements, Error> {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let resource_requirements_string = hoprd_deployment_spec.resources.or(default_deployment_spec.resources).unwrap_or_default();
        let resource_requirements: ResourceRequirements = serde_yml::from_str(&resource_requirements_string)?;
        Ok(resource_requirements)
    }

    pub fn get_environment_variables(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Result<Vec<EnvVar>, Error> {
        // Get default env vars
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let default_environment_variables: Vec<CustomEnvVar> = serde_yml::from_str(default_deployment_spec.env.as_deref().unwrap_or_default())?;
        trace!("Default environment variables: {:?}", default_environment_variables);
        // Get custom env vars
        let custom_environment_variables_string = hoprd_deployment_spec.env.or(default_deployment_spec.env).unwrap_or_default();
        trace!("Custom environment variables string: {}", custom_environment_variables_string);
        let custom_environment_variables: Vec<CustomEnvVar> = serde_yml::from_str(&custom_environment_variables_string)?;

        // Merge default and custom env vars, giving precedence to custom ones in case of name conflicts
        let env_vars: Vec<EnvVar> = default_environment_variables.iter().filter(| &default_environment_variable| {
            !custom_environment_variables.iter().any(|custom_environment_variable| custom_environment_variable.name == default_environment_variable.name)
        }).chain(custom_environment_variables.iter()).map(|env| env.to_env_var()).collect();
        Ok(env_vars)
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

    pub fn get_liveness_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>) -> Result<Option<Probe>, Error> {
        let default_liveness_probe = HoprdDeploymentSpec::build_probe("/healthyz".to_owned(), Some(5), Some(1), Some(3));
        if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
            if let Some(liveness_probe_string) = hoprd_deployment_spec.liveness_probe {
                Ok(Some(serde_yml::from_str(&liveness_probe_string)?))
            } else {
                Ok(Some(default_liveness_probe))
            }
        } else {
            Ok(Some(default_liveness_probe))
        }
    }

    pub fn get_startup_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>, source_node_logs: bool) -> Result<Option<Probe>, Error> {
        let period_seconds = if source_node_logs { Some(60) } else { Some(15) };
        let default_startup_probe = HoprdDeploymentSpec::build_probe("/startedz".to_owned(), period_seconds, Some(1), Some(60));
        if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
            if let Some(startup_probe_string) = hoprd_deployment_spec.startup_probe {
                Ok(Some(serde_yml::from_str(&startup_probe_string)?))
            } else {
                Ok(Some(default_startup_probe))
            }
        } else {
            Ok(Some(default_startup_probe))
        }
    }

    pub fn get_readiness_probe(hoprd_deployment_spec_option: Option<HoprdDeploymentSpec>, source_node_logs: bool) -> Result<Option<Probe>, Error> {
        let period_seconds = if source_node_logs { Some(60) } else { Some(15) };
        let default_readiness_probe = HoprdDeploymentSpec::build_probe("/readyz".to_owned(), period_seconds, Some(1), Some(60));
        if let Some(hoprd_deployment_spec) = hoprd_deployment_spec_option {
            if let Some(readiness_probe_string) = hoprd_deployment_spec.readiness_probe {
                Ok(Some(serde_yml::from_str(&readiness_probe_string)?))
            } else {
                Ok(Some(default_readiness_probe))
            }
        } else {
            Ok(Some(default_readiness_probe))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> HoprdDeploymentSpec {
        HoprdDeploymentSpec::default()
    }

    #[test]
    fn labels_and_annotations_default_to_empty() {
        assert!(HoprdDeploymentSpec::get_labels(None).is_empty());
        assert!(HoprdDeploymentSpec::get_annotations(None).is_empty());
        let mut with_labels = spec();
        with_labels.labels = Some(BTreeMap::from([("team".to_owned(), "infra".to_owned())]));
        assert_eq!(HoprdDeploymentSpec::get_labels(Some(with_labels)).get("team"), Some(&"infra".to_owned()));
    }

    #[test]
    fn default_resources_are_used_when_not_customized() {
        let resources = HoprdDeploymentSpec::get_resource_requirements(None).expect("defaults are valid");
        let limits = resources.limits.expect("default limits");
        assert_eq!(limits.get("cpu"), Some(&Quantity("1500m".to_owned())));
        assert_eq!(limits.get("memory"), Some(&Quantity("2Gi".to_owned())));
    }

    #[test]
    fn invalid_resources_yaml_is_an_error_not_a_panic() {
        let mut invalid = spec();
        invalid.resources = Some("limits: [not, a, map".to_owned());
        assert!(HoprdDeploymentSpec::get_resource_requirements(Some(invalid)).is_err());
    }

    #[test]
    fn default_environment_variables_are_returned() {
        let env = HoprdDeploymentSpec::get_environment_variables(None).expect("defaults are valid");
        let value_of = |name: &str| env.iter().find(|var| var.name == name).and_then(|var| var.value.clone());
        assert_eq!(value_of("RUST_LOG"), Some("info".to_owned()));
        assert_eq!(value_of("HOPRD_LOG_FORMAT"), Some("json".to_owned()));
    }

    #[test]
    fn custom_environment_variables_override_the_defaults_by_name() {
        let mut custom = spec();
        custom.env = Some("- name: RUST_LOG\n  value: debug\n- name: EXTRA\n  value: \"1\"\n".to_owned());
        let env = HoprdDeploymentSpec::get_environment_variables(Some(custom)).expect("custom env is valid");
        let values: Vec<_> = env.iter().filter(|var| var.name == "RUST_LOG").collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, Some("debug".to_owned()));
        assert!(env.iter().any(|var| var.name == "EXTRA"));
        assert!(env.iter().any(|var| var.name == "RUST_BACKTRACE"));
    }

    #[test]
    fn environment_variable_from_a_secret_is_supported() {
        let mut custom = spec();
        custom.env = Some("- name: TOKEN\n  value_from:\n    secret_key_ref:\n      key: k\n      name: s\n".to_owned());
        let env = HoprdDeploymentSpec::get_environment_variables(Some(custom)).expect("custom env is valid");
        let token = env.iter().find(|var| var.name == "TOKEN").expect("TOKEN is present");
        let selector = token.value_from.as_ref().and_then(|source| source.secret_key_ref.as_ref()).expect("secret ref");
        assert_eq!((selector.key.as_str(), selector.name.as_str()), ("k", "s"));
    }

    #[test]
    fn invalid_environment_yaml_is_an_error() {
        let mut invalid = spec();
        invalid.env = Some("{{not yaml".to_owned());
        assert!(HoprdDeploymentSpec::get_environment_variables(Some(invalid)).is_err());
    }

    #[test]
    fn default_probes_use_the_expected_paths_and_periods() {
        let liveness = HoprdDeploymentSpec::get_liveness_probe(None).expect("ok").expect("probe");
        assert_eq!(liveness.http_get.and_then(|get| get.path), Some("/healthyz".to_owned()));
        let startup = HoprdDeploymentSpec::get_startup_probe(None, false).expect("ok").expect("probe");
        assert_eq!(startup.period_seconds, Some(15));
        let readiness = HoprdDeploymentSpec::get_readiness_probe(None, true).expect("ok").expect("probe");
        assert_eq!(readiness.period_seconds, Some(60));
    }

    #[test]
    fn custom_probe_replaces_the_default_and_invalid_probe_fails() {
        let mut custom = spec();
        custom.liveness_probe = Some("periodSeconds: 99\n".to_owned());
        let probe = HoprdDeploymentSpec::get_liveness_probe(Some(custom)).expect("ok").expect("probe");
        assert_eq!(probe.period_seconds, Some(99));

        let mut invalid = spec();
        invalid.startup_probe = Some("periodSeconds: [".to_owned());
        assert!(HoprdDeploymentSpec::get_startup_probe(Some(invalid), false).is_err());
    }
}
