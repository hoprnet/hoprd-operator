use k8s_openapi::{api::core::v1::{ResourceRequirements, Probe, HTTPGetAction}, apimachinery::pkg::{api::resource::Quantity, util::intstr::IntOrString}};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap};


/// Struct to define Pod resources types
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
pub struct HoprdDeploymentSpec {
    resources: Option<String>,
    #[serde(rename(deserialize = "startupProbe"))]
    startup_probe: Option<String>,
    #[serde(rename(deserialize = "livenessProbe"))]
    liveness_probe: Option<String>,
    #[serde(rename(deserialize = "readinessProbe"))]
    readiness_probe: Option<String>,
}

impl Default for HoprdDeploymentSpec {
    fn default() -> Self {

            let mut limits: BTreeMap<String, Quantity> = BTreeMap::new();
            let mut requests: BTreeMap<String, Quantity> = BTreeMap::new();
            limits.insert("cpu".to_owned(), Quantity("1500m".to_owned()));
            limits.insert("memory".to_owned(), Quantity("2Gi".to_owned()));
            requests.insert("cpu".to_owned(), Quantity("750m".to_owned()));
            requests.insert("memory".to_owned(), Quantity("256Mi".to_owned()));
            let resources_spec = serde_yaml::to_string(&ResourceRequirements {
                requests: Some(requests),
                limits: Some(limits)

            }).unwrap();

            let default_probe = Probe {
                    http_get: Some(HTTPGetAction {
                        path: Some("/healthcheck/v1/version".to_owned()),
                        port: IntOrString::Int(8080),
                        ..HTTPGetAction::default()
                    }),
                    failure_threshold: Some(6),
                    initial_delay_seconds: Some(30),
                    period_seconds: Some(20),
                    success_threshold: Some(1),
                    timeout_seconds: Some(5),
                    ..Probe::default()
                };
            let default_probe_string = Some(serde_yaml::to_string(&default_probe).unwrap());

        Self { resources: Some(resources_spec), startup_probe: default_probe_string.clone(), liveness_probe: default_probe_string.clone(), readiness_probe: default_probe_string.clone() }
    }
}


impl HoprdDeploymentSpec {

    pub fn get_resource_requirements(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> ResourceRequirements {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let resource_requirements_string = hoprd_deployment_spec.resources.as_ref().unwrap_or(&default_deployment_spec.resources.as_ref().unwrap());
        let resource_requirements: ResourceRequirements = serde_yaml::from_str(resource_requirements_string).unwrap();
        resource_requirements
    }

    pub fn get_liveness_probe(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Probe {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let liveness_probe_string = hoprd_deployment_spec.liveness_probe.as_ref().unwrap_or(&default_deployment_spec.liveness_probe.as_ref().unwrap());
        let liveness_probe: Probe = serde_yaml::from_str(liveness_probe_string).unwrap();
        liveness_probe
    }

    pub fn get_startup_probe(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Probe {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let startup_probe_string = hoprd_deployment_spec.startup_probe.as_ref().unwrap_or(&default_deployment_spec.startup_probe.as_ref().unwrap());
        let startup_probe: Probe = serde_yaml::from_str(startup_probe_string).unwrap();
        startup_probe
    }

    pub fn get_readiness_probe(hoprd_deployment_spec: Option<HoprdDeploymentSpec>) -> Probe {
        let default_deployment_spec = HoprdDeploymentSpec::default();
        let hoprd_deployment_spec = hoprd_deployment_spec.unwrap_or(default_deployment_spec.clone());
        let readiness_probe_string = hoprd_deployment_spec.readiness_probe.as_ref().unwrap_or(&default_deployment_spec.readiness_probe.as_ref().unwrap());
        let readiness_probe: Probe = serde_yaml::from_str(readiness_probe_string).unwrap();
        readiness_probe
    }

}
#[derive(Serialize, Debug, Deserialize,  PartialEq, Clone, JsonSchema, Hash)]
pub struct EnablingFlag {
    pub enabled: bool
}
