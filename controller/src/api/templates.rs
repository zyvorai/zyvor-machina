// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::HeaderMap;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::tasks::TaskResponse;
use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TemplateRow {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub source_disk: String,
    pub cloud_init: bool,
    pub os_family: Option<String>,
    pub category: String,
    pub workload: String,
    pub description: String,
    pub featured: bool,
    pub marketplace: bool,
    pub icon: Option<String>,
    pub firewall_profile: Option<String>,
    pub approval_status: String,
    pub git_ref: String,
    pub daemon_json_path: String,
    pub project: String,
    /// `public` (default), or `private`: only the owning project and projects it shared the image with.
    pub visibility: String,
    #[sqlx(skip)]
    pub ec2_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTemplateBody {
    pub name: String,
    pub version: String,
    pub source_disk: String,
    #[serde(default)]
    pub cloud_init: bool,
    #[serde(default)]
    pub os_family: Option<String>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default = "default_marketplace")]
    pub marketplace: bool,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListTemplatesQuery {
    pub marketplace: Option<bool>,
    pub featured: Option<bool>,
    /// Only images this project may see: public ones, its own, and ones shared with it.
    pub project: Option<String>,
}

fn default_category() -> String {
    "Linux".into()
}

fn default_marketplace() -> bool {
    true
}

const TEMPLATE_SELECT: &str =
    "SELECT id, name, version, source_disk, cloud_init, os_family, category, COALESCE(workload, '') AS workload, description, featured, marketplace, icon, firewall_profile, COALESCE(approval_status, 'approved') AS approval_status, COALESCE(git_ref, '') AS git_ref, COALESCE(daemon_json_path, '') AS daemon_json_path, COALESCE(project, '') AS project, COALESCE(visibility, 'public') AS visibility FROM templates";

pub async fn list_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListTemplatesQuery>,
) -> Result<Json<Vec<TemplateRow>>, ApiError> {
    require_operator(&actor)?;
    // Build the filter from the ACTUAL boolean values. The old match only handled
    // the Some(true) arms, so ?marketplace=false (Some(false)) fell through to the
    // no-filter arm and returned every template instead of only non-marketplace
    // ones. Predicates are static literals (no user data interpolated → no
    // injection).
    let mut preds: Vec<&str> = vec![VISIBLE_TO_PROJECT];
    match q.marketplace {
        Some(true) => preds.push("marketplace = TRUE"),
        Some(false) => preds.push("marketplace = FALSE"),
        None => {}
    }
    match q.featured {
        Some(true) => preds.push("featured = TRUE"),
        Some(false) => preds.push("featured = FALSE"),
        None => {}
    }
    let where_clause = if preds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", preds.join(" AND "))
    };
    let rows = crate::db::query_as::<_, TemplateRow>(&format!(
        "{TEMPLATE_SELECT}{where_clause} ORDER BY featured DESC, name, version"
    ))
    .bind(q.project.as_deref())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(TemplateRow::with_id).collect()))
}

/// `?1` is the viewing project (NULL = no filter, the operator view).
const VISIBLE_TO_PROJECT: &str = "(?1 IS NULL OR COALESCE(visibility, 'public') = 'public' OR COALESCE(project, '') = ?1 \
     OR id IN (SELECT template_id FROM image_shares WHERE project = ?1))";

impl TemplateRow {
    fn with_id(mut self) -> Self {
        self.ec2_id = crate::resource_ids::ec2_id(crate::resource_ids::Kind::Image, self.id);
        self
    }
}

fn template_rows_with_auto_fetch(rows: Vec<TemplateRow>) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|t| {
            let mut v = serde_json::to_value(t).unwrap_or(serde_json::json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "auto_fetch".into(),
                    serde_json::json!(crate::engine::template_catalog::download_url_for(
                        &t.name, &t.version
                    )
                    .is_some()),
                );
            }
            v
        })
        .collect()
}

pub async fn list_marketplace_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    require_operator(&actor)?;
    let count: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM templates WHERE marketplace = TRUE")
        .fetch_one(&state.pool)
        .await?;
    if count == 0 {
        let _ = crate::engine::template_catalog::seed_default_templates(&state.pool).await;
    } else {
        let _ =
            crate::engine::template_catalog::prune_stale_marketplace_templates(&state.pool).await;
    }
    let rows = crate::db::query_as::<_, TemplateRow>(&format!(
        "{TEMPLATE_SELECT} WHERE marketplace = TRUE ORDER BY featured DESC, category, name, version"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(template_rows_with_auto_fetch(rows)))
}

pub async fn seed_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let inserted = crate::engine::template_catalog::seed_default_templates(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let pruned = crate::engine::template_catalog::prune_stale_marketplace_templates(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = crate::db::query_as::<_, TemplateRow>(&format!(
        "{TEMPLATE_SELECT} WHERE marketplace = TRUE ORDER BY featured DESC, category, name, version"
    ))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({
        "inserted": inserted,
        "pruned": pruned,
        "templates": rows,
    })))
}

pub async fn create_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateTemplateBody>,
) -> Result<Json<TemplateRow>, ApiError> {
    require_operator(&actor)?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO templates (id, name, version, source_disk, cloud_init, os_family, category, description, featured, marketplace, icon)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.version)
    .bind(&body.source_disk)
    .bind(body.cloud_init)
    .bind(&body.os_family)
    .bind(&body.category)
    .bind(&body.description)
    .bind(body.featured)
    .bind(body.marketplace)
    .bind(&body.icon)
    .execute(&state.pool)
    .await?;

    fetch_template_by_id(&state, id).await
}

pub async fn get_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
) -> Result<Json<TemplateRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, TemplateRow>(&format!(
        "{TEMPLATE_SELECT} WHERE name = ? AND version = ?"
    ))
    .bind(&name)
    .bind(&version)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn get_template_readiness(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
) -> Result<Json<crate::engine::template_readiness::TemplateReadiness>, ApiError> {
    require_operator(&actor)?;
    let readiness =
        crate::engine::template_readiness::check_template_readiness(&state.pool, &name, &version)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(readiness))
}

#[derive(Debug, Deserialize, Default)]
pub struct PrefetchMissingImagesBody {
    #[serde(default)]
    pub host_id: Option<uuid::Uuid>,
}

pub async fn prefetch_missing_template_images(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    body: Option<Json<PrefetchMissingImagesBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let body = body.map(|j| j.0).unwrap_or_default();
    let host_id = if let Some(id) = body.host_id {
        id
    } else {
        crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("no online hosts"))?
    };
    let task_id = enqueue_task(
        &state,
        "templates.prefetch_missing",
        serde_json::json!({ "host_id": host_id.to_string() }),
        Some("templates"),
        None,
        Some(host_id),
    )
    .await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "templates.prefetch_missing".into(),
    }))
}

pub async fn list_missing_template_images(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::engine::template_catalog::ensure_default_templates(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let missing = crate::engine::template_readiness::list_missing_marketplace_images(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let auto_fetch_count = missing.iter().filter(|m| m.auto_fetch).count();
    Ok(Json(serde_json::json!({
        "missing": missing,
        "count": missing.len(),
        "auto_fetch_count": auto_fetch_count,
        "summary": if missing.is_empty() {
            "All marketplace golden images are present on online hosts.".to_string()
        } else {
            format!(
                "{} golden image(s) missing — {} can auto-download on first VM create.",
                missing.len(),
                auto_fetch_count
            )
        },
    })))
}

pub async fn delete_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let deleted = crate::db::query("DELETE FROM templates WHERE name = ? AND version = ?")
        .bind(&name)
        .bind(&version)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::bad_request("template not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn fetch_template_by_id(state: &AppState, id: Uuid) -> Result<Json<TemplateRow>, ApiError> {
    let row = crate::db::query_as::<_, TemplateRow>(&format!("{TEMPLATE_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct ApproveTemplateBody {
    pub approval_status: String,
}

async fn run_git_template_sync(pool: &crate::db::DbPool) -> Result<usize, ApiError> {
    let dir = std::env::var("MACHINA_TEMPLATES_GIT_DIR")
        .map_err(|_| ApiError::bad_request("MACHINA_TEMPLATES_GIT_DIR not set"))?;
    crate::engine::template_git::sync_templates_from_git(pool, std::path::Path::new(&dir))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn sync_git_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let synced = run_git_template_sync(&state.pool).await?;
    Ok(Json(serde_json::json!({ "synced": synced })))
}

/// GitHub/GitLab push webhook — optional `X-Machina-Template-Sync-Token` when `MACHINA_TEMPLATES_SYNC_TOKEN` is set.
pub async fn sync_git_templates_webhook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Accept EITHER a valid webhook token (external git-server caller) OR an operator.
    // Previously, when MACHINA_TEMPLATES_SYNC_TOKEN was unset the token branch was skipped
    // entirely, leaving this state-mutating git pull triggerable by any authenticated
    // viewer.
    let token_ok = match std::env::var("MACHINA_TEMPLATES_SYNC_TOKEN") {
        Ok(expected) if !expected.is_empty() => {
            let token = headers
                .get("x-machina-template-sync-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            token == expected
        }
        _ => false,
    };
    if !token_ok {
        require_operator(&actor)?;
    }
    let synced = run_git_template_sync(&state.pool).await?;
    Ok(Json(
        serde_json::json!({ "synced": synced, "source": "webhook" }),
    ))
}

pub async fn approve_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
    Json(body): Json<ApproveTemplateBody>,
) -> Result<Json<TemplateRow>, ApiError> {
    require_admin(&actor)?;
    let status = body.approval_status.trim();
    if !["approved", "pending", "draft", "rejected"].contains(&status) {
        return Err(ApiError::bad_request(
            "approval_status must be approved|pending|draft|rejected",
        ));
    }
    crate::db::query("UPDATE templates SET approval_status = ? WHERE name = ? AND version = ?")
        .bind(status)
        .bind(&name)
        .bind(&version)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, TemplateRow>(&format!(
        "{TEMPLATE_SELECT} WHERE name = ? AND version = ?"
    ))
    .bind(&name)
    .bind(&version)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct VisibilityBody {
    pub visibility: String,
}

pub(crate) fn valid_visibility(v: &str) -> bool {
    matches!(v, "public" | "private")
}

async fn template_id(
    pool: &crate::db::DbPool,
    name: &str,
    version: &str,
) -> Result<Uuid, ApiError> {
    crate::db::query_scalar("SELECT id FROM templates WHERE name = ? AND version = ?")
        .bind(name)
        .bind(version)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::not_found("image not found"))
}

pub async fn set_template_visibility(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
    Json(body): Json<VisibilityBody>,
) -> Result<Json<TemplateRow>, ApiError> {
    require_operator(&actor)?;
    if !valid_visibility(&body.visibility) {
        return Err(ApiError::bad_request("visibility must be public or private"));
    }
    let id = template_id(&state.pool, &name, &version).await?;
    crate::db::query("UPDATE templates SET visibility = ? WHERE id = ?")
        .bind(&body.visibility)
        .bind(id)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, TemplateRow>(&format!("{TEMPLATE_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

pub async fn list_template_shares(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version)): AxumPath<(String, String)>,
) -> Result<Json<Vec<String>>, ApiError> {
    require_operator(&actor)?;
    let id = template_id(&state.pool, &name, &version).await?;
    let rows: Vec<String> =
        crate::db::query_scalar("SELECT project FROM image_shares WHERE template_id = ? ORDER BY project")
            .bind(id)
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows))
}

pub async fn share_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version, project)): AxumPath<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let known: Option<String> = crate::db::query_scalar("SELECT name FROM projects WHERE name = ?")
        .bind(&project)
        .fetch_optional(&state.pool)
        .await?;
    if known.is_none() {
        return Err(ApiError::bad_request("no project with that name"));
    }
    let id = template_id(&state.pool, &name, &version).await?;
    crate::db::query("INSERT OR IGNORE INTO image_shares (template_id, project) VALUES (?, ?)")
        .bind(id)
        .bind(&project)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "image": name, "version": version, "shared_with": project })))
}

pub async fn unshare_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    AxumPath((name, version, project)): AxumPath<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let id = template_id(&state.pool, &name, &version).await?;
    crate::db::query("DELETE FROM image_shares WHERE template_id = ? AND project = ?")
        .bind(id)
        .bind(&project)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "image": name, "version": version, "unshared": project })))
}

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn only_two_visibilities() {
        assert!(valid_visibility("public"));
        assert!(valid_visibility("private"));
        assert!(!valid_visibility("shared"));
        assert!(!valid_visibility(""));
    }
}
