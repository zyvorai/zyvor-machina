// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! First-party OpenStack importer API: connections, inventory sync, import jobs.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::openstack::{
    client::ConnectionConfig, import, inventory::InventoryKind, OpenStackClient,
};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConnectionRow {
    pub id: String,
    pub name: String,
    pub auth_url: String,
    pub username: String,
    pub project_name: String,
    pub region: String,
    pub insecure_tls: bool,
    pub status: String,
    pub last_error: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateConnection {
    pub name: String,
    pub auth_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_domain")]
    pub user_domain: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default = "default_domain")]
    pub project_domain: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default)]
    pub app_cred_id: String,
    #[serde(default)]
    pub app_cred_secret: String,
    #[serde(default)]
    pub insecure_tls: bool,
}

fn default_domain() -> String {
    "Default".into()
}
fn default_region() -> String {
    "RegionOne".into()
}

pub async fn list_connections(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ConnectionRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ConnectionRow>(
        "SELECT id, name, auth_url, username, project_name, region, insecure_tls, status, last_error \
         FROM openstack_connections ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_connection(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateConnection>,
) -> Result<Json<ConnectionRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.auth_url.trim().is_empty() {
        return Err(ApiError::bad_request("auth_url required"));
    }
    let id = Uuid::new_v4().to_string();
    let pw = crate::engine::ai::crypto::store_api_key(&body.password)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let secret = crate::engine::ai::crypto::store_api_key(&body.app_cred_secret)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query(
        "INSERT INTO openstack_connections \
         (id, name, auth_url, username, user_domain, project_name, project_domain, region, \
          password_enc, app_cred_id, app_cred_secret_enc, insecure_tls, created_by) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&body.name)
    .bind(&body.auth_url)
    .bind(&body.username)
    .bind(&body.user_domain)
    .bind(&body.project_name)
    .bind(&body.project_domain)
    .bind(&body.region)
    .bind(&pw)
    .bind(&body.app_cred_id)
    .bind(&secret)
    .bind(body.insecure_tls)
    .bind(&actor.username)
    .execute(&state.pool)
    .await?;
    Ok(Json(ConnectionRow {
        id,
        name: body.name,
        auth_url: body.auth_url,
        username: body.username,
        project_name: body.project_name,
        region: body.region,
        insecure_tls: body.insecure_tls,
        status: "ready".into(),
        last_error: String::new(),
    }))
}

pub async fn delete_connection(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("DELETE FROM openstack_connections WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({"deleted": true})))
}

async fn load_cfg(state: &AppState, id: &str) -> Result<ConnectionConfig, ApiError> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
    )> = crate::db::query_as(
        "SELECT auth_url, username, user_domain, project_name, project_domain, region, \
         password_enc, app_cred_id, app_cred_secret_enc, insecure_tls \
         FROM openstack_connections WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((
        auth_url,
        username,
        user_domain,
        project_name,
        project_domain,
        region,
        password_enc,
        app_cred_id,
        app_cred_secret_enc,
        insecure_tls,
    )) = row
    else {
        return Err(ApiError::not_found("connection not found"));
    };
    let password = crate::engine::ai::crypto::load_api_key(&password_enc)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let app_cred_secret = crate::engine::ai::crypto::load_api_key(&app_cred_secret_enc)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(ConnectionConfig {
        auth_url,
        username,
        password,
        user_domain,
        project_name,
        project_domain,
        region,
        app_cred_id,
        app_cred_secret,
        insecure_tls,
    })
}

pub async fn sync_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let cfg = load_cfg(&state, &id).await?;
    let client = OpenStackClient::connect(&cfg)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let items = crate::engine::openstack::inventory::sync_inventory(&state.pool, &id, &client)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({
        "count": items.len(),
        "items": items,
    })))
}

pub async fn list_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<(String, String, String, String, Option<i64>, String, String)> =
        crate::db::query_as(
            "SELECT kind, remote_id, name, status, size_bytes, disk_format, meta_json \
             FROM openstack_inventory WHERE connection_id = ? ORDER BY kind, name",
        )
        .bind(&id)
        .fetch_all(&state.pool)
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(kind, remote_id, name, status, size_bytes, disk_format, meta_json)| {
            json!({
                "kind": kind,
                "remote_id": remote_id,
                "name": name,
                "status": status,
                "size_bytes": size_bytes,
                "disk_format": disk_format,
                "meta": serde_json::from_str::<Value>(&meta_json).unwrap_or(json!({})),
            })
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct SubmitImport {
    pub kind: String,
    pub remote_id: String,
    #[serde(default)]
    pub dest_name: String,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    /// Lab-only: stage under this local directory instead of SSH to a host.
    #[serde(default)]
    pub local_stage_dir: Option<String>,
}

pub async fn submit_import(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<SubmitImport>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let kind = match body.kind.as_str() {
        "image" => InventoryKind::Image,
        "volume" => InventoryKind::Volume,
        "server" => InventoryKind::Server,
        _ => return Err(ApiError::bad_request("kind must be image, volume, or server")),
    };
    let cfg = load_cfg(&state, &id).await?;
    let job_id = Uuid::new_v4().to_string();
    crate::db::query(
        "INSERT INTO openstack_import_jobs \
         (id, connection_id, kind, remote_id, target, host_id, dest_name, status) \
         VALUES (?, ?, ?, ?, 'template', ?, ?, 'queued')",
    )
    .bind(&job_id)
    .bind(&id)
    .bind(kind.as_str())
    .bind(&body.remote_id)
    .bind(body.host_id.map(|h| h.to_string()))
    .bind(&body.dest_name)
    .execute(&state.pool)
    .await?;

    let req = import::ImportRequest {
        connection_id: id.clone(),
        kind,
        remote_id: body.remote_id,
        dest_name: body.dest_name,
        host_id: body.host_id,
        local_stage_dir: body.local_stage_dir.map(std::path::PathBuf::from),
    };
    let state2 = state.clone();
    let job2 = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = import::run_import_job(&state2, &job2, &cfg, &req).await {
            let _ = crate::db::query(
                "UPDATE openstack_import_jobs SET status = 'failed', error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(e.to_string())
            .bind(&job2)
            .execute(&state2.pool)
            .await;
        }
    });

    Ok(Json(json!({
        "job_id": job_id,
        "status": "queued",
    })))
}

pub async fn get_import_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(String, String, String, String, i64, String, String, String)> =
        crate::db::query_as(
            "SELECT id, connection_id, kind, remote_id, progress, status, error, COALESCE(template_id, '') \
             FROM openstack_import_jobs WHERE id = ?",
        )
        .bind(&job_id)
        .fetch_optional(&state.pool)
        .await?;
    let Some((id, connection_id, kind, remote_id, progress, status, error, template_id)) = row
    else {
        return Err(ApiError::not_found("import job not found"));
    };
    Ok(Json(json!({
        "id": id,
        "connection_id": connection_id,
        "kind": kind,
        "remote_id": remote_id,
        "progress": progress,
        "status": status,
        "error": error,
        "template_id": if template_id.is_empty() { Value::Null } else { json!(template_id) },
    })))
}

pub async fn list_import_jobs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<(String, String, String, String, i64, String, String)> = crate::db::query_as(
        "SELECT id, connection_id, kind, remote_id, progress, status, error \
         FROM openstack_import_jobs ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(&state.pool)
    .await?;
    let jobs: Vec<Value> = rows
        .into_iter()
        .map(|(id, connection_id, kind, remote_id, progress, status, error)| {
            json!({
                "id": id,
                "connection_id": connection_id,
                "kind": kind,
                "remote_id": remote_id,
                "progress": progress,
                "status": status,
                "error": error,
            })
        })
        .collect();
    Ok(Json(json!({ "jobs": jobs })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::openstack::mock;
    use crate::engine::test_support::test_state;
    use std::time::Duration;

    #[tokio::test]
    async fn import_image_via_mock() {
        let base = mock::start().await;
        let (state, _rx) = test_state().await;
        let actor = AuthUser {
            username: "admin".into(),
            role: "admin".into(),
            auth_source: Some("local".into()),
        };
        let Json(conn) = create_connection(
            State(state.clone()),
            Extension(actor.clone()),
            Json(CreateConnection {
                name: "lab".into(),
                auth_url: format!("{base}/v3"),
                username: "admin".into(),
                password: "x".into(),
                user_domain: "Default".into(),
                project_name: "admin".into(),
                project_domain: "Default".into(),
                region: "RegionOne".into(),
                app_cred_id: String::new(),
                app_cred_secret: String::new(),
                insecure_tls: false,
            }),
        )
        .await
        .unwrap();

        let Json(inv) = sync_inventory(
            State(state.clone()),
            Extension(actor.clone()),
            Path(conn.id.clone()),
        )
        .await
        .unwrap();
        assert!(inv["count"].as_u64().unwrap() >= 1);

        let dir = std::env::temp_dir().join(format!("os-import-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let Json(job) = submit_import(
            State(state.clone()),
            Extension(actor.clone()),
            Path(conn.id),
            Json(SubmitImport {
                kind: "image".into(),
                remote_id: "img-1".into(),
                dest_name: "cirros-import".into(),
                host_id: None,
                local_stage_dir: Some(dir.to_string_lossy().into()),
            }),
        )
        .await
        .unwrap();
        let job_id = job["job_id"].as_str().unwrap().to_string();
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let Json(st) = get_import_job(
                State(state.clone()),
                Extension(actor.clone()),
                Path(job_id.clone()),
            )
            .await
            .unwrap();
            if st["status"] == "done" {
                assert!(st["template_id"].as_str().is_some());
                return;
            }
            if st["status"] == "failed" {
                panic!("import failed: {}", st["error"]);
            }
        }
        panic!("import did not finish");
    }
}
