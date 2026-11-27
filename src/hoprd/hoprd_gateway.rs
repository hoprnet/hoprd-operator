//! Exposes a hoprd node through the Gateway API.
//!
//! Every `Hoprd` gets, in its own namespace and owned by it:
//! - a cert-manager `Certificate` (one certificate per node),
//! - a `ListenerSet` attached to the Gateway with an HTTPS listener (hostname of the node) and a TCP and a UDP
//!   listener for the p2p port (the same number for both protocols),
//! - an `HTTPRoute` for the API and a `TCPRoute` and a `UDPRoute` for the p2p port.
//!
//! The Gateway is either the built-in one of the operator or an existing one chosen with `spec.gateway.ref`.
use gateway_api::{
    gateways::Gateway,
    httproutes::{HttpRoute, HttpRouteParentRefs, HttpRouteRules, HttpRouteRulesBackendRefs, HttpRouteSpec},
    listenersets::{ListenerSet, ListenerSetListeners, ListenerSetListenersTls, ListenerSetListenersTlsCertificateRefs, ListenerSetListenersTlsMode, ListenerSetParentRef, ListenerSetSpec},
    tcproutes::{TcpRoute, TcpRouteParentRefs, TcpRouteRules, TcpRouteRulesBackendRefs, TcpRouteSpec},
    udproutes::{UdpRoute, UdpRouteParentRefs, UdpRouteRules, UdpRouteRulesBackendRefs, UdpRouteSpec},
};
use k8s_openapi::{api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::OwnerReference, NamespaceResourceScope};
use kube::{
    api::ListParams,
    core::ObjectMeta,
    Api, Resource, ResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tracing::{debug, info, warn};

use crate::{
    certificate::{Certificate, CertificateIssuerRef, CertificateSpec},
    constants,
    context_data::ContextData,
    hoprd::{hoprd_resource::Hoprd, hoprd_service},
    model::Error,
    operator_config::GatewayConfig,
    utils::{self, ResourceIdentity},
};

/// Maximum number of attempts to allocate the ports when concurrent allocations conflict
const ALLOCATION_MAX_ATTEMPTS: u8 = 10;
const GATEWAY_API_GROUP: &str = "gateway.networking.k8s.io";
const HTTPS_PORT: i32 = 443;
const HTTPS_LISTENER: &str = "https";
const P2P_TCP_LISTENER: &str = "p2p-tcp";
const P2P_UDP_LISTENER: &str = "p2p-udp";

/// Gateway of the node. When it is not set, the node uses the built-in Gateway of the operator.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Default, Hash)]
#[serde(rename_all = "camelCase")]
pub struct HoprdGatewaySpec {
    /// Existing Gateway to attach the node to. Its owner must allow the ListenerSets of the namespace of the node
    /// in `spec.allowedListeners`.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub gateway_ref: Option<GatewayRef>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema, Hash)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRef {
    pub name: String,
    pub namespace: String,
}

/// Gateway a node is attached to
#[derive(Debug, PartialEq, Clone)]
pub struct GatewayTarget {
    pub name: String,
    pub namespace: String,
    /// Whether it is the built-in Gateway of the operator
    pub builtin: bool,
}

impl GatewayTarget {
    pub fn for_hoprd(config: &GatewayConfig, hoprd: &Hoprd) -> Self {
        match hoprd.spec.gateway.as_ref().and_then(|gateway| gateway.gateway_ref.as_ref()) {
            Some(gateway_ref) => Self { name: gateway_ref.name.clone(), namespace: gateway_ref.namespace.clone(), builtin: false },
            None => Self { name: config.gateway_name.clone(), namespace: config.gateway_namespace.clone(), builtin: true },
        }
    }
}

/// What the rest of the operator needs to know about the network of a node
#[derive(Debug, PartialEq, Clone)]
pub struct NodeNetwork {
    /// Public address of the Gateway the node is attached to
    pub host: String,
    /// p2p port of the node, open in the Gateway for TCP and UDP
    pub p2p_port: u16,
}

/// Public address of the Gateway. For the built-in Gateway it is the configured IP, for an existing Gateway the first
/// address of its status.
pub async fn gateway_host(context_data: &ContextData, target: &GatewayTarget) -> Result<String, Error> {
    if target.builtin {
        return Ok(context_data.config.gateway.loadbalancer_ip.clone());
    }
    let api: Api<Gateway> = Api::namespaced(context_data.client.clone(), &target.namespace);
    let gateway = api.get(&target.name).await?;
    gateway
        .status
        .and_then(|status| status.addresses)
        .and_then(|addresses| addresses.into_iter().next())
        .map(|address| address.value)
        .ok_or_else(|| Error::HoprdStatusError(format!("Gateway {}/{} has no address yet", target.namespace, target.name)))
}

/// Hostname of the node, shortened by removing the part of the name that repeats the DNS domain.
/// Example: dns=piz-palu.dev.hoprnet.link, name=piz-palu-dev-node-1, namespace=ns -> node-1.ns.piz-palu.dev.hoprnet.link
pub fn node_hostname(name: &str, namespace: &str, dns_name: &str) -> String {
    let dns_labels: Vec<&str> = dns_name.split('.').collect();
    let mut shortened_name = name.to_owned();
    // Iterate over the DNS labels in reverse order to find the longest matching prefix
    for i in (1..=dns_labels.len()).rev() {
        let prefix_with_dash = format!("{}-", dns_labels[..i].join("-"));
        if let Some(rest) = shortened_name.strip_prefix(&prefix_with_dash) {
            shortened_name = rest.to_owned();
            break;
        }
    }
    format!("{shortened_name}.{namespace}.{dns_name}")
}

/// Name of the Secret that holds the certificate of the node
pub fn tls_secret_name(name: &str) -> String {
    format!("{name}-tls")
}

/// Finds the first free port inside `min..=max`
pub fn find_free_port(used: &BTreeSet<u16>, min: u16, max: u16) -> Option<u16> {
    (min..=max).find(|port| !used.contains(port))
}

/// Ports used by the ListenerSets attached to the Gateway, and the ListenerSets themselves
async fn attached_listener_sets(context_data: &ContextData, target: &GatewayTarget) -> Result<Vec<ListenerSet>, Error> {
    let api: Api<ListenerSet> = Api::all(context_data.client.clone());
    let attached = api
        .list(&ListParams::default())
        .await?
        .items
        .into_iter()
        .filter(|listener_set| {
            let parent = &listener_set.spec.parent_ref;
            let parent_namespace = parent.namespace.clone().or_else(|| listener_set.namespace()).unwrap_or_default();
            parent.name == target.name && parent_namespace == target.namespace && parent.kind.as_deref().is_none_or(|kind| kind == "Gateway")
        })
        .collect();
    Ok(attached)
}

/// Whether `created` has to give up its p2p port because an older ListenerSet of `others` already uses it.
/// Only the p2p port can conflict: the HTTPS listener is on the same port in every node and is told apart by its hostname
fn loses_port_conflict(created: &ListenerSet, others: &[ListenerSet]) -> bool {
    let Some(created_port) = p2p_port_of(created) else { return false };
    others
        .iter()
        .filter(|other| other.namespace() != created.namespace() || other.name_any() != created.name_any())
        .any(|other| precedence(other, created) == Ordering::Less && p2p_port_of(other) == Some(created_port))
}

/// Orders two ListenerSets the way the Gateway API resolves conflicts: the oldest wins, and `namespace/name` breaks ties
fn precedence(a: &ListenerSet, b: &ListenerSet) -> Ordering {
    let key = |listener_set: &ListenerSet| (listener_set.meta().creation_timestamp.clone(), format!("{}/{}", listener_set.namespace().unwrap_or_default(), listener_set.name_any()));
    key(a).cmp(&key(b))
}

/// Recovers the p2p port of a node from its ListenerSet
fn p2p_port_of(listener_set: &ListenerSet) -> Option<u16> {
    listener_set.spec.listeners.iter().find(|listener| listener.name == P2P_TCP_LISTENER).and_then(|listener| u16::try_from(listener.port).ok())
}

fn labels_of(hoprd: &Hoprd, name: &str) -> BTreeMap<String, String> {
    let mut labels = utils::common_lables(hoprd.spec.identity_pool_name.clone(), Some(name.to_owned()), None);
    labels.insert(constants::LABEL_KUBERNETES_IDENTITY_POOL.to_owned(), hoprd.spec.identity_pool_name.clone());
    labels
}

fn metadata(name: &str, namespace: &str, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> ObjectMeta {
    ObjectMeta {
        name: Some(name.to_owned()),
        namespace: Some(namespace.to_owned()),
        labels: Some(labels.clone()),
        owner_references: owner_references.clone(),
        ..ObjectMeta::default()
    }
}

fn build_listeners(hostname: &str, tls_secret: &str, p2p_port: u16) -> Vec<ListenerSetListeners> {
    let listeners = vec![
        ListenerSetListeners {
            name: HTTPS_LISTENER.to_owned(),
            port: HTTPS_PORT,
            protocol: "HTTPS".to_owned(),
            hostname: Some(hostname.to_owned()),
            tls: Some(ListenerSetListenersTls {
                mode: Some(ListenerSetListenersTlsMode::Terminate),
                certificate_refs: Some(vec![ListenerSetListenersTlsCertificateRefs { name: tls_secret.to_owned(), ..ListenerSetListenersTlsCertificateRefs::default() }]),
                ..ListenerSetListenersTls::default()
            }),
            ..ListenerSetListeners::default()
        },
        ListenerSetListeners { name: P2P_TCP_LISTENER.to_owned(), port: p2p_port.into(), protocol: "TCP".to_owned(), ..ListenerSetListeners::default() },
        ListenerSetListeners { name: P2P_UDP_LISTENER.to_owned(), port: p2p_port.into(), protocol: "UDP".to_owned(), ..ListenerSetListeners::default() },
    ];
    listeners
}

fn build_listener_set(name: &str, namespace: &str, hostname: &str, target: &GatewayTarget, p2p_port: u16, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> ListenerSet {
    let mut listener_set = ListenerSet::new(
        name,
        ListenerSetSpec {
            parent_ref: ListenerSetParentRef {
                group: Some(GATEWAY_API_GROUP.to_owned()),
                kind: Some("Gateway".to_owned()),
                name: target.name.clone(),
                namespace: Some(target.namespace.clone()),
            },
            listeners: build_listeners(hostname, &tls_secret_name(name), p2p_port),
        },
    );
    listener_set.metadata = metadata(name, namespace, labels, owner_references);
    listener_set
}

fn build_certificate(name: &str, namespace: &str, hostname: &str, config: &GatewayConfig, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> Certificate {
    let mut certificate = Certificate::new(
        &tls_secret_name(name),
        CertificateSpec {
            secret_name: tls_secret_name(name),
            dns_names: vec![hostname.to_owned()],
            issuer_ref: CertificateIssuerRef { name: config.certificate_issuer.name.clone(), kind: config.certificate_issuer.kind.clone(), group: "cert-manager.io".to_owned() },
        },
    );
    certificate.metadata = metadata(&tls_secret_name(name), namespace, labels, owner_references);
    certificate
}

fn listener_set_parent(name: &str, section_name: &str) -> (String, String, String, String) {
    (GATEWAY_API_GROUP.to_owned(), "ListenerSet".to_owned(), name.to_owned(), section_name.to_owned())
}

fn build_http_route(name: &str, namespace: &str, hostname: &str, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> HttpRoute {
    let (group, kind, parent_name, section_name) = listener_set_parent(name, HTTPS_LISTENER);
    let mut route = HttpRoute::new(
        name,
        HttpRouteSpec {
            hostnames: Some(vec![hostname.to_owned()]),
            parent_refs: Some(vec![HttpRouteParentRefs { group: Some(group), kind: Some(kind), name: parent_name, section_name: Some(section_name), ..HttpRouteParentRefs::default() }]),
            rules: Some(vec![HttpRouteRules {
                backend_refs: Some(vec![HttpRouteRulesBackendRefs { name: hoprd_service::api_service_name(name), port: Some(constants::API_PORT), ..HttpRouteRulesBackendRefs::default() }]),
                ..HttpRouteRules::default()
            }]),
        },
    );
    route.metadata = metadata(name, namespace, labels, owner_references);
    route
}

fn build_tcp_route(name: &str, namespace: &str, listener: &str, port: u16, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> TcpRoute {
    let (group, kind, parent_name, section_name) = listener_set_parent(name, listener);
    let route_name = format!("{name}-{listener}");
    let mut route = TcpRoute::new(
        &route_name,
        TcpRouteSpec {
            parent_refs: Some(vec![TcpRouteParentRefs { group: Some(group), kind: Some(kind), name: parent_name, section_name: Some(section_name), ..TcpRouteParentRefs::default() }]),
            rules: vec![TcpRouteRules { backend_refs: vec![TcpRouteRulesBackendRefs { name: hoprd_service::p2p_service_name(name), port: Some(port.into()), ..TcpRouteRulesBackendRefs::default() }], ..TcpRouteRules::default() }],
        },
    );
    route.metadata = metadata(&route_name, namespace, labels, owner_references);
    route
}

fn build_udp_route(name: &str, namespace: &str, listener: &str, port: u16, labels: &BTreeMap<String, String>, owner_references: &Option<Vec<OwnerReference>>) -> UdpRoute {
    let (group, kind, parent_name, section_name) = listener_set_parent(name, listener);
    let route_name = format!("{name}-{listener}");
    let mut route = UdpRoute::new(
        &route_name,
        UdpRouteSpec {
            parent_refs: Some(vec![UdpRouteParentRefs { group: Some(group), kind: Some(kind), name: parent_name, section_name: Some(section_name), ..UdpRouteParentRefs::default() }]),
            rules: vec![UdpRouteRules { backend_refs: vec![UdpRouteRulesBackendRefs { name: hoprd_service::p2p_service_name(name), port: Some(port.into()), ..UdpRouteRulesBackendRefs::default() }], ..UdpRouteRules::default() }],
        },
    );
    route.metadata = metadata(&route_name, namespace, labels, owner_references);
    route
}

/// Creates the Gateway API resources of a hoprd node, allocating its p2p port in the range of the operator configuration.
pub async fn create_networking(context_data: Arc<ContextData>, hoprd: &Hoprd) -> Result<NodeNetwork, Error> {
    let name = hoprd.name_any();
    let namespace = hoprd.require_namespace()?;
    let config = &context_data.config.gateway;
    let target = GatewayTarget::for_hoprd(config, hoprd);
    let dns_name = hoprd.spec.dns_name.clone().unwrap_or_else(|| config.dns_domain.clone());
    let hostname = node_hostname(&name, &namespace, &dns_name);
    let labels = labels_of(hoprd, &name);
    let owner_references: Option<Vec<OwnerReference>> = Some(vec![hoprd.controller_owner_ref(&()).ok_or_else(|| Error::UserInputError("The Hoprd resource has no uid".to_owned()))?]);

    let certificate_api: Api<Certificate> = Api::namespaced(context_data.client.clone(), &namespace);
    utils::apply(&certificate_api, &tls_secret_name(&name), &build_certificate(&name, &namespace, &hostname, config, &labels, &owner_references)).await?;

    let listener_set_api: Api<ListenerSet> = Api::namespaced(context_data.client.clone(), &namespace);
    let p2p_port = match listener_set_api.get_opt(&name).await? {
        // A previous reconciliation already allocated the port of the node: keep it
        Some(existing) => p2p_port_of(&existing).ok_or_else(|| Error::HoprdStatusError(format!("ListenerSet {namespace}/{name} has no p2p listener")))?,
        None => allocate_and_create(&context_data, &listener_set_api, hoprd, &target, &hostname, &labels, &owner_references).await?,
    };

    apply_routes(&context_data, &name, &namespace, &hostname, p2p_port, &labels, &owner_references).await?;
    let host = gateway_host(&context_data, &target).await?;
    info!("Gateway resources of Hoprd node {name} created, exposed at {host}:{p2p_port} through Gateway {}/{}", target.namespace, target.name);
    Ok(NodeNetwork { host, p2p_port })
}

#[allow(clippy::too_many_arguments)]
async fn allocate_and_create(
    context_data: &ContextData,
    listener_set_api: &Api<ListenerSet>,
    hoprd: &Hoprd,
    target: &GatewayTarget,
    hostname: &str,
    labels: &BTreeMap<String, String>,
    owner_references: &Option<Vec<OwnerReference>>,
) -> Result<u16, Error> {
    let name = hoprd.name_any();
    let namespace = hoprd.require_namespace()?;
    let config = &context_data.config.gateway;
    for attempt in 1..=ALLOCATION_MAX_ATTEMPTS {
        let used: BTreeSet<u16> = attached_listener_sets(context_data, target).await?.iter().filter_map(p2p_port_of).collect();
        let p2p_port = find_free_port(&used, config.port_min, config.port_max)
            .ok_or_else(|| Error::HoprdConfigError(format!("No free port in the range {}..={} of Gateway {}/{}", config.port_min, config.port_max, target.namespace, target.name)))?;
        let created = utils::apply(listener_set_api, &name, &build_listener_set(&name, &namespace, hostname, target, p2p_port, labels, owner_references)).await?;
        // Two nodes created at the same time can choose the same ports: the oldest ListenerSet keeps them
        // and the other one allocates again, which is the same precedence the Gateway uses to resolve conflicts
        let lost = loses_port_conflict(&created, &attached_listener_sets(context_data, target).await?);
        if !lost {
            return Ok(p2p_port);
        }
        debug!("Conflict allocating the port of {name} (attempt {attempt}/{ALLOCATION_MAX_ATTEMPTS}), retrying");
        utils::delete_and_wait(listener_set_api.clone(), &name).await?;
    }
    Err(Error::HoprdConfigError(format!("Could not allocate the port of {name} after {ALLOCATION_MAX_ATTEMPTS} attempts due to concurrent allocations")))
}

async fn apply_routes(
    context_data: &ContextData,
    name: &str,
    namespace: &str,
    hostname: &str,
    p2p_port: u16,
    labels: &BTreeMap<String, String>,
    owner_references: &Option<Vec<OwnerReference>>,
) -> Result<(), Error> {
    let client = context_data.client.clone();
    let http_api: Api<HttpRoute> = Api::namespaced(client.clone(), namespace);
    utils::apply(&http_api, name, &build_http_route(name, namespace, hostname, labels, owner_references)).await?;
    let tcp_api: Api<TcpRoute> = Api::namespaced(client.clone(), namespace);
    let udp_api: Api<UdpRoute> = Api::namespaced(client, namespace);
    let tcp_route = build_tcp_route(name, namespace, P2P_TCP_LISTENER, p2p_port, labels, owner_references);
    utils::apply(&tcp_api, &tcp_route.name_any(), &tcp_route).await?;
    let udp_route = build_udp_route(name, namespace, P2P_UDP_LISTENER, p2p_port, labels, owner_references);
    utils::apply(&udp_api, &udp_route.name_any(), &udp_route).await?;
    Ok(())
}

/// Deletes the Gateway API resources of a node, its certificate and the Secret that holds it. Every step is idempotent.
pub async fn delete_networking(context_data: Arc<ContextData>, name: &str, namespace: &str) -> Result<(), Error> {
    let client = context_data.client.clone();
    let selector = ListParams::default().labels(&format!("{}={}", constants::LABEL_KUBERNETES_INSTANCE, name));
    delete_labelled::<HttpRoute>(&client, namespace, &selector).await?;
    delete_labelled::<TcpRoute>(&client, namespace, &selector).await?;
    delete_labelled::<UdpRoute>(&client, namespace, &selector).await?;
    utils::delete_and_wait(Api::<ListenerSet>::namespaced(client.clone(), namespace), name).await?;
    utils::delete_and_wait(Api::<Certificate>::namespaced(client.clone(), namespace), &tls_secret_name(name)).await?;
    utils::delete_and_wait(Api::<Secret>::namespaced(client, namespace), &tls_secret_name(name)).await?;
    Ok(())
}

async fn delete_labelled<K>(client: &kube::Client, namespace: &str, selector: &ListParams) -> Result<(), Error>
where
    K: Resource<DynamicType = (), Scope = NamespaceResourceScope> + Clone + std::fmt::Debug + serde::de::DeserializeOwned + Send + 'static,
{
    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    for resource in api.list(selector).await?.items {
        if utils::delete_and_wait(api.clone(), &resource.name_any()).await.is_err() {
            warn!("Could not delete {} {}", K::kind(&()), resource.name_any());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(ports: &[u16]) -> BTreeSet<u16> {
        ports.iter().copied().collect()
    }

    #[test]
    fn hostname_removes_the_repeated_dns_prefix() {
        assert_eq!(node_hostname("piz-palu-dev-node-1", "ns", "piz-palu.dev.hoprnet.link"), "node-1.ns.piz-palu.dev.hoprnet.link");
        assert_eq!(node_hostname("other-node", "ns", "example.org"), "other-node.ns.example.org");
    }

    #[test]
    fn first_free_port_is_the_minimum_when_nothing_is_used() {
        assert_eq!(find_free_port(&used(&[]), 9000, 10000), Some(9000));
    }

    #[test]
    fn free_port_skips_used_ports_and_fills_gaps() {
        assert_eq!(find_free_port(&used(&[9000, 9001, 9003]), 9000, 10000), Some(9002));
    }

    #[test]
    fn no_port_when_the_port_range_is_exhausted() {
        assert_eq!(find_free_port(&used(&[9000, 9001]), 9000, 9001), None);
        assert_eq!(find_free_port(&used(&[]), 9001, 9000), None);
        assert_eq!(find_free_port(&used(&[]), 9003, 9003), Some(9003));
    }

    #[test]
    fn listeners_hold_https_and_the_p2p_port_for_both_protocols() {
        let listeners = build_listeners("node-1.ns.example.org", "node-1-tls", 9000);
        let names: Vec<&str> = listeners.iter().map(|listener| listener.name.as_str()).collect();
        assert_eq!(names, vec!["https", "p2p-tcp", "p2p-udp"]);
        assert_eq!(listeners[0].port, 443);
        assert_eq!(listeners[0].hostname.as_deref(), Some("node-1.ns.example.org"));
        assert_eq!(listeners[1].protocol, "TCP");
        assert_eq!(listeners[2].protocol, "UDP");
        assert_eq!(listeners[1].port, listeners[2].port);
    }

    #[test]
    fn the_p2p_port_is_recovered_from_the_listener_set() {
        let listener_set = ListenerSet::new("node", ListenerSetSpec { parent_ref: ListenerSetParentRef::default(), listeners: build_listeners("h", "s", 9100) });
        assert_eq!(p2p_port_of(&listener_set), Some(9100));
    }

    fn node_listener_set(name: &str, created: &str, p2p_port: u16) -> ListenerSet {
        let mut listener_set = ListenerSet::new(name, ListenerSetSpec { parent_ref: ListenerSetParentRef::default(), listeners: build_listeners(&format!("{name}.ns.example.org"), "tls", p2p_port) });
        listener_set.metadata.namespace = Some("ns".to_owned());
        listener_set.metadata.creation_timestamp = Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(created.parse().expect("a valid timestamp")));
        listener_set
    }

    #[test]
    fn nodes_with_different_p2p_ports_do_not_conflict_even_though_they_share_the_https_port() {
        let older = node_listener_set("node-1", "2026-11-27T12:00:00Z", 9000);
        let newer = node_listener_set("node-2", "2026-11-27T12:05:00Z", 9001);
        assert!(!loses_port_conflict(&newer, &[older.clone(), newer.clone()]));
    }

    #[test]
    fn only_the_newest_node_with_the_same_p2p_port_gives_it_up() {
        let older = node_listener_set("node-1", "2026-11-27T12:00:00Z", 9000);
        let newer = node_listener_set("node-2", "2026-11-27T12:05:00Z", 9000);
        let all = [older.clone(), newer.clone()];
        assert!(loses_port_conflict(&newer, &all));
        assert!(!loses_port_conflict(&older, &all));
    }

    #[test]
    fn routes_point_to_the_listener_and_the_service() {
        let labels = BTreeMap::new();
        let route = build_udp_route("node-1", "ns", "p2p-udp", 9001, &labels, &None);
        assert_eq!(route.name_any(), "node-1-p2p-udp");
        let parent = &route.spec.parent_refs.as_ref().expect("has a parent")[0];
        assert_eq!(parent.kind.as_deref(), Some("ListenerSet"));
        assert_eq!(parent.name, "node-1");
        assert_eq!(parent.section_name.as_deref(), Some("p2p-udp"));
        assert_eq!(route.spec.rules[0].backend_refs[0].name, "node-1-p2p");
        assert_eq!(route.spec.rules[0].backend_refs[0].port, Some(9001));
        let http = build_http_route("node-1", "ns", "node-1.ns.example.org", &labels, &None);
        assert_eq!(http.spec.rules.as_ref().expect("has rules")[0].backend_refs.as_ref().expect("has backends")[0].name, "node-1-api");
    }
}
