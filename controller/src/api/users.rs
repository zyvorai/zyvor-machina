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
pub struct UserRow {
    pub id: Uuid,
    pub username: String,
    pub role: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserBody {
    pub username: String,
    pub password: String,
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_role() -> String {
    "operator".into()
}

fn validate_username(username: &str) -> Result<String, ApiError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(ApiError::bad_request("username is required")
            .with_code("invalid_request")
            .with_remediation("Enter a non-empty username (letters, numbers, dash, underscore)."));
    }
    if username.len() > 64 {
        return Err(
            ApiError::bad_request("username must be at most 64 characters")
                .with_code("invalid_request"),
        );
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            ApiError::bad_request("username contains invalid characters")
                .with_code("invalid_name")
                .with_remediation("Use letters, numbers, dash, underscore, or dot only."),
        );
    }
    Ok(username.to_string())
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.trim().is_empty() {
        return Err(ApiError::bad_request("password must not be blank")
            .with_code("invalid_request")
            .with_remediation("Choose a non-blank password for platform login."));
    }
    if password.chars().count() < 8 {
        return Err(
            ApiError::bad_request("password must be at least 8 characters")
                .with_code("invalid_request")
                .with_remediation("Choose a longer password for platform login."),
        );
    }
    Ok(())
}

fn validate_role(role: &str) -> Result<String, ApiError> {
    match role {
        "admin" | "operator" | "viewer" => Ok(role.to_string()),
        _ => Err(
            ApiError::bad_request("role must be admin, operator, or viewer")
                .with_code("invalid_request"),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct PatchUserBody {
    pub role: Option<String>,
    pub password: Option<String>,
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<UserRow>>, ApiError> {
    require_admin(&actor)?;
    let rows = crate::db::query_as::<_, UserRow>(
        "SELECT id, username, role, created_at FROM users ORDER BY username",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateUserBody>,
) -> Result<Json<UserRow>, ApiError> {
    require_admin(&actor)?;
    let username = validate_username(&body.username)?;
    validate_password(&body.password)?;
    let role = validate_role(&body.role)?;
    let id = Uuid::new_v4();
    let hash = bcrypt::hash(&body.password, 12).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query("INSERT INTO users (id, username, password_hash, role) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(&username)
        .bind(hash)
        .bind(&role)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, UserRow>(
        "SELECT id, username, role, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn patch_user(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchUserBody>,
) -> Result<Json<UserRow>, ApiError> {
    require_admin(&actor)?;
    if let Some(role) = &body.role {
        let role = validate_role(role)?;
        crate::db::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(pass) = &body.password {
        validate_password(pass)?;
        let hash = bcrypt::hash(pass, bcrypt::DEFAULT_COST)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        crate::db::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(hash)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    let row = crate::db::query_as::<_, UserRow>(
        "SELECT id, username, role, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let row: (String,) = crate::db::query_as("SELECT username FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if row.0 == actor.username {
        return Err(ApiError::bad_request("cannot delete your own account")
            .with_code("invalid_request")
            .with_remediation("Sign in as another admin or delete a different user."));
    }
    crate::db::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Serialize)]
pub struct PruneInvalidUsersResponse {
    pub deleted: u64,
}

pub async fn prune_invalid_users(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<PruneInvalidUsersResponse>, ApiError> {
    require_admin(&actor)?;
    let result = crate::db::query("DELETE FROM users WHERE username IS NULL OR TRIM(username) = ''")
        .execute(&state.pool)
        .await?;
    Ok(Json(PruneInvalidUsersResponse {
        deleted: result.rows_affected(),
    }))
}

pub async fn me(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<UserRow> =
        crate::db::query_as("SELECT id, username, role, created_at FROM users WHERE username = ?")
            .bind(&actor.username)
            .fetch_optional(&state.pool)
            .await?;
    if let Some(row) = row {
        return Ok(Json(serde_json::json!({
            "id": row.id,
            "username": row.username,
            "role": row.role,
            "created_at": row.created_at,
        })));
    }
    Ok(Json(serde_json::json!({
        "username": actor.username,
        "role": actor.role,
    })))
}
