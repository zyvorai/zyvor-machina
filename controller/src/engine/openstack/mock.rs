// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! In-process mock Keystone/Glance/Nova/Cinder for importer unit tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Clone)]
struct MockState {
    token: String,
    images: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    base: Arc<Mutex<String>>,
}

/// Starts a mock OpenStack control plane; returns base URL (no trailing slash).
pub async fn start() -> String {
    let token = format!("mock-{}", Uuid::new_v4().simple());
    let images = Arc::new(Mutex::new(HashMap::from([(
        "img-1".into(),
        b"QFI\xfbmock-qcow2-content".to_vec(),
    )])));
    let base = Arc::new(Mutex::new(String::new()));
    let state = MockState {
        token: token.clone(),
        images: images.clone(),
        base: base.clone(),
    };
    let app = Router::new()
        .route("/v3/auth/tokens", post(issue_token))
        .route("/compute/servers/detail", get(servers))
        .route("/image/v2/images", get(images_list))
        .route("/image/v2/images/{id}", get(image_show))
        .route("/image/v2/images/{id}/file", get(image_file))
        .route("/volume/volumes/detail", get(volumes))
        .route("/volume/volumes/{id}/action", post(volume_action))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    *base.lock().unwrap() = url.clone();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    // tiny yield so the accept loop is ready
    tokio::task::yield_now().await;
    url
}

async fn issue_token(State(st): State<MockState>, Json(_body): Json<Value>) -> Response {
    let base = st.base.lock().unwrap().clone();
    let body = json!({
        "token": {
            "catalog": [
                {"type": "compute", "endpoints": [{"interface": "public", "region": "RegionOne", "url": format!("{base}/compute")}]},
                {"type": "image", "endpoints": [{"interface": "public", "region": "RegionOne", "url": format!("{base}/image")}]},
                {"type": "volumev3", "endpoints": [{"interface": "public", "region": "RegionOne", "url": format!("{base}/volume")}]},
            ]
        }
    });
    let mut resp = (StatusCode::CREATED, Json(body)).into_response();
    resp.headers_mut().insert(
        "x-subject-token",
        st.token.parse().unwrap(),
    );
    resp
}

fn check_token(st: &MockState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let t = headers
        .get("x-auth-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if t == st.token {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn servers(State(st): State<MockState>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    check_token(&st, &headers)?;
    Ok(Json(json!({
        "servers": [{"id": "srv-1", "name": "web1", "status": "ACTIVE"}]
    })))
}

async fn images_list(State(st): State<MockState>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    check_token(&st, &headers)?;
    Ok(Json(json!({
        "images": [{
            "id": "img-1",
            "name": "cirros",
            "status": "active",
            "size": 18,
            "disk_format": "qcow2"
        }]
    })))
}

async fn image_show(
    State(st): State<MockState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    check_token(&st, &headers)?;
    let images = st.images.lock().unwrap();
    if !images.contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(json!({
        "id": id,
        "name": "from-volume",
        "status": "active",
        "disk_format": "qcow2"
    })))
}

async fn image_file(
    State(st): State<MockState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Bytes, StatusCode> {
    check_token(&st, &headers)?;
    let images = st.images.lock().unwrap();
    images
        .get(&id)
        .cloned()
        .map(Bytes::from)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn volumes(State(st): State<MockState>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    check_token(&st, &headers)?;
    Ok(Json(json!({
        "volumes": [{"id": "vol-1", "name": "data", "status": "available", "size": 1}]
    })))
}

async fn volume_action(
    State(st): State<MockState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    check_token(&st, &headers)?;
    if id != "vol-1" {
        return Err(StatusCode::NOT_FOUND);
    }
    if body.get("os-volume_upload_image").is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let image_id = "img-from-vol".to_string();
    st.images
        .lock()
        .unwrap()
        .insert(image_id.clone(), b"QFI\xfbvolume-export".to_vec());
    Ok(Json(json!({
        "os-volume_upload_image": {
            "image_id": image_id,
            "image_name": body["os-volume_upload_image"]["image_name"],
            "status": "uploading"
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::openstack::client::{ConnectionConfig, OpenStackClient};
    use crate::engine::openstack::inventory;

    #[tokio::test]
    async fn mock_inventory_and_download() {
        let base = start().await;
        let cfg = ConnectionConfig {
            auth_url: format!("{base}/v3"),
            username: "admin".into(),
            password: "secret".into(),
            user_domain: "Default".into(),
            project_name: "admin".into(),
            project_domain: "Default".into(),
            region: "RegionOne".into(),
            app_cred_id: String::new(),
            app_cred_secret: String::new(),
            insecure_tls: false,
        };
        let client = OpenStackClient::connect(&cfg).await.unwrap();
        let images = inventory::list_images(&client).await.unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "cirros");
        let bytes = client
            .download_bytes(&format!("{}/v2/images/img-1/file", client.image))
            .await
            .unwrap();
        assert!(!bytes.is_empty());
        let servers = inventory::list_servers(&client).await.unwrap();
        assert_eq!(servers[0].remote_id, "srv-1");
    }
}
