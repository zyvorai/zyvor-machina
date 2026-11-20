// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::oidc_flow;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcSettings {
    pub enabled: bool,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Deserialize)]
pub struct OidcSettingsPatch {
    pub enabled: Option<bool>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OidcCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct OidcLoginResponse {
    pub authorize_url: String,
    pub state: String,
}

pub async fn get_oidc_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<OidcSettings>, ApiError> {
    require_operator(&actor)?;
    let cfg = oidc_flow::load_config(&state.pool, &default_redirect(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(OidcSettings {
        enabled: cfg.enabled,
        issuer: cfg.issuer,
        client_id: cfg.client_id,
        client_secret: mask_secret(&cfg.client_secret),
        redirect_uri: cfg.redirect_uri,
    }))
}

pub async fn patch_oidc_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<OidcSettingsPatch>,
) -> Result<Json<OidcSettings>, ApiError> {
    require_admin(&actor)?;
    if let Some(v) = body.enabled {
        crate::db::query("UPDATE clusters SET oidc_enabled = ?")
            .bind(v)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = &body.issuer {
        if !v.is_empty() && !v.starts_with("https://") {
            return Err(ApiError::bad_request("OIDC issuer must use HTTPS"));
        }
        crate::db::query("UPDATE clusters SET oidc_issuer = ?")
            .bind(v)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = &body.client_id {
        crate::db::query("UPDATE clusters SET oidc_client_id = ?")
            .bind(v)
            .execute(&state.pool)
            .await?;
    }
    if let Some(v) = &body.client_secret {
        if !v.is_empty() && v != "***" {
            crate::db::query("UPDATE clusters SET oidc_client_secret = ?")
                .bind(v)
                .execute(&state.pool)
                .await?;
        }
    }
    if let Some(v) = &body.redirect_uri {
        crate::db::query("UPDATE clusters SET oidc_redirect_uri = ?")
            .bind(v)
            .execute(&state.pool)
            .await?;
    }
    get_oidc_settings(State(state), Extension(actor)).await
}

pub async fn oidc_login(
    State(state): State<AppState>,
) -> Result<Json<OidcLoginResponse>, ApiError> {
    let cfg = oidc_flow::load_config(&state.pool, &default_redirect(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !cfg.enabled {
        return Err(ApiError::bad_request("OIDC is not enabled"));
    }
    let (csrf, url) = oidc_flow::begin_login(&state.pool, &cfg)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(OidcLoginResponse {
        authorize_url: url,
        state: csrf,
    }))
}

pub async fn oidc_login_redirect(State(state): State<AppState>) -> Result<Redirect, ApiError> {
    let cfg = oidc_flow::load_config(&state.pool, &default_redirect(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if !cfg.enabled {
        return Err(ApiError::bad_request("OIDC is not enabled"));
    }
    let (_csrf, url) = oidc_flow::begin_login(&state.pool, &cfg)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Redirect::temporary(&url))
}

pub async fn oidc_callback(
    State(state): State<AppState>,
    Query(q): Query<OidcCallbackQuery>,
) -> Result<axum::response::Response, ApiError> {
    let cfg = oidc_flow::load_config(&state.pool, &default_redirect(&state))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (username, role, token) = oidc_flow::complete_login(
        &state.pool,
        &cfg,
        &state.config.jwt_secret,
        &q.code,
        &q.state,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let web = state.config.web_base_url.trim_end_matches('/');
    let safe_user = html_escape(&username);
    let safe_role = html_escape(&role);
    let html = format!(
        r#"<!DOCTYPE html><html><head><title>Machina login</title></head><body>
<script>
localStorage.setItem('machina_platform_jwt', {token:?});
localStorage.removeItem('machina_platform_basic');
window.location.href = {web:?} + '/platform';
</script>
<p>Signing in as {safe_user} ({safe_role})…</p></body></html>"#
    );
    Ok(Html(html).into_response())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn default_redirect(state: &AppState) -> String {
    format!(
        "{}/api/v1/auth/oidc/callback",
        state.config.public_base_url.trim_end_matches('/')
    )
}

fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        "***".into()
    }
}
