use axum::{Router, routing::post, Json};
use axum_server::{tls_rustls::{RustlsConfig, bind_rustls}};
use rustls::{ServerConfig, pki_types::{CertificateDer, PrivateKeyDer}};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::{env, io::BufReader, net::SocketAddr};
use rustls::crypto::ring::default_provider;
use tracing::{info};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::operator_config::WebhookConfig;

#[derive(Serialize, Deserialize, Debug)]
struct ConversionReview {
    request: Option<ConversionRequest>,
    response: Option<ConversionResponse>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ConversionRequest {
    uid: String,
    desired_apiversion: String,
    objects: Vec<Value>, // raw JSON objects
}

#[derive(Serialize, Deserialize, Debug)]
struct ConversionResponse {
    uid: String,
    converted_objects: Vec<Value>,
    result: Status,
}

#[derive(Serialize, Deserialize, Debug)]
struct Status {
    status: String,
    message: Option<String>,
}

pub async fn wait_for_webhook_ready() -> Result<(), String> {
    use tokio::net::TcpStream;
    use tokio::time::{sleep, Duration};

    let addr = "0.0.0.0:8443";

    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            info!("Webhook is ready at {}", addr);
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(format!("Webhook did not start on {}", addr))
}

fn load_rustls_config(cert_path: &str, key_path: &str) -> anyhow::Result<ServerConfig> {
    // Install the ring crypto provider globally
    default_provider().install_default().expect("Install ring provider");

    // Load certificate chain
    let dir = env::current_dir().as_ref().unwrap().to_str().unwrap().to_owned();
    let cert_path = format!("{}/{}", dir.clone(), cert_path);
    let cert_file = std::fs::File::open(&cert_path).expect(format!("Could not open cert file: {}", cert_path).as_str());
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain = certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(CertificateDer::from)
        .collect::<Vec<_>>();

    // Load private key
    let key_path = format!("{}/{}", dir.clone(), key_path);
    let key_file = std::fs::File::open(&key_path).expect(format!("Could not open key file: {}", key_path).as_str());
    let mut key_reader = BufReader::new(key_file);
    let mut keys_raw = pkcs8_private_keys(&mut key_reader)
        .collect::<Result<Vec<_>, _>>()?;
    if keys_raw.is_empty() {
        anyhow::bail!("No private keys found in {}", key_path);
    }
    let key = PrivateKeyDer::Pkcs8(keys_raw.remove(0).into());

    // Build server config (no client auth)
    let mut cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(cfg)
}


pub async fn run_webhook_server(webhook_config:WebhookConfig) {
    // Define Axum app with routes
    let app = Router::new().route("/convert", post(convert));
    let addr = SocketAddr::from(([0, 0, 0, 0], 8443));

    info!("Starting webhook server with TLS");
    let server_config: ServerConfig = load_rustls_config(webhook_config.crt_file.as_str(), webhook_config.key_file.as_str()).expect("Invalid TLS");
    let tls_config = RustlsConfig::from_config(server_config.into());
    bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
        .unwrap();

    info!("hoprd-operator conversion webhook listening on {}", addr);
}

async fn convert_cluster_hoprd_v2_to_v3(resource: &mut Value) {
    // v1alpha2 -> v1alpha3
    if let Some(spec) = resource.get_mut("spec") {
        // Remove spec.forceIdentityName
        if let Some(_) = spec.get("forceIdentityName").cloned() {
            spec.as_object_mut().unwrap().remove("forceIdentityName");
        }
        // Remove spec.supportedRelease
        if let Some(_) = spec.get("supportedRelease").cloned() {
            spec.as_object_mut().unwrap().remove("supportedRelease");
        }
        // Move spec.portsAllocation to spec.service.portsAllocation
        if let Some(ports_allocation_value) = spec.get("portsAllocation").cloned() {
            // Remove old required field
            spec.as_object_mut().unwrap().remove("portsAllocation");
            // Add new nested field
            spec.as_object_mut().unwrap().insert(
                "service".to_string(),
                serde_json::json!({
                    "portsAllocation": ports_allocation_value
                }),
            );
        }
    }
}

async fn convert_cluster_hoprd_v3_to_v2(resource: &mut Value) {
    // v1alpha3 -> v1alpha2
    if let Some(spec) = resource.get_mut("spec").and_then(|s| s.as_object_mut()) {
        if let Some(service) = spec.get_mut("service").and_then(|p| p.as_object_mut()) {
            if let Some(ports_allocation_value) = service.remove("portsAllocation") {
                spec.insert("portsAllocation".to_string(), ports_allocation_value);
            }
        }
        spec.insert("forceIdentityName".to_string(), Value::Bool(true));
        spec.insert("supportedRelease".to_string(), Value::String("kaunas".to_string()));
    }
}

async fn convert_hoprd_v2_to_v3(resource: &mut Value) {
    // v1alpha2 -> v1alpha3
    if let Some(spec) = resource.get_mut("spec") {
        // Remove spec.supportedRelease
        if let Some(_) = spec.get("supportedRelease").cloned() {
            spec.as_object_mut().unwrap().remove("supportedRelease");
        }
        // Move spec.portsAllocation to spec.service.portsAllocation
        if let Some(ports_allocation_value) = spec.get("portsAllocation").cloned() {
            // Remove old required field
            spec.as_object_mut().unwrap().remove("portsAllocation");
            // Add new nested field
            spec.as_object_mut().unwrap().insert(
                "service".to_string(),
                serde_json::json!({
                    "portsAllocation": ports_allocation_value
                }),
            );
        }
    }
}

async fn convert_hoprd_v3_to_v2(resource: &mut Value) {
    if let Some(spec) = resource.get_mut("spec").and_then(|s| s.as_object_mut()) {
        if let Some(service) = spec.get_mut("service").and_then(|p| p.as_object_mut()) {
            if let Some(ports_allocation_value) = service.remove("portsAllocation") {
                spec.insert("portsAllocation".to_string(), ports_allocation_value);
            }
        }
        spec.insert("supportedRelease".to_string(), Value::String("kaunas".to_string()));
    }
}

async fn convert_identity_hoprd_v2_to_v3(resource: &mut Value) {
    if let Some(spec) = resource.get_mut("spec") {
        // Rename spec.nativeAddress to spec.nodeAddress
        if let Some(native_address_value) = spec.get("nativeAddress").cloned() {
            // Remove old field
            spec.as_object_mut().unwrap().remove("nativeAddress");
            // Add new field
            spec.as_object_mut().unwrap().insert("nodeAddress".to_string(), native_address_value);
        }
        // Remove spec.peerId
        if let Some(_) = spec.get("peerId").cloned() {
            spec.as_object_mut().unwrap().remove("peerId");
        }
    }
}

async fn convert_identity_hoprd_v3_to_v2(resource: &mut Value) {
    if let Some(spec) = resource.get_mut("spec") {
        // Rename spec.nodeAddress to spec.nativeAddress
        if let Some(node_address_value) = spec.get("nodeAddress").cloned() {
            // Remove old field
            spec.as_object_mut().unwrap().remove("nodeAddress");
            // Add new field
            spec.as_object_mut().unwrap().insert("nativeAddress".to_string(), node_address_value);
        }
        // Add spec.peerId with empty string
        spec.as_object_mut().unwrap().insert("peerId".to_string(), Value::String("deprecated".to_string()));
    }
}

async fn convert_identity_pool_v2_to_v3(_resource: &mut Value) {
    // No changes needed for IdentityPool between v1alpha2 and v1alpha3
}

async fn convert_identity_pool_v3_to_v2(_resource: &mut Value) {
    // No changes needed for IdentityPool between v1alpha3 and v1alpha2
}

// Conversion handler
async fn convert(Json(review): Json<ConversionReview>) -> Json<ConversionReview> {
    let mut response = ConversionResponse {
        uid: review.request.as_ref().unwrap().uid.clone(),
        converted_objects: vec![],
        result: Status {
            status: "Success".to_string(),
            message: None,
        },
    };

    let req = review.request.unwrap();
    for obj in req.objects {
        let mut resource = obj.clone();

        match req.desired_apiversion.as_str() {
            "clusterhoprds.hoprnet.org/v1alpha3" => convert_cluster_hoprd_v2_to_v3(&mut resource).await,
            "clusterhoprds.hoprnet.org/v1alpha2" => convert_cluster_hoprd_v3_to_v2(&mut resource).await,
            "hoprds.hoprnet.org/v1alpha3" => convert_hoprd_v2_to_v3(&mut resource).await,
            "hoprds.hoprnet.org/v1alpha2" => convert_hoprd_v3_to_v2(&mut resource).await,
            "identityhoprds.hoprnet.org/v1alpha3" => convert_identity_hoprd_v2_to_v3(&mut resource).await,
            "identityhoprds.hoprnet.org/v1alpha2" => convert_identity_hoprd_v3_to_v2(&mut resource).await,
            "identitypools.hoprnet.org/v1alpha3" => convert_identity_pool_v2_to_v3(&mut resource).await,
            "identitypools.hoprnet.org/v1alpha2" => convert_identity_pool_v3_to_v2(&mut resource).await,
            _ => {
                response.result = Status {
                    status: "Failure".to_string(),
                    message: Some(format!("Unsupported desired API version: {}", req.desired_apiversion)),
                };
                return Json(ConversionReview {
                    request: None,
                    response: Some(response),
                });
            }
            }
        response.converted_objects.push(resource);
    }

    Json(ConversionReview {
        request: None,
        response: Some(response),
    })
}