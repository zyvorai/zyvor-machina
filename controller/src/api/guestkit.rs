// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::guestkit_bridge;
use crate::state::AppState;

/// Map a GuestKit worker error to an API error: an unreachable worker (connection
/// refused / timeout) is a typed 503 the UI can show as "worker offline" rather than
/// a raw 400 with a reqwest error string; everything else stays a 400.
fn worker_error(e: anyhow::Error) -> ApiError {
    if let Some(re) = e.downcast_ref::<reqwest::Error>() {
        if re.is_connect() || re.is_timeout() {
            return ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "GuestKit worker is unreachable".into(),
                error_code: Some("guestkit_worker_unavailable".into()),
                remediation: Some(
                    "Start the GuestKit worker, set GUESTKIT_WORKER_URL, or disable with GUESTKIT_ENABLED=0.".into(),
                ),
                object_ref: None,
            };
        }
    }
    ApiError::bad_request(e.to_string())
}

pub async fn guestkit_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<guestkit_bridge::GuestkitStatus>, crate::api::ApiError> {
    require_operator(&actor)?;
    let cfg = state.config.clone();
    let status = tokio::task::spawn_blocking(move || guestkit_bridge::status(&cfg))
        .await
        .unwrap_or_else(|_| guestkit_bridge::GuestkitStatus {
            enabled: false,
            library_version: String::new(),
            worker_url: String::new(),
            worker_reachable: false,
            summary: "status check failed".into(),
        });
    Ok(Json(status))
}

#[derive(Debug, Deserialize)]
pub struct GuestkitDiskBody {
    pub image_path: String,
    #[serde(default = "default_kvm_target")]
    pub target: String,
    #[serde(default)]
    pub explain: bool,
}

fn default_kvm_target() -> String {
    "kvm".into()
}

pub async fn guestkit_doctor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<GuestkitDiskBody>,
) -> Result<Json<guestkit_bridge::GuestkitDoctorReport>, ApiError> {
    require_admin(&actor)?;
    guestkit_bridge::doctor_disk(&state.config, &body.image_path, &body.target, body.explain)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn guestkit_migrate_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<GuestkitDiskBody>,
) -> Result<Json<guestkit_bridge::GuestkitMigratePlanReport>, ApiError> {
    require_admin(&actor)?;
    guestkit_bridge::migrate_plan_disk(&state.config, &body.image_path, &body.target)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct GuestkitVmDoctorQuery {
    #[serde(default = "default_kvm_target")]
    pub target: String,
    #[serde(default)]
    pub explain: bool,
}

pub async fn guestkit_vm_doctor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Query(q): Query<GuestkitVmDoctorQuery>,
) -> Result<Json<guestkit_bridge::GuestkitDoctorReport>, ApiError> {
    require_operator(&actor)?;
    guestkit_bridge::doctor_vm(
        &state.config,
        &state.pool,
        &state.config.disk_image_dir,
        vm_id,
        &q.target,
        q.explain,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn guestkit_vm_migrate_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Query(q): Query<GuestkitVmDoctorQuery>,
) -> Result<Json<guestkit_bridge::GuestkitMigratePlanReport>, ApiError> {
    require_operator(&actor)?;
    guestkit_bridge::migrate_plan_vm(
        &state.config,
        &state.pool,
        &state.config.disk_image_dir,
        vm_id,
        &q.target,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct GuestkitJobBody {
    pub image_path: String,
    #[serde(default = "default_job_name")]
    pub name: String,
}

fn default_job_name() -> String {
    "machina-inspect".into()
}

pub async fn guestkit_submit_job(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<GuestkitJobBody>,
) -> Result<Json<guestkit_bridge::GuestkitJobSubmitResult>, ApiError> {
    require_operator(&actor)?;
    guestkit_bridge::submit_inspect_job(&state.config, &body.image_path, &body.name)
        .await
        .map_err(worker_error)
        .map(Json)
}

pub async fn guestkit_job_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    guestkit_bridge::get_worker_job_status(&state.config, &job_id)
        .await
        .map_err(worker_error)
        .map(Json)
}
