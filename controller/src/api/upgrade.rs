// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::tasks::TaskResponse;
use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize)]
pub struct HostSkew {
    pub id: Uuid,
    pub hostname: String,
    pub agent_version: String,
    pub status: crate::engine::upgrade_skew::Status,
    pub reason: String,
    pub running_vms: i64,
    pub maintenance_mode: bool,
    /// Why upgrading this host right now would be refused (None = safe to start).
    pub blocker: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentUpgradeMatrix {
    pub controller_version: String,
    pub recommended_agent: String,
    /// Oldest agent minor the controller supports (one minor behind it).
    pub min_agent: String,
    pub notes: String,
    /// Every enrolled host with its skew status and what blocks upgrading it.
    pub hosts: Vec<HostSkew>,
    /// Safe order: controller first, then the emptiest hosts.
    pub order: Vec<String>,
}

fn min_supported(controller: &str) -> String {
    match crate::engine::upgrade_skew::parse(controller) {
        Some(v) if v.minor > 0 => format!("{}.{}.0", v.major, v.minor - 1),
        Some(v) => format!("{}.{}.0", v.major, v.minor),
        None => controller.to_string(),
    }
}

pub async fn upgrade_matrix(
    State(state): State<AppState>,
) -> Result<Json<AgentUpgradeMatrix>, ApiError> {
    let controller = env!("CARGO_PKG_VERSION");
    let rows: Vec<(Uuid, String, String, bool, i64)> = crate::db::query_as(
        "SELECT h.id, h.hostname, COALESCE(h.agent_version, ''), COALESCE(h.maintenance_mode, FALSE),
                (SELECT COUNT(*) FROM vms v WHERE v.host_id = h.id AND v.observed_state = 'running')
         FROM hosts h ORDER BY h.hostname",
    )
    .fetch_all(&state.pool)
    .await?;
    let hosts: Vec<HostSkew> = rows
        .into_iter()
        .map(|(id, hostname, agent_version, maintenance_mode, running_vms)| {
            let skew = crate::engine::upgrade_skew::classify(controller, &agent_version);
            let blocker = crate::engine::upgrade_skew::upgrade_blocker(
                controller,
                controller,
                maintenance_mode,
                running_vms,
            );
            HostSkew { id, hostname, agent_version, status: skew.status, reason: skew.reason, running_vms, maintenance_mode, blocker }
        })
        .collect();
    let load: Vec<(String, i64)> = hosts.iter().map(|h| (h.hostname.clone(), h.running_vms)).collect();
    let mut order = vec!["controller".to_string()];
    order.extend(crate::engine::upgrade_skew::order(&load).into_iter().map(String::from));
    Ok(Json(AgentUpgradeMatrix {
        controller_version: controller.into(),
        recommended_agent: controller.into(),
        min_agent: min_supported(controller),
        notes: "Upgrade the controller first, then agents one host at a time: put a host into maintenance (its machines move off), upgrade its agent, check the heartbeat, take it out of maintenance.".into(),
        hosts,
        order,
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpgradeHostBody {
    #[serde(default = "default_version")]
    pub target_version: String,
}

fn default_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

pub async fn upgrade_host_agent(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpgradeHostBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_admin(&actor)?;
    let (maintenance, running): (bool, i64) = crate::db::query_as(
        "SELECT COALESCE(h.maintenance_mode, FALSE),
                (SELECT COUNT(*) FROM vms v WHERE v.host_id = h.id AND v.observed_state = 'running')
         FROM hosts h WHERE h.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("host not found"))?;
    let target = if body.target_version.trim().is_empty() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        body.target_version.trim().to_string()
    };
    if let Some(why) = crate::engine::upgrade_skew::upgrade_blocker(
        env!("CARGO_PKG_VERSION"),
        &target,
        maintenance,
        running,
    ) {
        return Err(ApiError::conflict("agent upgrade refused", &why));
    }
    let task_id = enqueue_task(
        &state,
        "host.agent.upgrade",
        serde_json::json!({
            "host_id": id.to_string(),
            "target_version": target,
        }),
        Some("host"),
        Some(id),
        Some(id),
    )
    .await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "host.agent.upgrade".into(),
    }))
}
