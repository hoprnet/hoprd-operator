use axum::{Router, routing::post, Json};
use tracing::info;
use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

pub async fn run_webhook_server() {
    let app = Router::new().route("/convert", post(convert));
    let addr = SocketAddr::from(([0, 0, 0, 0], 8443));
    info!("hoprd-operator conversion webhook listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
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