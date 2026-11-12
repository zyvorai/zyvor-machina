// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

use super::auth::OsActor;
use super::catalog;
use super::OsError;

pub async fn root_versions(State(state): State<AppState>) -> Json<Value> {
    let base = state.config.public_base_url.trim_end_matches('/');
    Json(json!({
        "versions": {
            "values": [{
                "id": "v3.14",
                "status": "stable",
                "updated": "2026-01-01T00:00:00Z",
                "links": [{"rel": "self", "href": format!("{base}/v3")}],
                "media-types": [{
                    "base": "application/json",
                    "type": "application/vnd.openstack.identity-v3+json"
                }]
            }]
        }
    }))
}

pub async fn identity_versions(State(state): State<AppState>) -> Json<Value> {
    let base = state.config.public_base_url.trim_end_matches('/');
    Json(json!({
        "version": {
            "id": "v3.14",
            "status": "stable",
            "updated": "2026-01-01T00:00:00Z",
            "links": [{"rel": "self", "href": format!("{base}/v3")}],
            "media-types": [{
                "base": "application/json",
                "type": "application/vnd.openstack.identity-v3+json"
            }]
        }
    }))
}

pub async fn list_projects(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    super::auth::require_operator(&actor)?;
    let rows: Vec<(uuid::Uuid, String, String, bool)> =
        crate::db::query_as("SELECT id, name, description, enabled FROM projects ORDER BY name")
            .fetch_all(&state.pool)
            .await?;
    let projects: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, description, enabled)| {
            json!({
                "id": id.to_string(),
                "name": name,
                "description": description,
                "enabled": enabled,
                "domain_id": "default",
                "is_domain": false,
                "links": {"self": null},
            })
        })
        .collect();
    Ok(Json(json!({ "projects": projects })))
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    crate::auth::require_admin(&actor.user).map_err(OsError::from)?;
    let rows: Vec<(String, String)> =
        crate::db::query_as("SELECT username, role FROM users ORDER BY username LIMIT 500")
            .fetch_all(&state.pool)
            .await?;
    let users: Vec<Value> = rows
        .into_iter()
        .map(|(name, role)| {
            json!({
                "id": name,
                "name": name,
                "enabled": true,
                "domain_id": "default",
                "password_expires_at": null,
                "options": {},
                "links": {"self": null},
                "description": role,
            })
        })
        .collect();
    let _ = catalog::build_catalog(&state.config.public_base_url);
    Ok(Json(json!({ "users": users })))
}
