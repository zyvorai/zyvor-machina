// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM QoS policies (egress/ingress Mbit/s + PPS) pushed into bpfd VM edge.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct QosPolicy {
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub egress_mbps: Option<i64>,
    pub ingress_mbps: Option<i64>,
    pub pps: Option<i64>,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateQos {
    pub name: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub egress_mbps: Option<i64>,
    #[serde(default)]
    pub ingress_mbps: Option<i64>,
    #[serde(default)]
    pub pps: Option<i64>,
    #[serde(default)]
    pub description: String,
}

pub async fn list_policies(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<QosPolicy>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        crate::db::query_as(
            "SELECT id, name, project_id, egress_mbps, ingress_mbps, pps, description \
             FROM vm_qos_policies ORDER BY name",
        )
        .fetch_all(&state.pool)
        .await?,
    ))
}

pub async fn create_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateQos>,
) -> Result<Json<QosPolicy>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let id = Uuid::new_v4().to_string();
    crate::db::query(
        "INSERT INTO vm_qos_policies (id, name, project_id, egress_mbps, ingress_mbps, pps, description) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&body.name)
    .bind(body.project_id.map(|p| p.to_string()))
    .bind(body.egress_mbps)
    .bind(body.ingress_mbps)
    .bind(body.pps)
    .bind(&body.description)
    .execute(&state.pool)
    .await?;
    Ok(Json(QosPolicy {
        id,
        name: body.name,
        project_id: body.project_id.map(|p| p.to_string()),
        egress_mbps: body.egress_mbps,
        ingress_mbps: body.ingress_mbps,
        pps: body.pps,
        description: body.description,
    }))
}

pub async fn delete_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("DELETE FROM vm_qos_policies WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    let _ = crate::engine::vm_netpol::reconcile(&state.pool, true).await;
    Ok(Json(json!({"deleted": true})))
}

#[derive(Debug, Deserialize)]
pub struct BindQos {
    pub vm_id: Uuid,
}

pub async fn bind_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<BindQos>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let policy: Option<QosPolicy> = crate::db::query_as(
        "SELECT id, name, project_id, egress_mbps, ingress_mbps, pps, description \
         FROM vm_qos_policies WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?;
    let policy = policy.ok_or_else(|| ApiError::not_found("qos policy not found"))?;
    let vm_name: Option<String> = crate::db::query_scalar("SELECT name FROM vms WHERE id = ?")
        .bind(body.vm_id)
        .fetch_optional(&state.pool)
        .await?;
    let vm_name = vm_name.ok_or_else(|| ApiError::not_found("vm not found"))?;
    let bind_id = Uuid::new_v4().to_string();
    crate::db::query(
        "INSERT INTO vm_qos_bindings (id, policy_id, vm_id) VALUES (?, ?, ?) \
         ON CONFLICT(vm_id) DO UPDATE SET policy_id = excluded.policy_id",
    )
    .bind(&bind_id)
    .bind(&id)
    .bind(body.vm_id.to_string())
    .execute(&state.pool)
    .await?;

    // Push rates into bpfd VM-edge on the next forced reconcile.
    let synced = crate::engine::vm_netpol::reconcile(&state.pool, true).await;
    let edge_pushed = synced.iter().any(|s| s.ok && s.pushed);

    Ok(Json(json!({
        "bound": true,
        "policy_id": id,
        "vm_id": body.vm_id,
        "vm_name": vm_name,
        "egress_mbps": policy.egress_mbps,
        "ingress_mbps": policy.ingress_mbps,
        "pps": policy.pps,
        "edge_pushed": edge_pushed,
    })))
}
