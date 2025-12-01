use axum::{Json, Router, response::IntoResponse, routing::post};
use axum_server::{tls_rustls::{RustlsConfig, bind_rustls}};
use rustls::{ServerConfig, pki_types::{CertificateDer, PrivateKeyDer}};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::{env, io::BufReader, net::SocketAddr};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use rustls::crypto::ring::default_provider;
use tracing::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value};
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};
use crate::operator_config::WebhookConfig;

#[derive(Deserialize, Serialize, Debug)]
struct ConversionRequest {
    request: ConversionRequestInner,
}

#[derive(Deserialize, Serialize, Debug)]
struct ConversionRequestInner {
    uid: String,
    #[serde(rename = "desiredAPIVersion")]
    desired_apiversion: String,
    objects: Vec<Value>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ConversionResponse {
    #[serde(rename = "apiVersion")]
    api_version: String,
    kind: String,
    response: ConversionResponseInner,
}

#[derive(Deserialize, Serialize, Debug)]
struct ConversionResponseInner {
    uid: String,
    #[serde(rename = "convertedObjects")]
    converted_objects: Vec<Value>,
    result: StatusResult,
}

#[derive(Deserialize, Serialize, Debug)]
struct StatusResult {
    status: String,
    message: Option<String>,
}

pub async fn wait_for_webhook_ready() -> Result<(), String> {
    let addr = "127.0.0.1:8443";  // use localhost, not 0.0.0.0
    info!("Waiting for webhook to be ready at {}", addr);
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

async fn add_observed_generation(resource: &mut Value) -> Result<(), String> {
    let observed_generation = resource.get("metadata")
        .and_then(|m| m.get("generation"))
        .cloned()
        .unwrap_or(Value::Number(0.into()));
    let status = resource.get_mut("status").ok_or("Missing 'status' field in v3 object")?;
    let status_obj = status.as_object_mut().ok_or("Status is not a JSON object")?;
    status_obj.insert("observed_generation".to_owned(), observed_generation);
    Ok(())
}


async fn add_status_checksum(resource: &mut Value) -> Result<(), String> {
    // Clone the spec value before mutably borrowing resource
    let spec = resource.get("spec").cloned().ok_or("Missing 'spec' field in object")?;
    let status = resource.get_mut("status").ok_or("Missing 'status' field in object")?;
    let status_obj = status.as_object_mut().ok_or("Status is not a JSON object")?;

    let mut hasher: DefaultHasher = DefaultHasher::new();
    spec.hash(&mut hasher);
    let checksum = hasher.finish().to_string();

    status_obj.insert("checksum".to_string(), Value::String(checksum.clone()));

    Ok(())
}

async fn convert_cluster_hoprd_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    debug!("Convert clusterHoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;


    // Move spec.portsAllocation to spec.service.portsAllocation
    // First, remove portsAllocation from spec_obj to avoid double mutable borrow
    let ports_allocation_value = spec_obj.remove("portsAllocation");
    if let Some(service) = spec_obj.get_mut("service").and_then(|p| p.as_object_mut()) {
        if let Some(ports_allocation_value) = ports_allocation_value {
            service.insert("portsAllocation".to_string(), ports_allocation_value);
        } else {
            warn!("spec.portsAllocation missing; adding default portsAllocation");
            service.insert("portsAllocation".to_string(), Value::Number(10.into()));
        }
    }

    // Remove spec.supportedRelease
    spec_obj.remove("supportedRelease");
    // Remove spec.forceIdentityName
    spec_obj.remove("forceIdentityName");

    add_observed_generation(resource).await?;

    Ok(())

}

async fn convert_cluster_hoprd_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    debug!("Convert clusterHoprd: v1alpha3 -> v1alpha2");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Move spec.service.portsAllocation to spec.portsAllocation
    if let Some(service) = spec_obj.get_mut("service").and_then(|p| p.as_object_mut()) {
        if let Some(ports_allocation_value) = service.remove("portsAllocation") {
            spec_obj.insert("portsAllocation".to_string(), ports_allocation_value);
        } else {
            warn!("spec.service.portsAllocation missing; adding default portsAllocation");
            spec_obj.insert("portsAllocation".to_string(), Value::Number(10.into()));
        }
    }

    // Insert spec.supportedRelease with "kaunas" value
    spec_obj.insert("supportedRelease".to_string(), Value::String("kaunas".to_string()));

    // Insert spec.forceIdentityName with true value
    spec_obj.insert("forceIdentityName".to_string(), Value::Bool(true));

    add_status_checksum(resource).await?;

    Ok(())

}

async fn convert_hoprd_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    debug!("Convert hoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Move spec.portsAllocation to spec.service.portsAllocation
    // First, remove portsAllocation from spec_obj to avoid double mutable borrow
    let ports_allocation_value = spec_obj.remove("portsAllocation");
    if let Some(service) = spec_obj.get_mut("service").and_then(|p| p.as_object_mut()) {
        if let Some(ports_allocation_value) = ports_allocation_value {
            service.insert("portsAllocation".to_string(), ports_allocation_value);
        } else {
            warn!("spec.portsAllocation missing; adding default portsAllocation");
            service.insert("portsAllocation".to_string(), Value::Number(10.into()));
        }
    }
    // Remove spec.supportedRelease
    spec_obj.remove("supportedRelease");

    add_observed_generation(resource).await?;

    Ok(())


}

async fn convert_hoprd_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    debug!("Convert hoprd: v1alpha3 -> v1alpha2");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Move spec.service.portsAllocation to spec.portsAllocation
    if let Some(service) = spec_obj.get_mut("service").and_then(|p| p.as_object_mut()) {
        if let Some(ports_allocation_value) = service.remove("portsAllocation") {
            spec_obj.insert("portsAllocation".to_string(), ports_allocation_value);
        } else {
            warn!("spec.service.portsAllocation missing; adding default portsAllocation");
            spec_obj.insert("portsAllocation".to_string(), Value::Number(10.into()));
        }
    }

    // Insert spec.supportedRelease with "kaunas" value
    spec_obj.insert("supportedRelease".to_string(), Value::String("kaunas".to_string()));

    add_status_checksum(resource).await?;

    Ok(())
}

async fn convert_identity_hoprd_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    debug!("Convert identityHoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Rename spec.nativeAddress to spec.nodeAddress
    if let Some(native_address) = spec_obj.remove("nativeAddress") {
        debug!("Renaming spec.nativeAddress to spec.nodeAddress");
        spec_obj.insert("nodeAddress".to_string(), native_address);
    } else {
        warn!("spec.nativeAddress missing; adding placeholder nativeAddress");
        spec_obj.insert("nodeAddress".to_string(), Value::String("lost".into()));
    }

    // Remove spec.peerId if present
    debug!("Removing spec.peerId if present");
    spec_obj.remove("peerId");

    add_observed_generation(resource).await?;

    Ok(())

}

async fn convert_identity_hoprd_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    debug!("Convert identityHoprd: v1alpha3 -> v1alpha2");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;

    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Rename spec.nodeAddress to spec.nativeAddress
    if let Some(native_address) = spec_obj.remove("nodeAddress") {
        spec_obj.insert("nativeAddress".to_string(), native_address);
    } else {
        warn!("spec.nodeAddress missing; adding placeholder nativeAddress");
        spec_obj.insert("nativeAddress".to_string(), Value::String("unknown".into()));
    }

    // Insert spec.peerId with unknown placeholder
    spec_obj.insert("peerId".to_string(), Value::String("unknown".into()));

    add_status_checksum(resource).await?;

    Ok(())

}

async fn convert_identity_pool_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    add_observed_generation(resource).await?;
    Ok(())
}

async fn convert_identity_pool_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    add_status_checksum(resource).await?;

    Ok(())
}

async fn convert_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    let kind = resource.get("kind").and_then(|k| k.as_str()).ok_or("Missing 'kind' field")?;
    match kind {
        "ClusterHoprd" => convert_cluster_hoprd_v2_to_v3(resource).await,
        "Hoprd" => convert_hoprd_v2_to_v3(resource).await,
        "IdentityHoprd" => convert_identity_hoprd_v2_to_v3(resource).await,
        "IdentityPool" => convert_identity_pool_v2_to_v3(resource).await,
        _ => {
            Err(format!("Unsupported kind for conversion from v2 to v3: {}", kind))
        }
    }
}

async fn convert_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    let kind = resource.get("kind").and_then(|k| k.as_str()).ok_or("Missing 'kind' field")?;
    match kind {
        "ClusterHoprd" => convert_cluster_hoprd_v3_to_v2(resource).await,
        "Hoprd" => convert_hoprd_v3_to_v2(resource).await,
        "IdentityHoprd" => convert_identity_hoprd_v3_to_v2(resource).await,
        "IdentityPool" => convert_identity_pool_v3_to_v2(resource).await,
        _ => {
            Err(format!("Unsupported kind for conversion from v3 to v2: {}", kind))
        }
    }
}

// Conversion handler
async fn convert(Json(request): Json<ConversionRequest>) -> impl IntoResponse {
    debug!("Received conversion request: {:?}", request);
    let mut response_inner = ConversionResponseInner {
        uid: request.request.uid.clone(),
        converted_objects: vec![],
        result: StatusResult {
            status: "Success".to_string(),
            message: None,
        },
    };

    for obj in request.request.objects {
        let mut resource = obj.clone();
        //debug!("Converting resource: {:?}", resource);

        let conversion_result = match request.request.desired_apiversion.as_str() {
            "hoprnet.org/v1alpha2" => Ok(convert_v3_to_v2(&mut resource).await),
            "hoprnet.org/v1alpha3" => Ok(convert_v2_to_v3(&mut resource).await),
            _ => {
                let msg = format!("Unsupported desired API version: {}", request.request.desired_apiversion.as_str());
                Err(msg)
            }
        };

        if let Err(err_msg) = conversion_result {
            response_inner.result.status = "Failure".to_string();
            response_inner.result.message = Some(err_msg.clone());
            error!("{}: {}", request.request.desired_apiversion.as_str(), err_msg);
            continue; // skip this object, still try others
        }
        // Override apiVersion to match requested version
        resource["apiVersion"] = serde_json::Value::String(request.request.desired_apiversion.clone());
        response_inner.converted_objects.push(resource);
    }
    //debug!("Conversion completed successfully with {:?}", response_inner.converted_objects);
    Json(ConversionResponse {
        api_version: "apiextensions.k8s.io/v1".to_string(),
        kind: "ConversionReview".to_string(),
        response: response_inner,
    })
}