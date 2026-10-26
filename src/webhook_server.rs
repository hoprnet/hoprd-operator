use anyhow::Error;
use axum::{Json, Router, response::IntoResponse, routing::post};
use axum_server::{tls_rustls::{RustlsConfig, bind_rustls}};
use rustls::{ServerConfig, pki_types::PrivateKeyDer};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::{env, io::BufReader, net::SocketAddr};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use rustls::crypto::ring::default_provider;
use tracing::{trace, error, info, warn};
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

async fn wait_for_webhook_ready() -> Result<(), String> {
    let addr = "127.0.0.1:8443";  // use localhost, not 0.0.0.0
    info!("Waiting for webhook to be ready");
    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }

    Err(format!("Webhook did not start on {}", addr))
}

fn load_rustls_config(cert_path: &str, key_path: &str) -> Result<ServerConfig, Error> {
    // Install the ring crypto provider globally
    // Fails only if a provider is already installed, which is harmless
    let _ = default_provider().install_default();

    // Load certificate chain
    let dir = env::current_dir()?
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in current directory path"))?
        .to_owned();
    let cert_path = format!("{}/{}", dir.clone(), cert_path);
    let cert_file = std::fs::File::open(&cert_path).map_err(|e| anyhow::anyhow!("Could not open cert file {}: {}", cert_path, e))?;
    let mut cert_reader = BufReader::new(cert_file);
    let cert_chain = certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter().collect::<Vec<_>>();

    // Load private key
    let key_path = format!("{}/{}", dir.clone(), key_path);
    let key_file = std::fs::File::open(&key_path).map_err(|e| anyhow::anyhow!("Could not open key file {}: {}", key_path, e))?;
    let mut key_reader = BufReader::new(key_file);
    let mut keys_raw = pkcs8_private_keys(&mut key_reader)
        .collect::<Result<Vec<_>, _>>()?;
    if keys_raw.is_empty() {
        return Err(anyhow::anyhow!("No private keys found in {}", key_path));
    }
    let key = PrivateKeyDer::Pkcs8(keys_raw.remove(0));

    // Build server config (no client auth)
    let mut cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(cfg)
}

pub async fn run_webhook_server(webhook_config:WebhookConfig) -> Result<(), Error> {
    // Define Axum app with routes
    let app = Router::new().route("/convert", post(convert));
    let addr = SocketAddr::from(([0, 0, 0, 0], 8443));

    info!("Starting webhook server with TLS");
    let server_config: ServerConfig = load_rustls_config(webhook_config.crt_file.as_str(), webhook_config.key_file.as_str())?;
    let tls_config = RustlsConfig::from_config(server_config.into());
    // Spawn the server in a background task
    let server_handle = tokio::spawn(async move {
        bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await
    });

    // Wait for the server to be ready
    match wait_for_webhook_ready().await {
        Ok(_) => info!("Webhook server is ready and listening on {}", addr),
        Err(e) => error!("Failed to start webhook server: {}", e),
    }
    
    // Wait for the server to complete (or error)
    match server_handle.await {
        Ok(Ok(())) => info!("Webhook server stopped gracefully"),
        Ok(Err(e)) => error!("Webhook server stopped with error: {}", e),
        Err(e) => error!("Webhook server task panicked: {}", e),
    }
    Ok(())
}

async fn add_observed_generation(resource: &mut Value) -> Result<(), String> {
    let observed_generation = resource.get("metadata")
        .and_then(|m| m.get("generation"))
        .cloned()
        .unwrap_or(Value::Number(0.into()));
    trace!("Setting observedGeneration to {:?}", observed_generation);
    let status = resource.get_mut("status").ok_or("Missing 'status' field in v3 object")?;
    let status_obj = status.as_object_mut().ok_or("Status is not a JSON object")?;
    trace!("Current status object before update: {:?}", status_obj);
    status_obj.insert("observedGeneration".to_owned(), observed_generation);
    status_obj.remove("checksum");
    trace!("Updated status object after insert: {:?}", status_obj);
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
    status_obj.remove("observedGeneration");

    Ok(())
}

// async fn add_new_updated_timestamp(resource: &mut Value) -> Result<(), String> {
//     let status = resource.get_mut("status").ok_or("Missing 'status' field in object")?;
//     let status_obj = status.as_object_mut().ok_or("Status is not a JSON object")?;

//     let timestamp = Utc::now().to_rfc3339();

//     status_obj.insert("updateTimestampNew".to_string(), Value::String(timestamp.clone()));
//     status_obj.remove("updateTimestamp");

//     Ok(())
// }

// async fn add_old_updated_timestamp(resource: &mut Value) -> Result<(), String> {
//     let status = resource.get_mut("status").ok_or("Missing 'status' field in object")?;
//     let status_obj = status.as_object_mut().ok_or("Status is not a JSON object")?;

//     status_obj.remove("updateTimestampNew");
//     let timestamp = Utc::now().to_rfc3339();
//     status_obj.insert("updateTimestamp".to_string(), Value::String(timestamp.clone()));

//     Ok(())
// }

async fn convert_cluster_hoprd_v2_to_v3(resource: &mut Value) -> Result<(), String> {
    trace!("Convert clusterHoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in ClusterHoprd v3 object")?;

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
    trace!("Convert clusterHoprd: v1alpha3 -> v1alpha2");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in ClusterHoprd v3 object")?;

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
    trace!("Convert hoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in Hoprd v3 object")?;
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
    trace!("Convert hoprd: v1alpha3 -> v1alpha2");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in Hoprd v3 object")?;
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
    trace!("Convert identityHoprd: v1alpha2 -> v1alpha3");

    let spec = resource
        .get_mut("spec")
        .ok_or("Missing 'spec' field in IdentityHoprd v3 object")?;
    let spec_obj = spec
        .as_object_mut()
        .ok_or("Spec is not a JSON object")?;

    // Rename spec.nativeAddress to spec.nodeAddress
    if let Some(native_address) = spec_obj.remove("nativeAddress") {
        trace!("Renaming spec.nativeAddress to spec.nodeAddress");
        spec_obj.insert("nodeAddress".to_string(), native_address);
    } else {
        warn!("spec.nativeAddress missing; adding placeholder nativeAddress");
        spec_obj.insert("nodeAddress".to_string(), Value::String("lost".into()));
    }

    // Remove spec.peerId if present
    trace!("Removing spec.peerId if present");
    spec_obj.remove("peerId");

    add_observed_generation(resource).await?;

    Ok(())

}

async fn convert_identity_hoprd_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    trace!("Convert identityHoprd: v1alpha3 -> v1alpha2");

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
    trace!("Convert identityPool: v1alpha2 -> v1alpha3");
    add_observed_generation(resource).await?;
    // add_new_updated_timestamp(resource).await?;
    trace!("Converted identityPool v2 to v3: {:?}", resource);
    Ok(())
}

async fn convert_identity_pool_v3_to_v2(resource: &mut Value) -> Result<(), String> {
    trace!("Convert identityPool: v1alpha3 -> v1alpha2");
    add_status_checksum(resource).await?;
    // add_old_updated_timestamp(resource).await?;
    trace!("Converted identityPool v3 to v2: {:?}", resource);
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
    trace!("Received conversion request: {:?}", request);
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
        trace!("Converting resource: {:?}", resource);

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
    trace!("Conversion completed successfully with {:?}", response_inner.converted_objects);
    Json(ConversionResponse {
        api_version: "apiextensions.k8s.io/v1".to_string(),
        kind: "ConversionReview".to_string(),
        response: response_inner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn hoprd_v2_to_v3_moves_ports_allocation_into_the_service() {
        let mut resource = json!({
            "kind": "Hoprd",
            "spec": { "portsAllocation": 7, "supportedRelease": "kaunas", "service": { "type": "ClusterIP" } },
            "status": { "checksum": "abc" },
            "metadata": { "generation": 3 },
        });
        convert_v2_to_v3(&mut resource).await.expect("conversion succeeds");
        assert_eq!(resource["spec"]["service"]["portsAllocation"], json!(7));
        assert!(resource["spec"].get("portsAllocation").is_none());
        assert!(resource["spec"].get("supportedRelease").is_none());
        assert_eq!(resource["status"]["observedGeneration"], json!(3));
        assert!(resource["status"].get("checksum").is_none());
    }

    #[tokio::test]
    async fn hoprd_v2_to_v3_uses_a_default_when_ports_allocation_is_missing() {
        let mut resource = json!({ "kind": "Hoprd", "spec": { "service": {} }, "status": {} });
        convert_v2_to_v3(&mut resource).await.expect("conversion succeeds");
        assert_eq!(resource["spec"]["service"]["portsAllocation"], json!(10));
        // Without generation the observed generation falls back to 0
        assert_eq!(resource["status"]["observedGeneration"], json!(0));
    }

    #[tokio::test]
    async fn hoprd_v3_to_v2_moves_ports_allocation_to_the_spec() {
        let mut resource = json!({
            "kind": "Hoprd",
            "spec": { "service": { "portsAllocation": 5 } },
            "status": { "observedGeneration": 1 },
        });
        convert_v3_to_v2(&mut resource).await.expect("conversion succeeds");
        assert_eq!(resource["spec"]["portsAllocation"], json!(5));
        assert_eq!(resource["spec"]["supportedRelease"], json!("kaunas"));
        assert!(resource["spec"]["service"].get("portsAllocation").is_none());
        assert!(resource["status"].get("observedGeneration").is_none());
        assert!(resource["status"]["checksum"].is_string());
    }

    #[tokio::test]
    async fn cluster_hoprd_round_trip_keeps_the_ports_allocation() {
        let mut resource = json!({
            "kind": "ClusterHoprd",
            "spec": { "portsAllocation": 4, "supportedRelease": "kaunas", "forceIdentityName": true, "service": {} },
            "status": {},
        });
        convert_v2_to_v3(&mut resource).await.expect("v2 to v3");
        assert!(resource["spec"].get("forceIdentityName").is_none());
        assert_eq!(resource["spec"]["service"]["portsAllocation"], json!(4));
        convert_v3_to_v2(&mut resource).await.expect("v3 to v2");
        assert_eq!(resource["spec"]["portsAllocation"], json!(4));
        assert_eq!(resource["spec"]["forceIdentityName"], json!(true));
    }

    #[tokio::test]
    async fn identity_hoprd_renames_the_address_field() {
        let mut resource = json!({ "kind": "IdentityHoprd", "spec": { "nativeAddress": "0x1", "peerId": "p" }, "status": {} });
        convert_v2_to_v3(&mut resource).await.expect("v2 to v3");
        assert_eq!(resource["spec"]["nodeAddress"], json!("0x1"));
        assert!(resource["spec"].get("nativeAddress").is_none());
        assert!(resource["spec"].get("peerId").is_none());
        convert_v3_to_v2(&mut resource).await.expect("v3 to v2");
        assert_eq!(resource["spec"]["nativeAddress"], json!("0x1"));
        assert_eq!(resource["spec"]["peerId"], json!("unknown"));
    }

    #[tokio::test]
    async fn identity_hoprd_uses_placeholders_when_the_address_is_missing() {
        let mut resource = json!({ "kind": "IdentityHoprd", "spec": {}, "status": {} });
        convert_v2_to_v3(&mut resource).await.expect("v2 to v3");
        assert_eq!(resource["spec"]["nodeAddress"], json!("lost"));
    }

    #[tokio::test]
    async fn identity_pool_only_touches_the_status() {
        let mut resource = json!({ "kind": "IdentityPool", "spec": { "network": "rotsee" }, "status": {}, "metadata": { "generation": 2 } });
        convert_v2_to_v3(&mut resource).await.expect("v2 to v3");
        assert_eq!(resource["status"]["observedGeneration"], json!(2));
        assert_eq!(resource["spec"]["network"], json!("rotsee"));
    }

    #[tokio::test]
    async fn malformed_resources_are_errors_not_panics() {
        assert!(convert_v2_to_v3(&mut json!({})).await.is_err());
        assert!(convert_v2_to_v3(&mut json!({ "kind": "Unknown" })).await.is_err());
        assert!(convert_v3_to_v2(&mut json!({ "kind": "Unknown" })).await.is_err());
        assert!(convert_v2_to_v3(&mut json!({ "kind": "Hoprd" })).await.is_err(), "missing spec");
        assert!(convert_v2_to_v3(&mut json!({ "kind": "Hoprd", "spec": {} })).await.is_err(), "missing status");
        assert!(convert_v2_to_v3(&mut json!({ "kind": "Hoprd", "spec": 1, "status": {} })).await.is_err(), "spec is not an object");
        assert!(convert_v3_to_v2(&mut json!({ "kind": "IdentityPool", "spec": {}, "status": 1 })).await.is_err(), "status is not an object");
    }
}
