// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ApiKeyRow {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip)]
    pub projects_raw: String,
    /// Projects the key is limited to; empty = not scoped (global role only).
    #[sqlx(skip)]
    pub projects: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyBody {
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
    /// Limit the key to these projects: it can then only use the cloud APIs and the instance routes of those projects.
    #[serde(default)]
    pub projects: Vec<String>,
}

fn default_role() -> String {
    "operator".into()
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub name: String,
    pub role: String,
    pub projects: Vec<String>,
    pub token: String,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ApiKeyRow>>, ApiError> {
    require_admin(&actor)?;
    let rows = crate::db::query_as::<_, ApiKeyRow>(
        "SELECT id, name, role,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at,
                strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at) AS last_used_at,
                projects AS projects_raw
         FROM api_keys ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    let rows = rows
        .into_iter()
        .map(|mut r| {
            r.projects = serde_json::from_str(&r.projects_raw).unwrap_or_default();
            r
        })
        .collect();
    Ok(Json(rows))
}

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateApiKeyBody>,
) -> Result<Json<CreateApiKeyResponse>, ApiError> {
    require_admin(&actor)?;
    if body.name.is_empty() || body.name.len() > 128 {
        return Err(ApiError::bad_request(
            "api key name must be 1–128 characters",
        ));
    }
    if !matches!(body.role.as_str(), "admin" | "operator" | "viewer") {
        return Err(ApiError::bad_request(
            "role must be admin, operator, or viewer",
        ));
    }
    validate_scope(&body.role, &body.projects).map_err(ApiError::bad_request)?;
    if !body.projects.is_empty() {
        // The key's name is its identity in audit logs and in the scope lookup, so a scoped key's name must be unique.
        let dup: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM api_keys WHERE name = ?").bind(&body.name).fetch_one(&state.pool).await?;
        if dup > 0 {
            return Err(ApiError::conflict("an API key with that name exists", "scoped keys need a unique name"));
        }
        for p in &body.projects {
            let known: Option<i64> = crate::db::query_scalar("SELECT 1 FROM projects WHERE name = ? AND enabled = TRUE").bind(p).fetch_optional(&state.pool).await?;
            if known.is_none() {
                return Err(ApiError::bad_request(format!("no enabled project named '{p}'")));
            }
        }
    }
    let id = Uuid::new_v4();
    let token = format!("machina_{}", Uuid::new_v4());
    let hash = hash_token(&token);
    crate::db::query("INSERT INTO api_keys (id, name, key_hash, role, projects) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(&body.name)
        .bind(hash)
        .bind(&body.role)
        .bind(serde_json::to_string(&body.projects).unwrap_or_else(|_| "[]".into()))
        .execute(&state.pool)
        .await?;
    Ok(Json(CreateApiKeyResponse {
        id: id.to_string(),
        name: body.name,
        role: body.role,
        projects: body.projects,
        token,
    }))
}

/// Rotate an existing API key: issue a new token (and hash) in place, preserving the
/// key's id/name/role so callers only swap the secret. The old token stops working
/// immediately. This is the token-rotation primitive day-2 ops needs — a leaked or
/// aged key can be cycled without deleting and re-provisioning the key record.
pub async fn rotate_api_key(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<CreateApiKeyResponse>, ApiError> {
    require_admin(&actor)?;
    let existing: Option<(String, String, String)> =
        crate::db::query_as("SELECT name, role, projects FROM api_keys WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((name, role, projects)) = existing else {
        return Err(ApiError::not_found("api key not found"));
    };
    let token = format!("machina_{}", Uuid::new_v4());
    let hash = hash_token(&token);
    // Reset last_used_at too — the new secret has never been used.
    crate::db::query(
        "UPDATE api_keys SET key_hash = ?, last_used_at = NULL, created_at = datetime('now') WHERE id = ?",
    )
    .bind(hash)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(Json(CreateApiKeyResponse {
        id: id.to_string(),
        name,
        role,
        projects: serde_json::from_str(&projects).unwrap_or_default(),
        token,
    }))
}

/// A scoped key must not be an admin (admins bypass every project check) and names at most 20 projects.
pub(crate) fn validate_scope(role: &str, projects: &[String]) -> Result<(), String> {
    if projects.is_empty() {
        return Ok(());
    }
    if role == "admin" {
        return Err("an admin key cannot be limited to projects; use operator or viewer".into());
    }
    if projects.len() > 20 {
        return Err("a key can be limited to at most 20 projects".into());
    }
    if projects.iter().any(|p| p.is_empty() || p.len() > 128) {
        return Err("project names must be 1-128 characters".into());
    }
    Ok(())
}

/// Projects an `apikey:<name>` identity is limited to (empty = not scoped, or not an API key).
pub async fn scope_of(pool: &crate::db::DbPool, username: &str) -> Vec<String> {
    let Some(name) = username.strip_prefix("apikey:") else {
        return Vec::new();
    };
    let raw: Option<String> = crate::db::query_scalar("SELECT projects FROM api_keys WHERE name = ? AND projects <> '[]' LIMIT 1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default()
}

pub(crate) async fn scope_of_conn(conn: &mut crate::db::DbConn, username: &str) -> Vec<String> {
    let Some(name) = username.strip_prefix("apikey:") else {
        return Vec::new();
    };
    let raw: Option<String> = crate::db::query_scalar("SELECT projects FROM api_keys WHERE name = ? AND projects <> '[]' LIMIT 1")
        .bind(name)
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten();
    raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default()
}

pub async fn delete_api_key(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let result = crate::db::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    // Report the true outcome rather than an unconditional success: a
    // nonexistent id previously still came back as {"deleted": true}, which
    // masks typos/races (e.g. two admins deleting the same key concurrently)
    // from the caller.
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("api key not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
}

pub async fn authenticate_api_key(
    pool: &crate::db::DbPool,
    token: &str,
) -> anyhow::Result<Option<AuthUser>> {
    let hash = hash_token(token);
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT name, role FROM api_keys WHERE key_hash = ?")
            .bind(&hash)
            .fetch_optional(pool)
            .await?;
    if let Some((name, role)) = row {
        let _ =
            crate::db::query("UPDATE api_keys SET last_used_at = datetime('now') WHERE key_hash = ?")
                .bind(&hash)
                .execute(pool)
                .await;
        Ok(Some(AuthUser {
            username: format!("apikey:{name}"),
            role,
            auth_source: None,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod scope_tests {
    use super::validate_scope;

    #[test]
    fn scope_rules() {
        assert!(validate_scope("admin", &[]).is_ok(), "unscoped admin keys are as before");
        assert!(validate_scope("operator", &["lab".into()]).is_ok());
        assert!(validate_scope("viewer", &["lab".into()]).is_ok());
        assert!(validate_scope("admin", &["lab".into()]).is_err());
        assert!(validate_scope("operator", &vec!["p".to_string(); 21]).is_err());
        assert!(validate_scope("operator", &["".into()]).is_err());
    }
}
