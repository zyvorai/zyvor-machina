// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use machina_core::FirewallPlanRequest;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::ai::firewall as ai_firewall;
use crate::engine::bpf::{self, telemetry};
use crate::engine::zeus_firewall;
use crate::state::AppState;

pub async fn status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, crate::api::ApiError> {
    crate::auth::require_operator(&actor)?;
    Ok(Json(serde_json::json!({
        "zeus_firewall": zeus_firewall::zeus_firewall_status().await,
        "native_bpf": bpf::fleet_status(&state.pool).await,
    })))
}

pub async fn overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::FirewallOverview>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::overview(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_target(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<zeus_firewall::FirewallTargetDetail>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::target_detail(&state.pool, &state.config, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_ports(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Vec<machina_core::OpenPort>>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::target_ports(&state.pool, &state.config, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_services(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Vec<machina_core::firewall::types::AllowedService>>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::target_services(&state.pool, &state.config, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_score(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<machina_core::FirewallScore>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::target_score(&state.pool, &state.config, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct PlanBody {
    pub profile: Option<String>,
    pub preset: Option<String>,
    pub enable: Option<bool>,
    pub stealth_level: Option<String>,
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

pub async fn plan_target(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<PlanBody>,
) -> Result<Json<machina_core::FirewallPlanResult>, ApiError> {
    require_operator(&actor)?;
    let stealth = body
        .stealth_level
        .as_deref()
        .map(machina_core::firewall::apply::stealth_level_from_str);
    let req = FirewallPlanRequest {
        profile: body.profile,
        enable: body.enable,
        stealth_level: stealth,
        preset: body.preset,
        dry_run: body.dry_run,
        zone_cidrs: state.config.firewall_zones.clone(),
    };
    zeus_firewall::plan_target(&state.pool, &state.config, &id, req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn apply_target(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<PlanBody>,
) -> Result<Json<machina_core::FirewallPlanResult>, ApiError> {
    require_operator(&actor)?;
    let stealth = body
        .stealth_level
        .as_deref()
        .map(machina_core::firewall::apply::stealth_level_from_str);
    let req = FirewallPlanRequest {
        profile: body.profile,
        enable: body.enable,
        stealth_level: stealth,
        preset: body.preset,
        dry_run: false,
        zone_cidrs: state.config.firewall_zones.clone(),
    };
    zeus_firewall::apply_target(&state.pool, &state.config, &id, req, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn list_profiles(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<zeus_firewall::profiles::ProfileListItem>>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::profiles::list_profiles(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn create_temporary_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::temporary::TemporaryRuleRequest>,
) -> Result<Json<zeus_firewall::temporary::TemporaryRule>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::temporary::create_temporary_rule(&state.pool, body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn get_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    require_operator(&actor)?;
    let host_id = Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    zeus_firewall::temporary::timeline(&state.pool, "host", host_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct LockdownBody {
    #[serde(default)]
    pub capture: bool,
    /// When true (default), return the Emergency Isolation plan without applying it.
    /// Set `dry_run: false` AND `confirm: true` to apply.
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// Required to apply lockdown when dry_run is false.
    #[serde(default)]
    pub confirm: bool,
}

pub async fn lockdown(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<LockdownBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    if body.dry_run || !body.confirm {
        // Preview-only: never touch host firewall from a casual POST.
        return Ok(Json(serde_json::json!({
            "ok": true,
            "dry_run": true,
            "preview": zeus_firewall::lockdown_preview(body.capture),
            "message": "Dry-run only — resubmit with {\"dry_run\": false, \"confirm\": true} to apply Emergency Isolation",
        })));
    }
    zeus_firewall::lockdown_target(
        &state.pool,
        &state.config,
        &id,
        body.capture,
        &actor.username,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ProfileBody {
    pub profile: String,
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

pub async fn apply_profile(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ProfileBody>,
) -> Result<Json<machina_core::FirewallPlanResult>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::apply_profile(
        &state.pool,
        &state.config,
        &id,
        &body.profile,
        &actor.username,
        body.dry_run,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn list_checkpoints(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Vec<zeus_firewall::checkpoint::CheckpointSummary>>, ApiError> {
    require_operator(&actor)?;
    let host_id = Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    zeus_firewall::checkpoint::list_checkpoints(&state.pool, "host", host_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct RollbackBody {
    pub checkpoint_id: Uuid,
}

pub async fn rollback(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let host_id = Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    zeus_firewall::checkpoint::rollback_checkpoint(
        &state.pool,
        "host",
        host_id,
        body.checkpoint_id,
        &actor.username,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct SimulateBody {
    pub target_id: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "ProductionServer".into()
}

pub async fn simulate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<SimulateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai_firewall::simulate_plan(&state.pool, &state.config, &body.target_id, &body.profile)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ExplainBody {
    pub target_id: String,
    pub question: Option<String>,
}

pub async fn ai_explain(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ExplainBody>,
) -> Result<Json<ai_firewall::FirewallExplainReport>, ApiError> {
    require_operator(&actor)?;
    ai_firewall::explain_exposure(
        &state.pool,
        &state.config,
        &body.target_id,
        body.question.as_deref(),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
    .map(Json)
}

pub async fn ai_secure_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ExplainBody>,
) -> Result<Json<ai_firewall::SecurePlanReport>, ApiError> {
    require_operator(&actor)?;
    ai_firewall::secure_machine_plan(&state.pool, &state.config, &body.target_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn compliance_report(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(kind): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai_firewall::compliance_report(&state.pool, &state.config, &kind)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct SiemExportQuery {
    #[serde(default = "default_siem_hours")]
    pub hours: i32,
}

fn default_siem_hours() -> i32 {
    168
}

pub async fn siem_export(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<SiemExportQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let export = zeus_firewall::siem::export_timeline(&state.pool, q.hours)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let native = telemetry::anomalies(&state.pool).await;
    let anomalies = native
        .get("anomalies")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(Json(serde_json::json!({
        "exported_at": export.exported_at,
        "event_count": export.event_count + anomalies.len(),
        "events": export.events,
        "bpf_anomalies": anomalies,
        "sources": ["firewall_timeline", telemetry::SOURCE]
    })))
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DriftQuery {
    pub target_kind: Option<String>,
}

pub async fn detect_drift(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<zeus_firewall::drift::DriftReport>, ApiError> {
    require_operator(&actor)?;
    let host_id = Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let detail = zeus_firewall::target_detail(&state.pool, &state.config, &id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    zeus_firewall::drift::detect_drift(&state.pool, "host", host_id, &detail.inventory)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn get_activity(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    if id != "local" {
        Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("invalid host id"))?;
    }
    let hours = q.hours.unwrap_or(24);
    if let Ok(addr) = resolve_agent(&state, &id).await {
        if let Ok(act) = crate::agent_client::get_firewall_activity(&addr, hours).await {
            return Ok(Json(act));
        }
        Ok(Json(
            serde_json::json!({ "events": [], "note": "activity unavailable" }),
        ))
    } else {
        Ok(Json(
            serde_json::json!({ "events": [], "note": "no agent" }),
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    pub hours: Option<u32>,
}

async fn resolve_agent(state: &AppState, target_id: &str) -> anyhow::Result<String> {
    if target_id == "local" {
        return Ok(state.config.default_agent_addr.clone());
    }
    let host_id = Uuid::parse_str(target_id)?;
    let addr: String =
        crate::db::query_scalar("SELECT COALESCE(agent_grpc_addr, '') FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_one(&state.pool)
            .await?;
    if addr.is_empty() {
        anyhow::bail!("no agent");
    }
    Ok(addr)
}

#[derive(Debug, Deserialize)]
pub struct PolicyBody {
    pub name: String,
    pub spec_yaml: String,
}

pub async fn list_policies(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    // Firewall rule internals are operator-sensitive; gate reads like every
    // sibling handler (create/update/delete all require_operator). Also surface
    // DB errors instead of swallowing them into an empty 200.
    require_operator(&actor)?;
    let rows: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, name, spec_yaml FROM firewall_policies ORDER BY name")
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, spec_yaml)| serde_json::json!({ "id": id, "name": name, "spec_yaml": spec_yaml }))
            .collect(),
    ))
}

pub async fn create_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PolicyBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO firewall_policies (id, name, spec_yaml) VALUES (?, ?, ?)")
        .bind(id)
        .bind(&body.name)
        .bind(&body.spec_yaml)
        .execute(&state.pool)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(serde_json::json!({ "id": id, "name": body.name })))
}

pub async fn request_risky_change(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::approvals::ApprovalRequest>,
) -> Result<Json<zeus_firewall::approvals::FirewallApproval>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::request_approval(&state.pool, body, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn list_approvals(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ApprovalListQuery>,
) -> Result<Json<Vec<zeus_firewall::approvals::FirewallApproval>>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::list_approvals(&state.pool, q.status.as_deref())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ApprovalListQuery {
    pub status: Option<String>,
}

pub async fn approve_change(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<zeus_firewall::approvals::ReviewBody>,
) -> Result<Json<zeus_firewall::approvals::ApprovalApplyResult>, ApiError> {
    require_admin(&actor)?;
    zeus_firewall::approve_and_apply(
        &state.pool,
        &state.config,
        id,
        &actor.username,
        body.note.as_deref(),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn reject_change(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<zeus_firewall::approvals::ReviewBody>,
) -> Result<Json<zeus_firewall::approvals::FirewallApproval>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::reject(&state.pool, id, &actor.username, body.note.as_deref())
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn export_gitops(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::gitops::GitOpsExport>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::export_policies(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn sync_gitops(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::gitops::GitOpsSyncRequest>,
) -> Result<Json<zeus_firewall::gitops::GitOpsSyncResult>, ApiError> {
    require_admin(&actor)?;
    zeus_firewall::sync_policies(&state.pool, body, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn k8s_status(
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::k8s::K8sFirewallStatus>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(zeus_firewall::k8s::status()))
}

#[derive(Debug, Deserialize)]
pub struct K8sPlanBody {
    pub namespace: String,
    pub profile: String,
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

pub async fn k8s_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<K8sPlanBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (manifests, warnings) =
        zeus_firewall::k8s::compile_plan(&state.config, &body.namespace, &body.profile)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "manifests": manifests, "dry_run": body.dry_run, "warnings": warnings }),
    ))
}

pub async fn k8s_apply(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<K8sPlanBody>,
) -> Result<Json<machina_core::FirewallPlanResult>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::k8s::apply_plan(
        &state.config,
        &state.pool,
        &body.namespace,
        &body.profile,
        &actor.username,
        body.dry_run,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn cloud_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::cloud::CloudOverview>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::cloud::overview(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn vm_guest_ports(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<zeus_firewall::guest_ports::GuestPortReport>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::guest_ports::vm_guest_ports(&state.pool, &state.config, &id)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("invalid vm id") {
                ApiError::bad_request(msg)
            } else if msg.contains("vm not found") {
                ApiError::not_found(msg)
            } else {
                ApiError::internal(msg)
            }
        })
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ConnectivityBody {
    pub target_id: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

pub async fn connectivity_matrix(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ConnectivityBody>,
) -> Result<Json<machina_core::ConnectivityMatrix>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::guest_ports::connectivity_matrix(
        &state.pool,
        &state.config,
        &body.target_id,
        &body.profile,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
    .map(Json)
}

pub async fn compliance_export_pdf(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(kind): Path<String>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let bytes =
        zeus_firewall::compliance_pdf::export_compliance_pdf(&state.pool, &state.config, &kind)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    axum::response::Response::builder()
        .header("Content-Type", "application/pdf")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"zeus-firewall-{kind}.pdf\""),
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn anomalies(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::anomalies(&state.pool).await))
}

pub async fn baremetal_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::BaremetalFirewallOverview>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::metal_overview(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn baremetal_scan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let uid = uuid::Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    zeus_firewall::scan_exposure(&state.pool, uid, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MetalTemporaryBody {
    pub preset: String,
    pub reason: String,
    #[serde(default)]
    pub owner: Option<String>,
}

pub async fn baremetal_temporary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<MetalTemporaryBody>,
) -> Result<Json<zeus_firewall::TemporaryRule>, ApiError> {
    require_operator(&actor)?;
    let uid = uuid::Uuid::parse_str(&id).map_err(|e| ApiError::bad_request(e.to_string()))?;
    zeus_firewall::create_metal_temporary_preset(
        &state.pool,
        uid,
        &body.preset,
        &body.reason,
        body.owner.as_deref(),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn finops_exposure(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::finops::ExposureFinOpsReport>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::finops::exposure_rollup(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn finops_exposure_export_csv(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let csv = zeus_firewall::finops::export_csv(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    axum::response::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            http::header::CONTENT_DISPOSITION,
            "attachment; filename=\"zeus-firewall-exposure-cost.csv\"",
        )
        .body(axum::body::Body::from(csv))
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn multisite_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::multisite::MultisiteOverview>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::overview(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn multisite_export(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::multisite::FederatedExport>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::federated_export(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn multisite_drift(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::multisite::SiteDriftReport>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::site_drift_compare(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn multisite_connectivity(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::multisite::CrossSiteConnectivity>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::cross_site_connectivity(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn multisite_sync(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::multisite::MultisiteSyncRequest>,
) -> Result<Json<zeus_firewall::multisite::MultisiteSyncResult>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::cross_site_sync(&state.pool, &state.config, body, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MultisiteTimelineQuery {
    #[serde(default = "default_timeline_limit")]
    pub limit: i64,
}

fn default_timeline_limit() -> i64 {
    20
}

pub async fn multisite_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<MultisiteTimelineQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::merge_timeline(&state.pool, q.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn multisite_dr_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::multisite::DrTemplateBundle>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::multisite::dr_template_bundle(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn operator_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::operator::FleetSecurePlan>, ApiError> {
    require_operator(&actor)?;
    zeus_firewall::operator::fleet_secure_preview(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn operator_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::operator::OperatorExecuteRequest>,
) -> Result<Json<zeus_firewall::operator::OperatorExecuteResult>, ApiError> {
    require_operator(&actor)?;
    if body.force {
        // `force` skips the approval queue and applies live immediately —
        // the same class of bypass that fleet_rebalance::execute() gates
        // behind require_admin for its live-apply path, and that this file
        // already gates behind require_admin for lockdown/approve_change.
        // Plain operator role must not be able to skip Zeus approval gates.
        require_admin(&actor)?;
    }
    zeus_firewall::operator::execute_secure(&state.pool, &state.config, &body, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn operator_execute_batch(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<zeus_firewall::operator::OperatorBatchExecuteRequest>,
) -> Result<Json<zeus_firewall::operator::OperatorBatchExecuteResult>, ApiError> {
    require_operator(&actor)?;
    if body.force {
        // See operator_execute: force bypasses approval fleet-wide here, so
        // it needs the same admin escalation.
        require_admin(&actor)?;
    }
    zeus_firewall::operator::execute_secure_batch(
        &state.pool,
        &state.config,
        &body,
        &actor.username,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

pub async fn operator_thresholds(
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_firewall::operator::OperatorThresholds>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(zeus_firewall::operator::thresholds()))
}
