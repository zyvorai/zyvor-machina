// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AuditRow {
    pub id: Uuid,
    pub actor: String,
    pub action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

pub async fn list_audit_logs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditRow>>, ApiError> {
    require_admin(&actor)?;
    let limit = q.limit.clamp(1, 500);
    fn escape_like(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    }
    let rows = match (&q.action, &q.actor) {
        (Some(action), Some(actor_filter)) if !action.is_empty() && !actor_filter.is_empty() => {
            crate::db::query_as::<_, AuditRow>(
                "SELECT id, actor, action, resource_type, resource_id,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM audit_logs WHERE action LIKE ? ESCAPE '\\' AND actor LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT ?",
            )
            .bind(format!("%{}%", escape_like(action)))
            .bind(format!("%{}%", escape_like(actor_filter)))
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        (Some(action), _) if !action.is_empty() => {
            crate::db::query_as::<_, AuditRow>(
                "SELECT id, actor, action, resource_type, resource_id,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM audit_logs WHERE action LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT ?",
            )
            .bind(format!("%{}%", escape_like(action)))
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        (_, Some(actor_filter)) if !actor_filter.is_empty() => {
            crate::db::query_as::<_, AuditRow>(
                "SELECT id, actor, action, resource_type, resource_id,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM audit_logs WHERE actor LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT ?",
            )
            .bind(format!("%{}%", escape_like(actor_filter)))
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
        _ => {
            crate::db::query_as::<_, AuditRow>(
                "SELECT id, actor, action, resource_type, resource_id,
                        strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
                 FROM audit_logs ORDER BY created_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&state.pool)
            .await?
        }
    };
    Ok(Json(rows))
}
