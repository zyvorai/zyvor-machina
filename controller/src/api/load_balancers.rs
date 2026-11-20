// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native L4 load balancer CRUD -- part of the external-cloud-client replacement ("Fleet Cloud"
//! native compute). See `controller/src/engine/load_balancer.rs` for how a listener +
//! member set gets pushed to the owning host's agent as a weighted round-robin iptables
//! rule set, and `controller/migrations/026_native_load_balancers.sql` for why this is
//! deliberately L4-only (no L7 policies, no amphora, no automatic health monitor in v1).

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

fn validate_listener(protocol: &str, port: u16) -> Result<(), ApiError> {
    machina_core::libvirt::host_network::validate_port_forward_protocol(protocol)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    machina_core::libvirt::host_network::validate_port_forward_host_port(port)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LoadBalancerRow {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub protocol: String,
    pub host_id: Uuid,
    pub listener_port: i64,
    pub status: String,
    pub status_message: String,
    pub created_at: String,
    /// Health check: `none` (default), `tcp` or `http`.
    pub hc_protocol: String,
    pub hc_port: Option<i64>,
    pub hc_path: String,
    pub hc_interval_secs: i64,
    pub hc_timeout_secs: i64,
    pub hc_healthy_threshold: i64,
    pub hc_unhealthy_threshold: i64,
}

const LB_SELECT: &str =
    "SELECT id, project_id, name, protocol, host_id, listener_port, status, status_message, created_at, hc_protocol, hc_port, hc_path, \
     hc_interval_secs, hc_timeout_secs, hc_healthy_threshold, hc_unhealthy_threshold FROM load_balancers";

pub async fn list_load_balancers(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<LoadBalancerRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, LoadBalancerRow>(&format!("{LB_SELECT} ORDER BY name"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

pub async fn get_load_balancer(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<LoadBalancerRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, LoadBalancerRow>(&format!("{LB_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct CreateLoadBalancerBody {
    pub name: String,
    pub host_id: Uuid,
    pub listener_port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

pub async fn create_load_balancer(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateLoadBalancerBody>,
) -> Result<Json<LoadBalancerRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    validate_listener(&body.protocol, body.listener_port)?;

    let host_exists: Option<(Uuid,)> = crate::db::query_as("SELECT id FROM hosts WHERE id = ?")
        .bind(body.host_id)
        .fetch_optional(&state.pool)
        .await?;
    if host_exists.is_none() {
        return Err(ApiError::bad_request("host not found"));
    }

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO load_balancers (id, project_id, name, protocol, host_id, listener_port) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.project_id)
    .bind(&body.name)
    .bind(&body.protocol)
    .bind(body.host_id)
    .bind(body.listener_port as i64)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ApiError::conflict(
                "a load balancer already listens on that host/protocol/port",
                "pick a different listener_port or host",
            )
        } else {
            ApiError::from(e)
        }
    })?;

    // No members yet, so this just marks the (empty) rule set active -- lets the caller
    // start adding members immediately instead of the row sitting in an ambiguous default
    // state until the first member triggers a reconcile.
    let _ = crate::engine::load_balancer::apply(&state.pool, &state.config, id).await;

    let row = crate::db::query_as::<_, LoadBalancerRow>(&format!("{LB_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

pub async fn delete_load_balancer(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::engine::load_balancer::teardown(&state.pool, &state.config, id).await;
    crate::db::query("DELETE FROM load_balancers WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LbMemberRow {
    pub id: Uuid,
    pub load_balancer_id: Uuid,
    pub vm_id: Uuid,
    pub vm_name: String,
    pub vm_ip: Option<String>,
    pub port: i64,
    pub weight: i64,
    pub enabled: bool,
    /// `unknown` (not probed yet), `healthy` or `unhealthy` (left out of rotation).
    pub health: String,
    pub health_detail: String,
}

const LB_MEMBER_SELECT: &str = "SELECT m.id, m.load_balancer_id, m.vm_id, v.name AS vm_name, \
     NULLIF(v.guest_ip, '') AS vm_ip, m.port, m.weight, m.enabled, m.health, m.health_detail \
     FROM lb_members m JOIN vms v ON v.id = m.vm_id";

pub async fn list_lb_members(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(lb_id): Path<Uuid>,
) -> Result<Json<Vec<LbMemberRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, LbMemberRow>(&format!(
        "{LB_MEMBER_SELECT} WHERE m.load_balancer_id = ? ORDER BY m.created_at"
    ))
    .bind(lb_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct AddLbMemberBody {
    pub vm_id: Uuid,
    pub port: u16,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

pub async fn add_lb_member(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(lb_id): Path<Uuid>,
    Json(body): Json<AddLbMemberBody>,
) -> Result<Json<LbMemberRow>, ApiError> {
    require_operator(&actor)?;
    if body.port == 0 {
        return Err(ApiError::bad_request("port must be non-zero"));
    }
    let vm_exists: Option<(Uuid,)> = crate::db::query_as("SELECT id FROM vms WHERE id = ?")
        .bind(body.vm_id)
        .fetch_optional(&state.pool)
        .await?;
    if vm_exists.is_none() {
        return Err(ApiError::bad_request("vm not found"));
    }

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO lb_members (id, load_balancer_id, vm_id, port, weight) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(lb_id)
    .bind(body.vm_id)
    .bind(body.port as i64)
    .bind(body.weight.max(1) as i64)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ApiError::conflict(
                "that VM/port is already a member",
                "remove it first, or use a different port",
            )
        } else {
            ApiError::from(e)
        }
    })?;

    crate::engine::load_balancer::apply(&state.pool, &state.config, lb_id)
        .await
        .map_err(|e| ApiError::bad_request(format!("member saved but rule push failed: {e}")))?;

    let row = crate::db::query_as::<_, LbMemberRow>(&format!("{LB_MEMBER_SELECT} WHERE m.id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchLbMemberBody {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub weight: Option<u32>,
}

pub async fn patch_lb_member(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((lb_id, member_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchLbMemberBody>,
) -> Result<Json<LbMemberRow>, ApiError> {
    require_operator(&actor)?;
    if let Some(enabled) = body.enabled {
        crate::db::query("UPDATE lb_members SET enabled = ? WHERE id = ? AND load_balancer_id = ?")
            .bind(enabled)
            .bind(member_id)
            .bind(lb_id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(weight) = body.weight {
        crate::db::query("UPDATE lb_members SET weight = ? WHERE id = ? AND load_balancer_id = ?")
            .bind(weight.max(1) as i64)
            .bind(member_id)
            .bind(lb_id)
            .execute(&state.pool)
            .await?;
    }

    crate::engine::load_balancer::apply(&state.pool, &state.config, lb_id)
        .await
        .map_err(|e| ApiError::bad_request(format!("member updated but rule push failed: {e}")))?;

    let row = crate::db::query_as::<_, LbMemberRow>(&format!("{LB_MEMBER_SELECT} WHERE m.id = ?"))
        .bind(member_id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

pub async fn delete_lb_member(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((lb_id, member_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("DELETE FROM lb_members WHERE id = ? AND load_balancer_id = ?")
        .bind(member_id)
        .bind(lb_id)
        .execute(&state.pool)
        .await?;
    crate::engine::load_balancer::apply(&state.pool, &state.config, lb_id)
        .await
        .map_err(|e| ApiError::bad_request(format!("member removed but rule push failed: {e}")))?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Deserialize)]
pub struct HealthCheckBody {
    pub protocol: String,
    #[serde(default)]
    pub port: Option<i64>,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default = "default_interval")]
    pub interval_secs: i64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: i64,
    #[serde(default = "default_healthy")]
    pub healthy_threshold: i64,
    #[serde(default = "default_unhealthy")]
    pub unhealthy_threshold: i64,
}

fn default_path() -> String {
    "/".into()
}
fn default_interval() -> i64 {
    10
}
fn default_timeout() -> i64 {
    3
}
fn default_healthy() -> i64 {
    2
}
fn default_unhealthy() -> i64 {
    3
}

/// `PUT /api/v1/load-balancers/{id}/health-check`: set (or with `protocol: none` clear) the member health check. Changing it
/// resets every member to `unknown` so stale verdicts do not outlive the old check.
pub async fn set_health_check(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(lb_id): Path<Uuid>,
    Json(b): Json<HealthCheckBody>,
) -> Result<Json<LoadBalancerRow>, ApiError> {
    require_operator(&actor)?;
    let check = crate::engine::lb_health::Check {
        protocol: b.protocol,
        port: b.port,
        path: b.path,
        interval_secs: b.interval_secs,
        timeout_secs: b.timeout_secs,
        healthy_threshold: b.healthy_threshold,
        unhealthy_threshold: b.unhealthy_threshold,
    };
    crate::engine::lb_health::validate(&check).map_err(ApiError::bad_request)?;
    let r = crate::db::query(
        "UPDATE load_balancers SET hc_protocol = ?, hc_port = ?, hc_path = ?, hc_interval_secs = ?, hc_timeout_secs = ?, \
         hc_healthy_threshold = ?, hc_unhealthy_threshold = ?, hc_last_run = NULL WHERE id = ?",
    )
    .bind(&check.protocol)
    .bind(check.port)
    .bind(&check.path)
    .bind(check.interval_secs)
    .bind(check.timeout_secs)
    .bind(check.healthy_threshold)
    .bind(check.unhealthy_threshold)
    .bind(lb_id)
    .execute(&state.pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("load balancer not found"));
    }
    crate::db::query("UPDATE lb_members SET health = 'unknown', health_ok = 0, health_fail = 0, health_detail = '' WHERE load_balancer_id = ?")
        .bind(lb_id)
        .execute(&state.pool)
        .await?;
    // Members that were out of rotation are back in until the new check says otherwise.
    let _ = crate::engine::load_balancer::apply(&state.pool, &state.config, lb_id).await;
    let row = crate::db::query_as::<_, LoadBalancerRow>(&format!("{LB_SELECT} WHERE id = ?")).bind(lb_id).fetch_one(&state.pool).await?;
    Ok(Json(row))
}
