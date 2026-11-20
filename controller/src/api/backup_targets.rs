// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BackupTargetRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub config_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct CreateBackupTargetBody {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub config_json: serde_json::Value,
}

fn default_kind() -> String {
    "local".into()
}

pub async fn list_backup_targets(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<BackupTargetRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, BackupTargetRow>(
        "SELECT id, name, kind, config_json FROM backup_targets ORDER BY name LIMIT 200",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_backup_target(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBackupTargetBody>,
) -> Result<Json<BackupTargetRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if !["local", "s3", "nfs"].contains(&body.kind.as_str()) {
        return Err(ApiError::bad_request(format!(
            "invalid backup kind '{}' — must be one of: local, s3, nfs",
            body.kind
        )));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO backup_targets (id, name, kind, config_json) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(&body.name)
        .bind(&body.kind)
        .bind(&body.config_json)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, BackupTargetRow>(
        "SELECT id, name, kind, config_json FROM backup_targets WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_backup_target(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    require_operator(&actor)?;
    let deleted = crate::db::query("DELETE FROM backup_targets WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found("backup target not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
