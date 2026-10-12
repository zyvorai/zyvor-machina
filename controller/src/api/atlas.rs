// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Atlas storage control-plane API — thin proxy handlers over `engine::atlas_bridge`
// plus VM-oriented orchestration in `engine::atlas_vm`. Surfaced on the platform
// Storage → Atlas page and used by the VM create/snapshot/backup flows.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::{atlas_bridge, atlas_vm};
use crate::state::AppState;

/// Map an Atlas client error to an API error: connection/timeout failures become
/// a typed 503 ("Atlas unreachable"); everything else is a 502 carrying the
/// upstream message so the UI can render Atlas' own `error.code`/`message`.
fn atlas_error(e: anyhow::Error) -> ApiError {
    if let Some(re) = e.downcast_ref::<reqwest::Error>() {
        if re.is_connect() || re.is_timeout() {
            return ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "Atlas storage gateway is unreachable".into(),
                error_code: Some("atlas_unavailable".into()),
                remediation: Some(
                    "Start the Atlas gateway, set ATLAS_BASE_URL, or disable with ATLAS_ENABLED=0."
                        .into(),
                ),
                object_ref: None,
            };
        }
    }
    // Propagate the upstream Atlas status: a client error (4xx) — e.g. 409
    // conflict, 404 not found, 400 validation — passes through so the UI can act
    // on it; an Atlas 5xx is surfaced as 502 (bad gateway).
    if let Some(ae) = e.downcast_ref::<atlas_bridge::AtlasApiError>() {
        let status = StatusCode::from_u16(ae.status)
            .ok()
            .filter(|s| s.is_client_error())
            .unwrap_or(StatusCode::BAD_GATEWAY);
        let code = if ae.code.is_empty() {
            "atlas_error".to_string()
        } else {
            format!("atlas_{}", ae.code.to_lowercase())
        };
        return ApiError {
            status,
            message: ae.message.clone(),
            error_code: Some(code),
            remediation: None,
            object_ref: None,
        };
    }
    ApiError {
        status: StatusCode::BAD_GATEWAY,
        message: e.to_string(),
        error_code: Some("atlas_error".into()),
        remediation: None,
        object_ref: None,
    }
}

/// Build a client or return a typed error when Atlas is disabled.
fn client(state: &AppState) -> Result<atlas_bridge::AtlasClient, ApiError> {
    atlas_bridge::require_client(&state.config).map_err(|e| ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: e.to_string(),
        error_code: Some("atlas_disabled".into()),
        remediation: Some("Set ATLAS_ENABLED=1 and ATLAS_BASE_URL to enable Atlas storage.".into()),
        object_ref: None,
    })
}

// ---- Status & inventory ---------------------------------------------------

pub async fn atlas_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<atlas_bridge::AtlasStatus>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(atlas_bridge::status(&state.config).await))
}

pub async fn atlas_backends(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<atlas_bridge::AtlasBackend>>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_backends()
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_pools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_pools()
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_clusters(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_clusters()
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_metrics_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .metrics_summary()
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ForecastQuery {
    #[serde(default)]
    pub minutes: Option<i64>,
}

/// Atlas capacity projection (`days_to_full`, `growth_bytes_per_day`).
pub async fn atlas_forecast(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ForecastQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .forecast(q.minutes.unwrap_or(1440))
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AlertQuery {
    #[serde(default)]
    pub state: Option<String>,
}

pub async fn atlas_alerts(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<AlertQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_alerts(q.state.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_policies(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_policies()
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct VolumeQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
}

pub async fn atlas_volumes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<VolumeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    // Values are percent-encoded before being joined into the querystring — an
    // unescaped `&`/`=` in a filter value would otherwise let the caller inject
    // extra query parameters into the upstream Atlas request.
    let mut parts = Vec::new();
    if let Some(s) = &q.state {
        parts.push(format!("state={}", urlencoding::encode(s)));
    }
    if let Some(b) = &q.backend {
        parts.push(format!("backend={}", urlencoding::encode(b)));
    }
    if let Some(k) = &q.kind {
        parts.push(format!("kind={}", urlencoding::encode(k)));
    }
    if let Some(t) = &q.tenant {
        parts.push(format!("tenant={}", urlencoding::encode(t)));
    }
    client(&state)?
        .list_volumes(&parts.join("&"))
        .await
        .map_err(atlas_error)
        .map(Json)
}

// ---- Volume write path ----------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateVolumeBody {
    pub name: String,
    pub size_bytes: i64,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub storage_class: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

pub async fn atlas_create_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateVolumeBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    let cfg = &state.config;
    let policy = body.policy.as_deref().unwrap_or(&cfg.atlas_default_policy);
    client(&state)?
        .create_volume(
            &cfg.atlas_tenant_id,
            &body.name,
            body.size_bytes,
            policy,
            None,
            body.storage_class.as_deref(),
            body.namespace.as_deref(),
        )
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<String>,
) -> Result<Json<atlas_bridge::AtlasVolume>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .get_volume(&volume_id)
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_delete_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    client(&state)?
        .delete_volume(&volume_id)
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ExpandVolumeBody {
    pub new_size_bytes: i64,
}

pub async fn atlas_expand_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<String>,
    Json(body): Json<ExpandVolumeBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .expand_volume(&volume_id, body.new_size_bytes)
        .await
        .map_err(atlas_error)
        .map(Json)
}

// ---- Snapshots / clone / restore ------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SnapshotBody {
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn atlas_snapshot_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<String>,
    Json(body): Json<SnapshotBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .snapshot_volume(&volume_id, body.name.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_snapshots(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_snapshots()
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct CloneBody {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}

pub async fn atlas_clone_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(snapshot_id): Path<String>,
    Json(body): Json<CloneBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .clone_snapshot(&snapshot_id, &body.name, body.namespace.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct RestoreSnapshotBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

pub async fn atlas_restore_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(snapshot_id): Path<String>,
    Json(body): Json<RestoreSnapshotBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .restore_snapshot(
            &snapshot_id,
            body.name.as_deref(),
            body.namespace.as_deref(),
        )
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ForceQuery {
    #[serde(default)]
    pub force: bool,
}

pub async fn atlas_delete_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(snapshot_id): Path<String>,
    Query(q): Query<ForceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .delete_snapshot(&snapshot_id, q.force)
        .await
        .map_err(atlas_error)
        .map(Json)
}

// ---- Buckets & backups ----------------------------------------------------

pub async fn atlas_buckets(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_buckets()
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct CreateBucketBody {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}

pub async fn atlas_create_bucket(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBucketBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .create_bucket(&body.name, body.namespace.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct BackupQuery {
    #[serde(default)]
    pub volume_id: Option<String>,
}

pub async fn atlas_backups(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<BackupQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_backups(q.volume_id.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct BackupBody {
    pub volume_id: String,
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default = "default_backup_mode")]
    pub mode: String,
    #[serde(default)]
    pub keep: i64,
}

fn default_backup_mode() -> String {
    "data".into()
}

pub async fn atlas_backup_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BackupBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    let bucket_id = body
        .bucket_id
        .clone()
        .or_else(|| state.config.atlas_backup_bucket_id.clone())
        .ok_or_else(|| {
            ApiError::bad_request("no bucket_id given and ATLAS_BACKUP_BUCKET_ID is not configured")
        })?;
    client(&state)?
        .backup_volume(&body.volume_id, &bucket_id, &body.mode, body.keep)
        .await
        .map_err(atlas_error)
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct RestoreBackupBody {
    pub backup_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_restore_mode")]
    pub mode: String,
}

fn default_restore_mode() -> String {
    "snapshot".into()
}

pub async fn atlas_restore_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<RestoreBackupBody>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .restore_backup(&body.backup_id, body.name.as_deref(), &body.mode)
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_delete_backup(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(backup_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    client(&state)?
        .delete_backup(&backup_id)
        .await
        .map_err(atlas_error)
        .map(Json)
}

// ---- Jobs -----------------------------------------------------------------

pub async fn atlas_jobs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .list_jobs()
        .await
        .map_err(atlas_error)
        .map(Json)
}

pub async fn atlas_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<Json<atlas_bridge::AtlasJob>, ApiError> {
    require_operator(&actor)?;
    client(&state)?
        .get_job(&job_id)
        .await
        .map_err(atlas_error)
        .map(Json)
}

// ---- VM-oriented orchestration --------------------------------------------

pub async fn atlas_vm_volumes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<Vec<atlas_vm::VmAtlasVolume>>, ApiError> {
    require_operator(&actor)?;
    atlas_vm::list_vm_volumes(&state.pool, vm_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ProvisionVmVolumeBody {
    pub size_gib: i64,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default = "default_root_role")]
    pub role: String,
    #[serde(default)]
    pub name: Option<String>,
}

fn default_root_role() -> String {
    "data_disk".into()
}

/// Provision an Atlas volume and bind it to a VM (owner = machina/virtual_machine).
pub async fn atlas_provision_vm_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<ProvisionVmVolumeBody>,
) -> Result<Json<atlas_vm::VmAtlasVolume>, ApiError> {
    require_operator(&actor)?;
    atlas_vm::provision_vm_volume(
        &state,
        vm_id,
        body.size_gib,
        body.policy.as_deref(),
        &body.role,
        body.name.as_deref(),
    )
    .await
    .map_err(atlas_error)
    .map(Json)
}

/// Snapshot every Atlas-backed disk of a VM.
pub async fn atlas_snapshot_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<SnapshotBody>,
) -> Result<Json<Vec<atlas_bridge::AtlasJob>>, ApiError> {
    require_operator(&actor)?;
    atlas_vm::snapshot_vm(&state, vm_id, body.name.as_deref())
        .await
        .map_err(atlas_error)
        .map(Json)
}

/// Back up every Atlas-backed disk of a VM to the configured (or given) bucket.
#[derive(Debug, Deserialize)]
pub struct VmBackupBody {
    #[serde(default)]
    pub bucket_id: Option<String>,
    #[serde(default = "default_backup_mode")]
    pub mode: String,
    #[serde(default)]
    pub keep: i64,
}

pub async fn atlas_backup_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<VmBackupBody>,
) -> Result<Json<Vec<atlas_bridge::AtlasJob>>, ApiError> {
    require_operator(&actor)?;
    atlas_vm::backup_vm(
        &state,
        vm_id,
        body.bucket_id.as_deref(),
        &body.mode,
        body.keep,
    )
    .await
    .map_err(atlas_error)
    .map(Json)
}
