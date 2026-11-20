// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MigrationJobRow {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub source_host_id: Uuid,
    pub dest_host_id: Uuid,
    // Resolved hostnames (the frontend's VmMigrationRecord type expects these, matching field
    // names source_host/dest_host — the query used to select only the *_id columns, so the VM
    // detail page's "Migration history" rendered "undefined → undefined" for every row).
    pub source_host: String,
    pub dest_host: String,
    pub live: bool,
    pub status: String,
    pub progress: i16,
    pub message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const MIGRATION_JOB_SELECT: &str = "SELECT mj.id, mj.vm_id, mj.source_host_id, mj.dest_host_id,
         COALESCE(src.hostname, 'unknown') AS source_host,
         COALESCE(dst.hostname, 'unknown') AS dest_host,
         mj.live, mj.status, mj.progress, mj.message,
         strftime('%Y-%m-%dT%H:%M:%SZ', mj.created_at) AS created_at
       FROM migration_jobs mj
       LEFT JOIN hosts src ON src.id = mj.source_host_id
       LEFT JOIN hosts dst ON dst.id = mj.dest_host_id";

pub async fn list_migration_jobs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<MigrationJobRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, MigrationJobRow>(&format!(
        "{MIGRATION_JOB_SELECT} ORDER BY mj.created_at DESC LIMIT 100"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn list_vm_migration_jobs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<MigrationJobRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, MigrationJobRow>(&format!(
        "{MIGRATION_JOB_SELECT} WHERE mj.vm_id = ? ORDER BY mj.created_at DESC LIMIT 50"
    ))
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}
