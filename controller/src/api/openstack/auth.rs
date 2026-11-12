// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Keystone v3 token issue / validate and `X-Auth-Token` middleware.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::state::AppState;

use super::catalog;
use super::OsError;

const TOKEN_TTL_HOURS: i64 = 24;

#[derive(Debug, Clone)]
pub struct OsActor {
    pub user: AuthUser,
    pub project_id: Uuid,
    pub project_name: String,
    pub token_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub auth: AuthBody,
}

#[derive(Debug, Deserialize)]
pub struct AuthBody {
    pub identity: Identity,
    pub scope: Option<Scope>,
}

#[derive(Debug, Deserialize)]
pub struct Identity {
    pub methods: Vec<String>,
    pub password: Option<PasswordAuth>,
    pub application_credential: Option<AppCredAuth>,
}

#[derive(Debug, Deserialize)]
pub struct PasswordAuth {
    pub user: PasswordUser,
}

#[derive(Debug, Deserialize)]
pub struct PasswordUser {
    pub name: Option<String>,
    pub id: Option<String>,
    pub password: String,
    #[serde(default)]
    pub domain: Option<DomainRef>,
}

#[derive(Debug, Deserialize)]
pub struct AppCredAuth {
    pub id: Option<String>,
    pub secret: Option<String>,
    /// Machina API key (`machina_…`) accepted as the secret; id optional.
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Scope {
    pub project: Option<ProjectScope>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectScope {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub domain: Option<DomainRef>,
}

#[derive(Debug, Deserialize)]
pub struct DomainRef {
    pub id: Option<String>,
    pub name: Option<String>,
}

pub async fn issue_token(
    State(state): State<AppState>,
    Json(req): Json<AuthRequest>,
) -> Result<Response, OsError> {
    let user = authenticate_identity(&state, &req.auth.identity).await?;
    let (project_id, project_name) = resolve_scope(&state, req.auth.scope.as_ref()).await?;
    let token_id = mint_token(&state, &user, project_id, &project_name).await?;
    let expires = (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).to_rfc3339();
    let issued = Utc::now().to_rfc3339();
    let catalog = catalog::build_catalog(&state.config.public_base_url);
    let body = json!({
        "token": {
            "methods": req.auth.identity.methods,
            "expires_at": expires,
            "issued_at": issued,
            "user": {
                "id": user.username,
                "name": user.username,
                "domain": {"id": "default", "name": "Default"},
            },
            "project": {
                "id": project_id.to_string(),
                "name": project_name,
                "domain": {"id": "default", "name": "Default"},
            },
            "roles": [{"id": user.role.clone(), "name": user.role}],
            "catalog": catalog,
        }
    });
    let mut resp = (StatusCode::CREATED, Json(body)).into_response();
    resp.headers_mut().insert(
        header::HeaderName::from_static("x-subject-token"),
        header::HeaderValue::from_str(&token_id).map_err(|e| OsError::internal(e.to_string()))?,
    );
    Ok(resp)
}

/// `GET /v3/auth/tokens` with `X-Auth-Token` + optional `X-Subject-Token`.
pub async fn validate_token(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response, OsError> {
    let subject = req
        .headers()
        .get("x-subject-token")
        .or_else(|| req.headers().get("x-auth-token"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| OsError::unauthorized("X-Auth-Token or X-Subject-Token required"))?;
    let actor = load_token(&state, subject).await?;
    let catalog = catalog::build_catalog(&state.config.public_base_url);
    let expires: String = crate::db::query_scalar("SELECT expires_at FROM os_tokens WHERE id = ?")
        .bind(&actor.token_id)
        .fetch_one(&state.pool)
        .await?;
    let body = json!({
        "token": {
            "methods": ["password"],
            "expires_at": expires,
            "user": {
                "id": actor.user.username,
                "name": actor.user.username,
                "domain": {"id": "default", "name": "Default"},
            },
            "project": {
                "id": actor.project_id.to_string(),
                "name": actor.project_name,
                "domain": {"id": "default", "name": "Default"},
            },
            "roles": [{"id": actor.user.role.clone(), "name": actor.user.role}],
            "catalog": catalog,
        }
    });
    let mut resp = (StatusCode::OK, Json(body)).into_response();
    resp.headers_mut().insert(
        header::HeaderName::from_static("x-subject-token"),
        header::HeaderValue::from_str(&actor.token_id)
            .map_err(|e| OsError::internal(e.to_string()))?,
    );
    Ok(resp)
}

pub async fn os_token_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if std::env::var("MACHINA_SKIP_AUTH").ok().as_deref() == Some("1") {
        let project = crate::api::projects::default_project_id(&state.pool)
            .await
            .unwrap_or_else(|_| Uuid::nil());
        let name: String = crate::db::query_scalar("SELECT name FROM projects WHERE id = ?")
            .bind(project)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "default".into());
        req.extensions_mut().insert(OsActor {
            user: AuthUser {
                username: "dev".into(),
                role: "admin".into(),
                auth_source: Some("local".into()),
            },
            project_id: project,
            project_name: name,
            token_id: "dev".into(),
        });
        req.extensions_mut().insert(AuthUser {
            username: "dev".into(),
            role: "admin".into(),
            auth_source: Some("local".into()),
        });
        return Ok(next.run(req).await);
    }

    let token = req
        .headers()
        .get("x-auth-token")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let actor = load_token(&state, token)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    req.extensions_mut().insert(actor.user.clone());
    req.extensions_mut().insert(actor);
    Ok(next.run(req).await)
}

async fn authenticate_identity(
    state: &AppState,
    identity: &Identity,
) -> Result<AuthUser, OsError> {
    if identity.methods.iter().any(|m| m == "password") {
        let pw = identity
            .password
            .as_ref()
            .ok_or_else(|| OsError::unauthorized("password identity required"))?;
        let name = pw
            .user
            .name
            .clone()
            .or_else(|| pw.user.id.clone())
            .ok_or_else(|| OsError::unauthorized("user name required"))?;
        return crate::auth::authenticate(&state.pool, &name, &pw.user.password)
            .await
            .map_err(|e| OsError::internal(e.to_string()))?
            .ok_or_else(|| OsError::unauthorized("invalid username or password"));
    }
    if identity
        .methods
        .iter()
        .any(|m| m == "application_credential")
    {
        let cred = identity
            .application_credential
            .as_ref()
            .ok_or_else(|| OsError::unauthorized("application_credential required"))?;
        let secret = cred
            .secret
            .clone()
            .or_else(|| cred.id.clone())
            .or_else(|| cred.name.clone())
            .ok_or_else(|| OsError::unauthorized("application credential secret required"))?;
        return crate::api::apikeys::authenticate_api_key(&state.pool, &secret)
            .await
            .map_err(|e| OsError::internal(e.to_string()))?
            .ok_or_else(|| OsError::unauthorized("invalid application credential"));
    }
    Err(OsError::unauthorized(
        "supported methods: password, application_credential",
    ))
}

async fn resolve_scope(
    state: &AppState,
    scope: Option<&Scope>,
) -> Result<(Uuid, String), OsError> {
    let project = scope
        .and_then(|s| s.project.as_ref())
        .ok_or_else(|| OsError::unauthorized("project scope required"))?;
    if let Some(id) = project.id.as_ref() {
        let uuid = Uuid::parse_str(id).map_err(|_| OsError::bad_request("invalid project id"))?;
        let name: Option<String> =
            crate::db::query_scalar("SELECT name FROM projects WHERE id = ? AND enabled = TRUE")
                .bind(uuid)
                .fetch_optional(&state.pool)
                .await?;
        let name = name.ok_or_else(|| OsError::unauthorized("project not found"))?;
        return Ok((uuid, name));
    }
    if let Some(name) = project.name.as_ref() {
        let row: Option<(Uuid, String)> = crate::db::query_as(
            "SELECT id, name FROM projects WHERE name = ? AND enabled = TRUE",
        )
        .bind(name)
        .fetch_optional(&state.pool)
        .await?;
        let (id, name) = row.ok_or_else(|| OsError::unauthorized("project not found"))?;
        return Ok((id, name));
    }
    Err(OsError::unauthorized("project id or name required"))
}

async fn mint_token(
    state: &AppState,
    user: &AuthUser,
    project_id: Uuid,
    project_name: &str,
) -> Result<String, OsError> {
    let token_id = format!("gAAAAA{}", Uuid::new_v4().simple());
    let expires = (Utc::now() + Duration::hours(TOKEN_TTL_HOURS)).to_rfc3339();
    crate::db::query(
        "INSERT INTO os_tokens (id, username, role, project_id, project_name, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&token_id)
    .bind(&user.username)
    .bind(&user.role)
    .bind(project_id.to_string())
    .bind(project_name)
    .bind(&expires)
    .execute(&state.pool)
    .await?;
    Ok(token_id)
}

async fn load_token(state: &AppState, token: &str) -> Result<OsActor, OsError> {
    let row: Option<(String, String, String, String, String)> = crate::db::query_as(
        "SELECT username, role, project_id, project_name, expires_at FROM os_tokens WHERE id = ?",
    )
    .bind(token)
    .fetch_optional(&state.pool)
    .await?;
    let Some((username, role, project_id_s, project_name, expires_at)) = row else {
        return Err(OsError::unauthorized("invalid token"));
    };
    let project_id = Uuid::parse_str(&project_id_s)
        .map_err(|_| OsError::unauthorized("invalid token project"))?;
    let expires = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&expires_at, "%Y-%m-%d %H:%M:%S")
                .map(|n| n.and_utc())
        })
        .map_err(|_| OsError::unauthorized("token expired"))?;
    if expires < Utc::now() {
        let _ = crate::db::query("DELETE FROM os_tokens WHERE id = ?")
            .bind(token)
            .execute(&state.pool)
            .await;
        return Err(OsError::unauthorized("token expired"));
    }
    Ok(OsActor {
        user: AuthUser {
            username,
            role,
            auth_source: Some("keystone".into()),
        },
        project_id,
        project_name,
        token_id: token.to_string(),
    })
}

#[cfg(test)]
pub(crate) async fn mint_token_for_test(
    state: &AppState,
    username: &str,
    role: &str,
    project_id: Uuid,
    project_name: &str,
) -> Result<String, OsError> {
    mint_token(
        state,
        &AuthUser {
            username: username.into(),
            role: role.into(),
            auth_source: Some("local".into()),
        },
        project_id,
        project_name,
    )
    .await
}

/// Helper for handlers that need both OsActor and AuthUser.
pub fn require_operator(actor: &OsActor) -> Result<(), OsError> {
    crate::auth::require_operator(&actor.user).map_err(OsError::from)
}

#[allow(dead_code)]
pub async fn catalog_json(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    let _ = actor;
    Ok(Json(json!({
        "catalog": catalog::build_catalog(&state.config.public_base_url)
    })))
}
