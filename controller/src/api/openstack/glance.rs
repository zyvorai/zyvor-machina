// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};

use crate::api::templates::{CreateTemplateBody, ListTemplatesQuery, TemplateRow};
use crate::state::AppState;

use super::auth::{require_operator, OsActor};
use super::ids::parse_uuid;
use super::OsError;

fn image_json(t: &TemplateRow) -> Value {
    json!({
        "id": t.id.to_string(),
        "name": t.name,
        "status": if t.approval_status == "approved" { "active" } else { "queued" },
        "visibility": t.visibility,
        "protected": false,
        "container_format": "bare",
        "disk_format": "qcow2",
        "min_disk": 0,
        "min_ram": 0,
        "size": null,
        "virtual_size": null,
        "owner": t.project,
        "tags": [],
        "created_at": "",
        "updated_at": "",
        "file": format!("/v2/images/{}/file", t.id),
        "schema": "/v2/schemas/image",
        "os_family": t.os_family,
        "machina_version": t.version,
        "machina_source_disk": t.source_disk,
    })
}

pub async fn list_images(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let Json(rows) = crate::api::templates::list_templates(
        State(state),
        Extension(actor.user.clone()),
        Query(ListTemplatesQuery {
            marketplace: None,
            featured: None,
            project: Some(actor.project_name.clone()),
        }),
    )
    .await?;
    Ok(Json(json!({
        "images": rows.iter().map(image_json).collect::<Vec<_>>(),
        "first": "/v2/images",
        "schema": "/v2/schemas/images",
    })))
}

pub async fn get_image(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<Json<Value>, OsError> {
    require_operator(&actor)?;
    let id = parse_uuid(&id)?;
    let row = crate::db::query_as::<_, TemplateRow>(
        "SELECT id, name, version, source_disk, cloud_init, os_family, category, \
         COALESCE(workload, '') AS workload, description, featured, marketplace, icon, \
         firewall_profile, COALESCE(approval_status, 'approved') AS approval_status, \
         COALESCE(git_ref, '') AS git_ref, COALESCE(daemon_json_path, '') AS daemon_json_path, \
         COALESCE(project, '') AS project, COALESCE(visibility, 'public') AS visibility \
         FROM templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| OsError::not_found("image not found"))?;
    Ok(Json(image_json(&row)))
}

pub async fn create_image(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Json(body): Json<Value>,
) -> Result<(axum::http::StatusCode, Json<Value>), OsError> {
    require_operator(&actor)?;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OsError::bad_request("name required"))?
        .to_string();
    let source_disk = body
        .get("machina_source_disk")
        .or_else(|| body.get("location"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            OsError::bad_request(
                "machina_source_disk required (host path to an existing qcow2/raw disk)",
            )
        })?
        .to_string();
    let Json(row) = crate::api::templates::create_template(
        State(state),
        Extension(actor.user.clone()),
        Json(CreateTemplateBody {
            name,
            version: body
                .get("machina_version")
                .and_then(|v| v.as_str())
                .unwrap_or("1")
                .into(),
            source_disk,
            cloud_init: true,
            os_family: body
                .get("os_family")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            category: "Linux".into(),
            description: body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            featured: false,
            marketplace: false,
            icon: None,
        }),
    )
    .await?;
    Ok((axum::http::StatusCode::CREATED, Json(image_json(&row))))
}

pub async fn delete_image(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, OsError> {
    crate::auth::require_admin(&actor.user).map_err(OsError::from)?;
    let id = parse_uuid(&id)?;
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT name, version FROM templates WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((name, version)) = row else {
        return Err(OsError::not_found("image not found"));
    };
    let _ = crate::api::templates::delete_template(
        State(state),
        Extension(actor.user.clone()),
        axum::extract::Path((name, version)),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
