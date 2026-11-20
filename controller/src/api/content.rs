// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ContentImageRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub size_gib: i64,
    pub status: String,
    pub category: String,
    pub description: String,
    pub submitted_by: Option<String>,
    pub approved_by: Option<String>,
    pub approved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub rejected_reason: Option<String>,
    pub checksum: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateContentImageBody {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub size_gib: i64,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListContentQuery {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RejectContentBody {
    pub reason: Option<String>,
}

fn default_kind() -> String {
    "iso".into()
}

fn default_category() -> String {
    "Custom Appliances".into()
}

const CONTENT_SELECT: &str = "SELECT id, name, kind, path, size_gib, status, category, description, submitted_by, approved_by, approved_at, rejected_reason, checksum, created_at FROM content_images";

pub async fn list_content_images(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListContentQuery>,
) -> Result<Json<Vec<ContentImageRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = if let Some(ref status) = q.status {
        crate::db::query_as::<_, ContentImageRow>(&format!(
            "{CONTENT_SELECT} WHERE status = ? ORDER BY name"
        ))
        .bind(status)
        .fetch_all(&state.pool)
        .await?
    } else {
        crate::db::query_as::<_, ContentImageRow>(&format!("{CONTENT_SELECT} ORDER BY name"))
            .fetch_all(&state.pool)
            .await?
    };
    Ok(Json(rows))
}

pub async fn create_content_image(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateContentImageBody>,
) -> Result<Json<ContentImageRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_one(&state.pool)
        .await?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO content_images (id, cluster_id, name, kind, path, size_gib, status, category, description, submitted_by, checksum)
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(cluster_id)
    .bind(&body.name)
    .bind(&body.kind)
    .bind(&body.path)
    .bind(body.size_gib)
    .bind(&body.category)
    .bind(&body.description)
    .bind(&actor.username)
    .bind(&body.checksum)
    .execute(&state.pool)
    .await?;
    fetch_content_row(&state, id).await
}

pub async fn approve_content_image(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ContentImageRow>, ApiError> {
    require_operator(&actor)?;
    let now = Utc::now();
    let updated = crate::db::query(
        "UPDATE content_images SET status = 'available', approved_by = ?, approved_at = ?, rejected_reason = NULL
         WHERE id = ? AND status IN ('pending', 'rejected')",
    )
    .bind(&actor.username)
    .bind(now)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::bad_request(
            "image not found or not pending approval",
        ));
    }
    fetch_content_row(&state, id).await
}

pub async fn reject_content_image(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<RejectContentBody>,
) -> Result<Json<ContentImageRow>, ApiError> {
    require_operator(&actor)?;
    let reason = body
        .reason
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Rejected by administrator".into());
    let updated = crate::db::query(
        "UPDATE content_images SET status = 'rejected', approved_by = NULL, approved_at = NULL, rejected_reason = ?
         WHERE id = ? AND status = 'pending'",
    )
    .bind(&reason)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::bad_request(
            "image not found or not pending approval",
        ));
    }
    fetch_content_row(&state, id).await
}

pub async fn delete_content_image(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let res = crate::db::query("DELETE FROM content_images WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("content image not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true, "id": id })))
}

async fn fetch_content_row(state: &AppState, id: Uuid) -> Result<Json<ContentImageRow>, ApiError> {
    let row = crate::db::query_as::<_, ContentImageRow>(&format!("{CONTENT_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}
