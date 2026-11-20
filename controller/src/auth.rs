// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub username: String,
    pub role: String,
    /// Federated auth source from JWT claim (`oidc`, `saml`, `local`).
    pub auth_source: Option<String>,
}

pub fn require_admin(user: &AuthUser) -> Result<(), crate::api::ApiError> {
    if user.role == "admin" {
        Ok(())
    } else {
        Err(crate::api::ApiError::forbidden("admin role required")
            .with_remediation("Sign in with an administrator account to manage users."))
    }
}

pub fn require_operator(user: &AuthUser) -> Result<(), crate::api::ApiError> {
    if user.role == "admin" || user.role == "operator" {
        Ok(())
    } else {
        Err(crate::api::ApiError::forbidden("operator role required"))
    }
}

// Fixed, valid bcrypt hash with no known corresponding plaintext used by this
// codebase. It exists purely to give `authenticate` something to hash against
// for unknown usernames — see the comment below.
const DUMMY_BCRYPT_HASH: &str = "$2a$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy";

pub async fn authenticate(
    pool: &crate::db::DbPool,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<AuthUser>> {
    let row: Option<(String, String)> =
        crate::db::query_as("SELECT password_hash, role FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;

    // Always run bcrypt::verify, even for an unknown username, against a fixed
    // dummy hash — bcrypt is deliberately expensive (~50-100ms), so returning
    // early for a nonexistent user would make login response time an oracle
    // for username enumeration. The dummy hash's plaintext is unknown to us,
    // but that's irrelevant: `role` is None in that branch, so a coincidental
    // match still falls through to `Ok(None)` below.
    let (hash, role) = match row {
        Some((hash, role)) => (hash, Some(role)),
        None => (DUMMY_BCRYPT_HASH.to_string(), None),
    };
    // bcrypt::verify is deliberately expensive (~50-100ms at default cost). Run it
    // on the blocking pool so it doesn't stall an async runtime worker thread —
    // otherwise a burst of Basic-auth requests could starve the executor.
    let password = password.to_string();
    let verified = tokio::task::spawn_blocking(move || bcrypt::verify(&password, &hash)).await??;
    match (verified, role) {
        (true, Some(role)) => Ok(Some(AuthUser {
            username: username.to_string(),
            role,
            auth_source: Some("local".into()),
        })),
        _ => Ok(None),
    }
}

fn parse_basic_auth(value: &str) -> Option<(String, String)> {
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if std::env::var("MACHINA_SKIP_AUTH").ok().as_deref() == Some("1") {
        req.extensions_mut().insert(AuthUser {
            username: "dev".into(),
            role: "admin".into(),
            auth_source: Some("local".into()),
        });
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    if let Some(value) = auth_header {
        if let Some((user, pass)) = parse_basic_auth(value) {
            if let Ok(Some(user)) = authenticate(&state.pool, &user, &pass).await {
                req.extensions_mut().insert(user);
                return Ok(next.run(req).await);
            }
        }
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if let Ok(Some(user)) =
                crate::api::apikeys::authenticate_api_key(&state.pool, token).await
            {
                req.extensions_mut().insert(user);
                return Ok(next.run(req).await);
            }
            if let Ok(claims) = crate::jwt::verify_token(&state.config.jwt_secret, token) {
                req.extensions_mut().insert(AuthUser {
                    username: claims.sub,
                    role: claims.role,
                    auth_source: claims.auth,
                });
                return Ok(next.run(req).await);
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
    pub role: String,
}

/// Token-exchange login: trades a one-time username/password for a JWT, so a
/// caller (e.g. the web UI's "direct controller" mode) never needs to persist
/// the raw password anywhere — only the resulting short-lived token. Public
/// route (no auth_middleware), rate-limited alongside hosts::join_host.
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, crate::api::ApiError> {
    let user = authenticate(&state.pool, &req.username, &req.password)
        .await
        .map_err(|e| crate::api::ApiError::internal(e.to_string()))?
        .ok_or_else(|| crate::api::ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "invalid username or password".into(),
            error_code: Some("unauthorized".into()),
            remediation: None,
            object_ref: None,
        })?;
    let token = crate::jwt::issue_token(
        &state.config.jwt_secret,
        &user.username,
        &user.role,
        86400,
        Some("local"),
    )
    .map_err(|e| crate::api::ApiError::internal(e.to_string()))?;
    Ok(Json(LoginResponse {
        token,
        username: user.username,
        role: user.role,
    }))
}
