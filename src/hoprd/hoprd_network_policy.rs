use crate::hoprd::hoprd_gateway::GatewayTarget;
use crate::hoprd::hoprd_resource::Hoprd;
use crate::identity_pool::identity_pool_resource::IdentityPool;
use crate::model::Error as HoprdError;
use k8s_openapi::{
    api::networking::v1::{NetworkPolicy, NetworkPolicyIngressRule, NetworkPolicyPeer, NetworkPolicyPort, NetworkPolicySpec},
    apimachinery::pkg::{
        apis::meta::v1::{LabelSelector, OwnerReference},
        util::intstr::IntOrString,
    },
};
use kube::{core::ObjectMeta, Api, ResourceExt};
use std::{collections::BTreeMap, sync::Arc};
use tracing::info;

use crate::{constants, context_data::ContextData, utils};

/// Creates the NetworkPolicy that allows the ingress traffic of a node:
/// - its API and its p2p port from the Gateway. The traffic of the Internet arrives through the routes of the Gateway,
///   so the node only sees the Envoy proxy pods as the source,
/// - its p2p port from the nodes of the same Hopr network, in any namespace.
///
/// The port is the one the operator has just allocated to the node, so the policy opens exactly it.
/// It is owned by the Hoprd, so it is removed with it.
pub async fn create_network_policy(context_data: Arc<ContextData>, hoprd: &Hoprd, p2p_port: u16, owner_references: Option<Vec<OwnerReference>>) -> Result<(), HoprdError> {
    let name = hoprd.name_any();
    let namespace = hoprd.namespace().ok_or_else(|| HoprdError::UserInputError("The Hoprd resource has no namespace".to_owned()))?;
    let gateway = GatewayTarget::for_hoprd(&context_data.config.gateway, hoprd);
    let identity_pool_api: Api<IdentityPool> = Api::namespaced(context_data.client.clone(), &namespace);
    let network = identity_pool_api.get(&hoprd.spec.identity_pool_name).await?.spec.network;
    let policy = build_network_policy(&name, &namespace, &hoprd.spec.identity_pool_name, &gateway.namespace, &network, p2p_port, owner_references);
    let api: Api<NetworkPolicy> = Api::namespaced(context_data.client.clone(), &namespace);
    utils::apply(&api, &name, &policy).await?;
    info!("NetworkPolicy {name} created successfully");
    Ok(())
}

/// Deletes the NetworkPolicy of a node
pub async fn delete_network_policy(context_data: Arc<ContextData>, name: &str, namespace: &str) -> Result<(), HoprdError> {
    let api: Api<NetworkPolicy> = Api::namespaced(context_data.client.clone(), namespace);
    utils::delete_and_wait(api, name).await?;
    Ok(())
}

fn build_network_policy(name: &str, namespace: &str, identity_pool_name: &str, gateway_namespace: &str, network: &str, p2p_port: u16, owner_references: Option<Vec<OwnerReference>>) -> NetworkPolicy {
    // The same labels the Services of the node use to select its pod
    let mut labels: BTreeMap<String, String> = utils::common_lables(identity_pool_name.to_owned(), Some(name.to_owned()), None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), identity_pool_name.to_owned());

    let node_ports = || {
        ["TCP", "UDP"].map(|protocol| NetworkPolicyPort {
            protocol: Some(protocol.to_owned()),
            port: Some(IntOrString::Int(p2p_port.into())),
            end_port: None,
        })
    };

    let mut gateway_ports = vec![NetworkPolicyPort { protocol: Some("TCP".to_owned()), port: Some(IntOrString::Int(constants::API_PORT)), end_port: None }];
    gateway_ports.extend(node_ports());
    let mut ingress = vec![NetworkPolicyIngressRule {
        from: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([("kubernetes.io/metadata.name".to_owned(), gateway_namespace.to_owned())])),
                ..LabelSelector::default()
            }),
            ..NetworkPolicyPeer::default()
        }]),
        ports: Some(gateway_ports),
    }];
    ingress.push(NetworkPolicyIngressRule {
        from: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector::default()),
            pod_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([
                    (constants::LABEL_KUBERNETES_COMPONENT.to_owned(), "node".to_owned()),
                    (constants::LABEL_NODE_NETWORK.to_owned(), network.to_owned()),
                ])),
                ..LabelSelector::default()
            }),
            ..NetworkPolicyPeer::default()
        }]),
        ports: Some(node_ports().to_vec()),
    });

    NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels.clone()),
            owner_references,
            ..ObjectMeta::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(LabelSelector { match_labels: Some(labels), ..LabelSelector::default() }),
            policy_types: Some(vec!["Ingress".to_owned()]),
            ingress: Some(ingress),
            ..NetworkPolicySpec::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_ports(rule: &NetworkPolicyIngressRule) -> Vec<(String, i32)> {
        rule.ports
            .iter()
            .flatten()
            .map(|port| {
                let number = match port.port {
                    Some(IntOrString::Int(number)) => number,
                    _ => panic!("numeric port expected"),
                };
                (port.protocol.clone().unwrap_or_default(), number)
            })
            .collect()
    }

    #[test]
    fn the_gateway_reaches_the_api_and_the_p2p_port_of_the_node() {
        let policy = build_network_policy("node-1", "ns", "pool", "hoprd-operator", "rotsee", 9010, None);
        let ingress = policy.spec.unwrap().ingress.unwrap();
        let from = ingress[0].from.as_ref().unwrap()[0].namespace_selector.as_ref().unwrap().match_labels.as_ref().unwrap();
        assert_eq!(from.get("kubernetes.io/metadata.name"), Some(&"hoprd-operator".to_owned()));
        assert_eq!(rule_ports(&ingress[0]), vec![("TCP".to_owned(), constants::API_PORT), ("TCP".to_owned(), 9010), ("UDP".to_owned(), 9010)]);
    }

    #[test]
    fn the_nodes_of_the_same_network_of_any_namespace_reach_the_p2p_port_but_not_the_api() {
        let policy = build_network_policy("node-1", "ns", "pool", "gw", "rotsee", 9010, None);
        let ingress = policy.spec.unwrap().ingress.unwrap();
        let peer = &ingress[1].from.as_ref().unwrap()[0];
        assert_eq!(peer.namespace_selector, Some(LabelSelector::default()), "any namespace");
        let labels = peer.pod_selector.as_ref().unwrap().match_labels.as_ref().unwrap();
        assert_eq!(labels.get(constants::LABEL_NODE_NETWORK), Some(&"rotsee".to_owned()));
        assert_eq!(rule_ports(&ingress[1]), vec![("TCP".to_owned(), 9010), ("UDP".to_owned(), 9010)]);
    }

    #[test]
    fn nothing_else_is_allowed_in() {
        let policy = build_network_policy("node-1", "ns", "pool", "gw", "rotsee", 9010, None);
        assert_eq!(policy.spec.unwrap().ingress.unwrap().len(), 2, "only the Gateway and the nodes of the same network");
    }

    #[test]
    fn the_policy_selects_the_pod_of_the_node_and_is_owned_by_the_hoprd() {
        let owner = OwnerReference { name: "node-1".to_owned(), ..OwnerReference::default() };
        let policy = build_network_policy("node-1", "ns", "pool", "gw", "rotsee", 9010, Some(vec![owner]));
        let selector = policy.spec.unwrap().pod_selector.unwrap().match_labels.unwrap();
        assert_eq!(selector.get(constants::LABEL_KUBERNETES_INSTANCE), Some(&"node-1".to_owned()));
        assert_eq!(selector.get(constants::LABEL_KUBERNETES_IDENTITY_POOL), Some(&"pool".to_owned()));
        assert_eq!(policy.metadata.owner_references.unwrap().len(), 1);
    }
}
