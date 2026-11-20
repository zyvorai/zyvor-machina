// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::ai::security as ai_security;
use crate::engine::ai::security_graph;
use crate::engine::bpf::{self, policies, telemetry};
use crate::engine::zeus_security;
use crate::state::AppState;

pub async fn status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_security::ZeusSecurityStatus>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(zeus_security::status(&state.pool).await))
}

pub async fn fleet_threat(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<zeus_security::FleetThreatSummary>, ApiError> {
    require_operator(&actor)?;
    zeus_security::fleet_threat(&state.pool, &state.config)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn sensors(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::sensors(&state.pool).await))
}

pub async fn asset_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::asset_inventory(&state.pool).await))
}

pub async fn fleet_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        zeus_security::fleet_timeline(&state.pool, q.hours.unwrap_or(24)).await,
    ))
}

pub async fn sync_alerts(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let n = zeus_security::sync_security_alerts(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        json!({ "inserted": n, "summary": format!("Synced {n} security alert(s) to notification outbox") }),
    ))
}

pub async fn correlations(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::correlations(&state.pool).await))
}

#[derive(Debug, Deserialize)]
pub struct L7Query {
    pub limit: Option<usize>,
    pub protocol: Option<String>,
}

pub async fn fleet_l7(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<L7Query>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(
        telemetry::l7(&state.pool, limit, q.protocol.as_deref()).await,
    ))
}

pub async fn fleet_accounting(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::accounting(&state.pool).await))
}

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    pub limit: Option<usize>,
}

/// Service LB / VM edge / sandbox / shield / node isolation / TLS state per host.
pub async fn fleet_native_dataplane(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::native_dataplane(&state.pool).await))
}

pub async fn fleet_tls_fingerprints(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(telemetry::tls_fingerprints(&state.pool, limit).await))
}

pub async fn fleet_rtnl_events(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(telemetry::rtnl_events(&state.pool, limit).await))
}

pub async fn fleet_guard_events(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(telemetry::guard_events(&state.pool, limit).await))
}

pub async fn fleet_vm_intel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::vm_intel(&state.pool).await))
}

pub async fn fleet_icmp_errors(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::icmp_errors(&state.pool).await))
}

pub async fn security_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<security_graph::SecurityGraph>, ApiError> {
    require_operator(&actor)?;
    security_graph::build_graph(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct HostQuery {
    pub hours: Option<u32>,
}

async fn resource(state: &AppState, id: Uuid, resource: &str, hours: u32) -> Json<Value> {
    Json(zeus_security::host_resource(&state.pool, &id.to_string(), resource, hours).await)
}

pub async fn host_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        zeus_security::host_summary(&state.pool, &id.to_string()).await,
    ))
}

pub async fn host_processes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "processes", q.hours.unwrap_or(24)).await)
}

pub async fn host_connections(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "connections", q.hours.unwrap_or(24)).await)
}

pub async fn host_dns(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "dns", q.hours.unwrap_or(24)).await)
}

pub async fn host_files(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "files", q.hours.unwrap_or(168)).await)
}

pub async fn host_ports(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "ports", 0).await)
}

pub async fn host_containers(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "containers", 0).await)
}

pub async fn host_timeline(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(resource(&state, id, "timeline", q.hours.unwrap_or(24)).await)
}

#[derive(Debug, Deserialize)]
pub struct ProcessGraphQuery {
    pub pid: Option<u64>,
}

pub async fn host_process_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<ProcessGraphQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let mut graph =
        zeus_security::host_resource(&state.pool, &id.to_string(), "process-graph", 0).await;
    if let Some(pid) = q.pid {
        graph["focus_pid"] = json!(pid);
    }
    Ok(Json(graph))
}

#[derive(Debug, Deserialize)]
pub struct SearchBody {
    pub query: String,
    pub host_id: Option<String>,
    pub limit: Option<usize>,
}

pub async fn search(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<SearchBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        telemetry::search(
            &state.pool,
            &body.query,
            body.host_id.as_deref(),
            body.limit.unwrap_or(200),
        )
        .await,
    ))
}

pub async fn fabric_health(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::fabric_health(&state.pool).await))
}

pub async fn hunt_queries(Extension(actor): Extension<AuthUser>) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(telemetry::hunt_queries()))
}

#[derive(Debug, Deserialize)]
pub struct HuntRunQuery {
    pub host_id: Option<String>,
}

pub async fn run_hunt_query(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(query_id): Path<String>,
    Query(q): Query<HuntRunQuery>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        telemetry::run_hunt(&state.pool, &query_id, q.host_id.as_deref()).await,
    ))
}

#[derive(Debug, Deserialize)]
pub struct ExplainEventBody {
    pub event: Value,
    pub host_id: Option<String>,
}

pub async fn explain_event(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ExplainEventBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    ai_security::explain_event(&state.pool, &body.event, body.host_id.as_deref())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AttackReconstructBody {
    pub host_id: String,
    pub hours: Option<u32>,
}

pub async fn attack_reconstruct(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<AttackReconstructBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let timeline = zeus_security::host_resource(
        &state.pool,
        &body.host_id,
        "timeline",
        body.hours.unwrap_or(24),
    )
    .await;
    ai_security::attack_reconstruct(&state.pool, &timeline)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct NlSearchBody {
    pub query: String,
    pub host_id: Option<String>,
}

pub async fn nl_search(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<NlSearchBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let (translated, llm_powered) =
        ai_security::translate_nl_search_async(&state.pool, &body.query).await;
    let results = telemetry::search(&state.pool, &translated, body.host_id.as_deref(), 200).await;
    let hits = results
        .get("hit_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(Json(json!({
        "original_query": body.query,
        "search_query": translated,
        "results": results,
        "hit_count": hits,
        "search_backend": telemetry::SOURCE,
        "llm_powered": llm_powered
    })))
}

#[derive(Debug, Deserialize)]
pub struct HuntSummaryBody {
    pub hours: Option<u32>,
}

pub async fn hunt_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<HuntSummaryBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let hours = body.hours.unwrap_or(48);
    let timeline = zeus_security::fleet_timeline(&state.pool, hours).await;
    let correlations = telemetry::correlations(&state.pool).await;
    ai_security::hunt_summary(&state.pool, &correlations, &timeline)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

// ---------------------------------------------------------------------------
// Runtime enforcement (native eBPF)
// ---------------------------------------------------------------------------

pub async fn enforcement_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(policies::enforcement_status(&state.pool).await))
}

pub async fn enforcement_policies(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(policies::enforcement_policies(&state.pool).await))
}

#[derive(Debug, Deserialize)]
pub struct CreateEnforcementPolicyBody {
    pub name: String,
    pub kind: String,
    pub r#match: String,
    pub enabled: Option<bool>,
    pub scope: Option<String>,
    pub host_ids: Option<Vec<String>>,
    pub description: Option<String>,
}

/// Result of a native operation; `ok: false` turns into a 400 so callers
/// can't mistake a rejected policy for an applied one.
fn native_result(native: Value, summary: String) -> Result<Json<Value>, ApiError> {
    if native.get("ok").and_then(|v| v.as_bool()) == Some(false) && native.get("error").is_some() {
        let msg = native["error"]
            .as_str()
            .unwrap_or("enforcement request rejected")
            .to_string();
        return Err(ApiError::bad_request(msg).with_code("enforcement_rejected"));
    }
    Ok(Json(json!({ "native": native, "summary": summary })))
}

pub async fn create_enforcement_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateEnforcementPolicyBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let payload = json!({
        "name": body.name,
        "kind": body.kind,
        "match": body.r#match,
        "enabled": body.enabled.unwrap_or(true),
        "scope": body.scope.unwrap_or_else(|| "fleet".into()),
        "host_ids": body.host_ids.unwrap_or_default(),
        "description": body.description.unwrap_or_default(),
    });
    let res = policies::create_enforcement_policy(&state.pool, &payload).await;
    native_result(res, format!("Enforcement policy {} created", body.name))
}

#[derive(Debug, Deserialize)]
pub struct ApplyEnforcementBody {
    #[serde(default)]
    pub host_ids: Vec<String>,
}

pub async fn apply_enforcement_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(policy_id): Path<String>,
    Json(body): Json<ApplyEnforcementBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let res =
        policies::apply_enforcement_policy(&state.pool, &state.config, &policy_id, &body.host_ids)
            .await;
    let summary = res["summary"].as_str().unwrap_or("").to_string();
    native_result(res, summary)
}

#[derive(Debug, Deserialize)]
pub struct PatchEnforcementPolicyBody {
    pub enabled: Option<bool>,
    pub r#match: Option<String>,
    pub description: Option<String>,
}

pub async fn patch_enforcement_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(policy_id): Path<String>,
    Json(body): Json<PatchEnforcementPolicyBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let payload = json!({
        "enabled": body.enabled,
        "match": body.r#match,
        "description": body.description,
    });
    let res = policies::patch_enforcement_policy(&state.pool, &policy_id, &payload).await;
    native_result(res, format!("Enforcement policy {policy_id} updated"))
}

pub async fn delete_enforcement_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(policy_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let res = policies::delete_enforcement_policy(&state.pool, &policy_id).await;
    native_result(res, format!("Enforcement policy {policy_id} deleted"))
}

pub async fn enforcement_policy_document(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(policy_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let doc = policies::policy_document(&state.pool, &policy_id).await;
    if doc.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(ApiError::not_found("enforcement policy not found"));
    }
    Ok(Json(doc))
}

pub async fn attach_enforcement(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    Ok(Json(
        policies::attach_enforcement(&state.pool, &state.config).await,
    ))
}

pub async fn sync_enforcement(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    Ok(Json(policies::sync_enforcement(&state.pool).await))
}

pub async fn detach_enforcement(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    Ok(Json(policies::detach_enforcement(&state.pool).await))
}

pub async fn fleet_sensors(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let native = telemetry::sensors(&state.pool).await;
    let sensors = native["sensors"].as_array().cloned().unwrap_or_default();
    let hosts: Vec<(Uuid, String, String)> =
        crate::db::query_as("SELECT id, hostname, state FROM hosts ORDER BY hostname")
            .fetch_all(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let matrix: Vec<Value> = hosts
        .iter()
        .map(|(id, hostname, host_state)| {
            let id_str = id.to_string();
            let sensor = sensors.iter().find(|s| s["host_id"] == json!(id_str));
            json!({
                "host_id": id_str,
                "hostname": hostname,
                "host_state": host_state,
                "sensor": sensor,
                "sensor_status": sensor.and_then(|s| s["status"].as_str()).unwrap_or("missing"),
            })
        })
        .collect();
    Ok(Json(json!({
        "sensors": sensors,
        "matrix": matrix,
        "summary": native["summary"],
    })))
}

pub async fn host_enforcement(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(policies::host_enforcement(&state.pool, &id).await))
}

pub async fn host_fabric_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let Some(host) = bpf::host(&state.pool, &id).await else {
        return Ok(Json(json!({
            "host_id": id,
            "agent_reachable": false,
            "message": "host not found or agent address missing",
        })));
    };
    match bpf::call(&host, &machina_bpf::api::Request::Status).await {
        Ok(status) => Ok(Json(
            json!({ "host_id": id, "agent_reachable": true, "fabric": status }),
        )),
        Err(e) => Ok(Json(
            json!({ "host_id": id, "agent_reachable": false, "message": format!("{e:#}") }),
        )),
    }
}

/// Admin passthrough to one host's machina-bpfd (interfaces, QoS, capture,
/// telemetry config). Streaming subscriptions are not proxied.
pub async fn host_bpf_call(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(req): Json<machina_bpf::api::Request>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    if matches!(req, machina_bpf::api::Request::Subscribe { .. }) {
        return Err(ApiError::bad_request(
            "subscribe is not supported over this endpoint",
        ));
    }
    let host = bpf::host(&state.pool, &id)
        .await
        .ok_or_else(|| ApiError::not_found("host not found"))?;
    bpf::call(&host, &req)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(format!("{e:#}")).with_code("bpfd_error"))
}
