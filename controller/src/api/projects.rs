// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProjectRow {
    pub id: Option<Uuid>,
    pub name: String,
    pub vm_count: i64,
}

/// Canonical "default" project resolver, shared by every native handler
/// (`api::volumes`, `api::networking`, `api::stacks`) that accepts an optional
/// `project_id` and needs to fall back to the seeded default when omitted.
pub(crate) async fn default_project_id(pool: &crate::db::DbPool) -> Result<Uuid, ApiError> {
    crate::db::query_scalar("SELECT id FROM projects WHERE name = 'default'")
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::internal("no default project — controller bootstrap has not run"))
}

/// `GET /api/v1/projects` — VM-count-by-project view, unchanged in shape except for
/// the added `id`: every name here is backfilled into `projects` on boot (see
/// `db::ensure_native_projects`), so `id` is always present once the controller has
/// started at least once against this database.
pub async fn list_projects(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ProjectRow>>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<(Option<Uuid>, String, i64)> = crate::db::query_as(
        "SELECT p.id, v.name, COUNT(*) AS vm_count
         FROM (SELECT COALESCE(NULLIF(project, ''), 'default') AS name FROM vms) v
         LEFT JOIN projects p ON p.name = v.name
         GROUP BY v.name, p.id ORDER BY v.name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, vm_count)| ProjectRow { id, name, vm_count })
            .collect(),
    ))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectRegistryRow {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

pub async fn list_project_registry(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ProjectRegistryRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ProjectRegistryRow>(
        "SELECT id, name, description, enabled FROM projects ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectBody {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub async fn create_project(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateProjectBody>,
) -> Result<Json<ProjectRegistryRow>, ApiError> {
    require_admin(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO projects (id, name, description) VALUES (?, ?, ?)")
        .bind(id)
        .bind(&body.name)
        .bind(&body.description)
        .execute(&state.pool)
        .await?;
    Ok(Json(ProjectRegistryRow {
        id,
        name: body.name,
        description: body.description,
        enabled: true,
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectMemberRow {
    pub user_id: Uuid,
    pub username: String,
    pub role: String,
}

pub async fn list_project_members(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<ProjectMemberRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, ProjectMemberRow>(
        "SELECT a.user_id, u.username, a.role
         FROM project_role_assignments a
         JOIN users u ON u.id = a.user_id
         WHERE a.project_id = ?
         ORDER BY u.username",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct AddProjectMemberBody {
    pub user_id: Uuid,
    #[serde(default = "default_member_role")]
    pub role: String,
}

fn default_member_role() -> String {
    "operator".into()
}

pub async fn add_project_member(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddProjectMemberBody>,
) -> Result<Json<ProjectMemberRow>, ApiError> {
    require_admin(&actor)?;
    if !matches!(body.role.as_str(), "admin" | "operator" | "viewer") {
        return Err(ApiError::bad_request(
            "role must be admin, operator, or viewer",
        ));
    }
    crate::db::query(
        "INSERT OR IGNORE INTO project_role_assignments (id, user_id, project_id, role) VALUES (?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(body.user_id)
    .bind(id)
    .bind(&body.role)
    .execute(&state.pool)
    .await?;
    let username: String = crate::db::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(body.user_id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(ProjectMemberRow {
        user_id: body.user_id,
        username,
        role: body.role,
    }))
}

pub async fn remove_project_member(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    crate::db::query("DELETE FROM project_role_assignments WHERE project_id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}
