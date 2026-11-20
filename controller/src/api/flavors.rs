// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native compute flavor catalog — Phase 1 of the external-cloud-client replacement
//! ("Fleet Cloud" native compute). A flavor is a named (vcpus, memory, disk) preset
//! picked at instance-create time. Instance lifecycle itself (create/start/stop/
//! reboot/console/rename) already exists via the classic native VM APIs
//! (`api::vms`) — this is the one genuinely missing native piece; creating an
//! "instance" from a flavor is just resolving the preset to raw cpu/memory/disk
//! values and calling `vms::create_vm` with them, the same way `api::stacks`
//! already composes over it.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FlavorRow {
    pub id: Uuid,
    pub name: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_gib: i64,
    pub description: String,
    pub is_public: bool,
}

const FLAVOR_SELECT: &str =
    "SELECT id, name, vcpus, memory_mib, disk_gib, description, is_public FROM flavors";

pub async fn list_flavors(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<FlavorRow>>, ApiError> {
    require_operator(&actor)?;
    let rows =
        crate::db::query_as::<_, FlavorRow>(&format!("{FLAVOR_SELECT} ORDER BY memory_mib, vcpus"))
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows))
}

pub async fn get_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<FlavorRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, FlavorRow>(&format!("{FLAVOR_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct CreateFlavorBody {
    pub name: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_gib: i64,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub is_public: bool,
}

fn default_true() -> bool {
    true
}

/// `POST /api/v1/flavors` — admin-only, matching the pattern of every other
/// catalog/policy-shaping resource in this codebase (projects, storage tiers):
/// operators consume flavors, only admins define the preset catalog.
pub async fn create_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateFlavorBody>,
) -> Result<Json<FlavorRow>, ApiError> {
    require_admin(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.vcpus < 1 {
        return Err(ApiError::bad_request("vcpus must be at least 1"));
    }
    if body.memory_mib < 1 {
        return Err(ApiError::bad_request("memory_mib must be at least 1"));
    }
    if body.disk_gib < 1 {
        return Err(ApiError::bad_request("disk_gib must be at least 1"));
    }
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO flavors (id, name, vcpus, memory_mib, disk_gib, description, is_public) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(body.vcpus)
    .bind(body.memory_mib)
    .bind(body.disk_gib)
    .bind(&body.description)
    .bind(body.is_public)
    .execute(&state.pool)
    .await?;
    Ok(Json(FlavorRow {
        id,
        name: body.name,
        vcpus: body.vcpus,
        memory_mib: body.memory_mib,
        disk_gib: body.disk_gib,
        description: body.description,
        is_public: body.is_public,
    }))
}

pub async fn delete_flavor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    crate::db::query("DELETE FROM flavors WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
