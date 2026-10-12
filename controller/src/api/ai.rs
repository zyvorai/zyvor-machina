// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Extension;
use axum::Json;
use futures_util::stream::Stream;
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::ai;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SpotlightBody {
    pub query: String,
}

pub async fn get_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::settings::AiSettings>, ApiError> {
    require_operator(&actor)?;
    ai::settings::get_ai_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::settings::AiSettingsPatch>,
) -> Result<Json<ai::settings::AiSettings>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::settings::patch_ai_settings(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn spotlight(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<SpotlightBody>,
) -> Result<Json<ai::SpotlightResult>, ApiError> {
    require_operator(&actor)?;
    // Cap query length (as copilot_chat does): `route_spotlight` runs ~150 sequential
    // substring scans plus NL parsers over the query, so an unbounded body is an
    // asymmetric CPU cost. 32 KiB is far beyond any real spotlight query.
    if body.query.len() > 32_768 {
        return Err(ApiError::bad_request(
            "spotlight query too long (max 32768 bytes)",
        ));
    }
    let online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let q = body.query.trim();
    let mut hits = Vec::new();
    if !q.is_empty() {
        let vms: Vec<(Uuid, String, String)> = crate::db::query_as(
            "SELECT id, name, observed_state FROM vms WHERE name LIKE ? ORDER BY name LIMIT 12",
        )
        .bind(format!("%{q}%"))
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
        for (id, name, st) in vms {
            hits.push(ai::SearchHit {
                kind: "vm".into(),
                id: id.to_string(),
                label: name,
                sublabel: Some(st),
            });
        }
    }

    Ok(Json(ai::intent_router::route_spotlight(
        &body.query,
        online,
        hits,
    )))
}

pub async fn jarvis_landing(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::SpotlightResult>, ApiError> {
    require_operator(&actor)?;
    let online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let missing = crate::engine::template_readiness::list_missing_marketplace_images(&state.pool)
        .await
        .unwrap_or_default();
    Ok(Json(ai::intent_router::jarvis_landing_intents(
        online, missing,
    )))
}

#[derive(Debug, Deserialize)]
pub struct CopilotBody {
    pub message: String,
    pub vm_id: Option<Uuid>,
    pub host_id: Option<Uuid>,
    #[serde(default)]
    pub vm_ids: Option<Vec<Uuid>>,
}

pub async fn copilot_chat(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CopilotBody>,
) -> Result<Json<ai::CopilotResponse>, ApiError> {
    require_operator(&actor)?;
    if body.message.len() > 32_768 {
        return Err(ApiError::bad_request("message too long (max 32 768 chars)"));
    }
    ai::copilot_chat(
        &state.pool,
        &state.config,
        &body.message,
        body.vm_id,
        body.host_id,
        body.vm_ids,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
    .map(Json)
}

pub async fn copilot_stream(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CopilotBody>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    if require_operator(&actor).is_err() {
        let (tx1, rx1) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(1);
        let _ = tx1.try_send(Ok(Event::default()
            .data(serde_json::json!({"type":"error","message":"Forbidden"}).to_string())));
        return Sse::new(ReceiverStream::new(rx1))
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    }
    if body.message.len() > 32_768 {
        let _ = tx.try_send(Ok(Event::default().data(
            serde_json::json!({"type":"error","message":"message too long"}).to_string(),
        )));
        return Sse::new(ReceiverStream::new(rx))
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    }
    if let Some(prompt) = machina_bpf::netpol::nl::policy_request(&body.message) {
        let prompt: String = prompt
            .chars()
            .take(machina_bpf::netpol::nl::MAX_PROMPT)
            .collect();
        tokio::spawn(async move {
            let d = crate::api::vm_network_policies::compose(&state, &prompt, false).await;
            for chunk in ai::chunk_text(&machina_bpf::netpol::nl::chat_reply(&d), 48) {
                let payload = serde_json::json!({ "type": "chunk", "text": chunk }).to_string();
                let _ = tx.send(Ok(Event::default().data(payload))).await;
            }
            let done = serde_json::json!({
                "type": "done",
                "deterministic": d.source != "llm",
                "context_summary": "VM network policy draft",
            })
            .to_string();
            let _ = tx.send(Ok(Event::default().data(done))).await;
        });
        return Sse::new(ReceiverStream::new(rx))
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    }
    let pool = state.pool.clone();
    let config = state.config.clone();
    let message = body.message;
    let vm_id = body.vm_id;
    let host_id = body.host_id;
    let vm_ids = body.vm_ids;

    tokio::spawn(async move {
        let send = |data: String| async {
            let _ = tx.send(Ok(Event::default().data(data))).await;
        };

        match ai::build_copilot_base(&pool, &config, &message, vm_id, host_id, vm_ids).await {
            Ok(base) => {
                for chunk in ai::chunk_text(&base.reply, 48) {
                    let payload = serde_json::json!({ "type": "chunk", "text": chunk }).to_string();
                    send(payload).await;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }

                let system = format!(
                    "You are Zyra, an autonomous infrastructure engineer and cloud architect. Be concise. Use bullet points. {}",
                    ai::guest_tools::tools_system_prompt()
                );
                let mut deterministic = true;
                if let Ok(Some(llm_text)) = ai::llm::complete_simple(
                    &pool,
                    &system,
                    &format!("Context: {}\nUser: {}", base.ctx_json, message),
                )
                .await
                {
                    deterministic = false;
                    send(serde_json::json!({ "type": "chunk", "text": "\n\n" }).to_string()).await;
                    for chunk in ai::chunk_text(&llm_text, 48) {
                        let payload =
                            serde_json::json!({ "type": "chunk", "text": chunk }).to_string();
                        send(payload).await;
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }

                let done = serde_json::json!({
                    "type": "done",
                    "deterministic": deterministic,
                    "context_summary": base.context_summary,
                })
                .to_string();
                send(done).await;
            }
            Err(e) => {
                let err =
                    serde_json::json!({ "type": "error", "message": e.to_string() }).to_string();
                send(err).await;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Debug, Deserialize)]
pub struct ExplainBody {
    pub screen: String,
    #[serde(default)]
    pub object_ref: serde_json::Value,
}

pub async fn explain(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ExplainBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let text = ai::explain_screen(&state.pool, &body.screen, &body.object_ref)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "explanation": text })))
}

#[derive(Debug, Deserialize)]
pub struct RunbookBody {
    pub incident: String,
    #[serde(default)]
    pub context: serde_json::Value,
}

pub async fn runbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<RunbookBody>,
) -> Result<Json<ai::runbook::Runbook>, ApiError> {
    require_operator(&actor)?;
    ai::runbook::generate(&state.pool, &body.incident, &body.context)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct BlueprintGenBody {
    pub prompt: String,
}

pub async fn generate_blueprint(
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BlueprintGenBody>,
) -> Result<Json<ai::blueprint::GeneratedBlueprint>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(ai::blueprint::generate_from_nl(&body.prompt)))
}

pub async fn cost_guardian(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::cost::CostAnalysis>, ApiError> {
    require_operator(&actor)?;
    ai::cost::analyze(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn capacity_planner(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::capacity::CapacityPlan>, ApiError> {
    require_operator(&actor)?;
    ai::capacity::plan(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn security_sentinel(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::security::SecurityReport>, ApiError> {
    require_operator(&actor)?;
    ai::security::scan(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct NetworkExplainBody {
    pub vm_a: String,
    pub vm_b: String,
    pub port: Option<i32>,
}

pub async fn policy_export(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::policy_export::PolicyExport>, ApiError> {
    require_operator(&actor)?;
    ai::policy_export::export_policy_yaml(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn network_explain(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<NetworkExplainBody>,
) -> Result<Json<ai::network::NetworkExplainResult>, ApiError> {
    require_operator(&actor)?;
    ai::network::explain_reach(
        &state.pool,
        &state.config,
        &body.vm_a,
        &body.vm_b,
        body.port,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
    .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MigrationAdvisorQuery {
    pub vm: String,
    pub os: Option<String>,
    pub disk_path: Option<String>,
    pub host_id: Option<String>,
    #[serde(default)]
    pub has_rdm: bool,
}

pub async fn migration_advisor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<MigrationAdvisorQuery>,
) -> Result<Json<ai::migration::MigrationAdvisorReport>, ApiError> {
    require_operator(&actor)?;
    // Score with the caller-supplied os/has_rdm. Previously the non-vmware branch
    // hardcoded "linux"/false, so ?provider=hyperv&os=windows&has_rdm=true was
    // advised against wrong inputs (both branches call the same advisor).
    let os = q.os.as_deref().unwrap_or("linux");
    let mut report = ai::migration::advise_vmware_vm(&q.vm, os, q.has_rdm);

    if let Some(disk_path) = q.disk_path.filter(|p| !p.is_empty()) {
        if state.config.guestkit_enabled {
            if let Ok(plan) =
                crate::engine::guestkit_bridge::migrate_plan_disk(&state.config, &disk_path, "kvm")
                    .await
            {
                if let Ok(doc) = crate::engine::guestkit_bridge::doctor_disk(
                    &state.config,
                    &disk_path,
                    "kvm",
                    false,
                )
                .await
                {
                    report = ai::migration::merge_guestkit(
                        report,
                        doc.boot_score,
                        plan.migration_score,
                        &doc.blockers,
                        &doc.warnings,
                        &plan.summary,
                    );
                }
            }
        }
    }

    if let Some(host_id) = q.host_id.filter(|p| !p.is_empty()) {
        if let Ok(detail) =
            crate::engine::zeus_firewall::target_detail(&state.pool, &state.config, &host_id).await
        {
            let deps: Vec<String> = detail
                .inventory
                .open_ports
                .iter()
                .map(|p| format!("{}:{}", p.service_name, p.port))
                .collect();
            report = ai::migration::merge_firewall_migration(report, deps);
        }
    } else {
        let inferred = infer_migration_firewall_deps(&q.vm, q.os.as_deref().unwrap_or("linux"));
        if !inferred.is_empty() {
            report = ai::migration::merge_firewall_migration(report, inferred);
        }
    }

    Ok(Json(report))
}

fn infer_migration_firewall_deps(vm: &str, os: &str) -> Vec<String> {
    let vm_l = vm.to_lowercase();
    let mut deps = vec!["ssh:22".into()];
    if vm_l.contains("db") || vm_l.contains("postgres") || vm_l.contains("mysql") {
        deps.push("postgresql:5432".into());
        deps.push("mysql:3306".into());
    }
    if vm_l.contains("web") || vm_l.contains("nginx") || vm_l.contains("apache") {
        deps.push("http:80".into());
        deps.push("https:443".into());
    }
    if vm_l.contains("ldap") || vm_l.contains("ad") {
        deps.push("ldap:389".into());
    }
    if vm_l.contains("erp") || vm_l.contains("oracle") {
        deps.push("oracle:1521".into());
        deps.push("smtp:25".into());
    }
    if os.to_lowercase().contains("windows") {
        deps.push("rdp:3389".into());
    }
    deps
}

pub async fn vm_doctor(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<crate::engine::vm_health::VmHealthReport>, ApiError> {
    require_operator(&actor)?;
    crate::engine::vm_health::run_vm_health_check(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AutopilotProposeQuery {
    pub vm_id: Option<Uuid>,
}

pub async fn performance_autopilot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ai::performance_autopilot::PerformanceAutopilotQuery>,
) -> Result<Json<ai::performance_autopilot::PerformanceAutopilotReport>, ApiError> {
    require_operator(&actor)?;
    ai::performance_autopilot::analyze(&state.pool, &q)
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn autopilot_propose(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<AutopilotProposeQuery>,
) -> Result<Json<ai::autopilot::AutopilotProposal>, ApiError> {
    require_operator(&actor)?;
    ai::autopilot::propose(&state.pool, q.vm_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn autopilot_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::autopilot::ExecuteBody>,
) -> Result<Json<ai::autopilot::ExecuteResult>, ApiError> {
    require_operator(&actor)?;
    ai::autopilot::execute(&state, &actor, &body)
        .await
        .map(Json)
}

pub async fn compliance_report(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::compliance::ComplianceReport>, ApiError> {
    require_operator(&actor)?;
    ai::compliance::generate(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn compliance_export_html(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Html<String>, ApiError> {
    require_operator(&actor)?;
    let report = ai::compliance::generate(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(axum::response::Html(ai::compliance::report_to_html(
        &report,
    )))
}

pub async fn compliance_export_pdf(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let report = ai::compliance::generate(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let bytes = ai::compliance::report_to_pdf(&report);
    axum::response::Response::builder()
        .header(http::header::CONTENT_TYPE, "application/pdf")
        .header(
            http::header::CONTENT_DISPOSITION,
            "attachment; filename=\"machina-compliance-report.pdf\"",
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct TerminalSuggestBody {
    pub vm_id: Option<Uuid>,
    pub vm_name: Option<String>,
}

pub async fn terminal_suggest(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<TerminalSuggestBody>,
) -> Result<Json<ai::terminal::TerminalSuggestResult>, ApiError> {
    require_operator(&actor)?;
    ai::terminal::suggest(&state.pool, body.vm_id, body.vm_name.as_deref())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AutopilotRunBody {
    #[serde(default)]
    pub vm_id: Option<Uuid>,
    #[serde(default = "default_max_actions")]
    pub max_actions: usize,
}

fn default_max_actions() -> usize {
    3
}

pub async fn autopilot_run(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<AutopilotRunBody>,
) -> Result<Json<ai::autopilot::AutopilotRunResult>, ApiError> {
    require_operator(&actor)?;
    let settings = ai::settings::get_ai_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let cap = settings.autopilot_max_actions.clamp(1, 10) as usize;
    let max = if body.max_actions == default_max_actions() {
        cap
    } else {
        body.max_actions.min(cap)
    };
    ai::autopilot::run_safe_batch(&state, &actor, body.vm_id, max)
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct AutopilotHistoryQuery {
    #[serde(default = "default_history_limit")]
    pub limit: i64,
}

fn default_history_limit() -> i64 {
    20
}

pub async fn autopilot_history(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<AutopilotHistoryQuery>,
) -> Result<Json<Vec<ai::autopilot::AutopilotHistoryEntry>>, ApiError> {
    require_operator(&actor)?;
    ai::autopilot::list_history(&state.pool, q.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn capacity_export_csv(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let csv = ai::capacity::export_csv(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    axum::response::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            http::header::CONTENT_DISPOSITION,
            "attachment; filename=\"machina-capacity-planner.csv\"",
        )
        .body(axum::body::Body::from(csv))
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn cost_export_csv(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let csv = ai::cost::export_csv(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    axum::response::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            http::header::CONTENT_DISPOSITION,
            "attachment; filename=\"machina-cost-guardian.csv\"",
        )
        .body(axum::body::Body::from(csv))
        .map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn fleet_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::fleet_summary::FleetZyraSummary>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_summary::summarize(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn fleet_local(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::fleet_summary::FleetClusterSlice>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_summary::local_export(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn twin_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::digital_twin::DigitalTwinGraph>, ApiError> {
    require_operator(&actor)?;
    ai::digital_twin::build_graph(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn twin_impact(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::digital_twin::ImpactRequest>,
) -> Result<Json<ai::digital_twin::ImpactAnalysis>, ApiError> {
    require_operator(&actor)?;
    ai::digital_twin::analyze_impact(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn noisy_neighbors(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ai::noisy_neighbor::NoisyNeighborQuery>,
) -> Result<Json<ai::noisy_neighbor::NoisyNeighborReport>, ApiError> {
    require_operator(&actor)?;
    ai::noisy_neighbor::analyze(&state.pool, &q)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn analyze_blackbox_incident(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ai::blackbox_rca::BlackBoxRcaQuery>,
) -> Result<Json<ai::blackbox_rca::BlackBoxRcaReport>, ApiError> {
    require_operator(&actor)?;
    ai::blackbox_rca::analyze(&state.pool, &q)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn analyze_incident(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ai::root_cause::AnalyzeIncidentQuery>,
) -> Result<Json<ai::root_cause::IncidentAnalysis>, ApiError> {
    require_operator(&actor)?;
    let mut result = ai::root_cause::analyze(&state.pool, &q)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let pw = crate::engine::bpf::telemetry::anomalies(&state.pool).await;
    if pw
        .get("anomalies")
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty())
    {
        ai::root_cause::merge_bpf_anomalies(&mut result.timeline, &pw);
    }
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct EnvironmentIntentBody {
    pub query: String,
}

pub async fn vm_builder(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::vm_builder::VmBuilderBody>,
) -> Result<Json<ai::vm_builder::VmBuilderResult>, ApiError> {
    require_operator(&actor)?;
    ai::vm_builder::build(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn intent_environment(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<EnvironmentIntentBody>,
) -> Result<Json<ai::environment_intent::EnvironmentResourcePlan>, ApiError> {
    require_operator(&actor)?;
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .ok_or_else(|| ApiError::bad_request("no cluster configured — run machina-controller bootstrap"))?;
    Ok(Json(ai::environment_intent::plan_environment(
        &body.query,
        rates.0,
        rates.1,
    )))
}

pub async fn intent_environment_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::environment_intent::EnvironmentExecuteBody>,
) -> Result<Json<ai::environment_intent::EnvironmentExecuteResult>, ApiError> {
    require_operator(&actor)?;
    ai::environment_intent::execute_environment(&state, &actor, &body)
        .await
        .map(Json)
}

pub async fn sre_forecast(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::sre_predict::SreForecastReport>, ApiError> {
    require_operator(&actor)?;
    ai::sre_predict::forecast(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn sre_remediate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::sre_remediate::SreRemediationReport>, ApiError> {
    require_operator(&actor)?;
    ai::sre_remediate::propose(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn compliance_remediate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::compliance_remediate::ComplianceRemediationReport>, ApiError> {
    require_operator(&actor)?;
    ai::compliance_remediate::propose(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::zyra_summary::ZyraOsSummary>, ApiError> {
    require_operator(&actor)?;
    ai::zyra_summary::summarize(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn fleet_power_optimize(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::fleet_power::FleetPowerReport>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_power::optimize(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn fleet_heatmap(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::fleet_heatmap::FleetHeatmap>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_heatmap::heatmap(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct RebalanceQuery {
    #[serde(default = "default_rebalance_max")]
    pub max_moves: usize,
}

fn default_rebalance_max() -> usize {
    5
}

pub async fn fleet_rebalance_propose(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<RebalanceQuery>,
) -> Result<Json<ai::fleet_rebalance::RebalanceProposal>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_rebalance::propose(&state.pool, q.max_moves)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn fleet_rebalance_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::fleet_rebalance::RebalanceExecuteBody>,
) -> Result<Json<ai::fleet_rebalance::RebalanceExecuteResult>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_rebalance::execute(&state, &actor, &body)
        .await
        .map(Json)
}

pub async fn cost_attribution(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::cost_attribution::CostAttributionReport>, ApiError> {
    require_operator(&actor)?;
    ai::cost_attribution::attribute(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn compliance_frameworks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::compliance_frameworks::ComplianceFrameworksReport>, ApiError> {
    require_operator(&actor)?;
    ai::compliance_frameworks::scan(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn security_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::security_graph::SecurityGraph>, ApiError> {
    require_operator(&actor)?;
    ai::security_graph::build_graph(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn security_attack_path(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::security_graph::AttackPathQuery>,
) -> Result<Json<ai::security_graph::AttackPathResult>, ApiError> {
    require_operator(&actor)?;
    ai::security_graph::attack_path(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct KnowledgeSearchBody {
    pub query: String,
}

pub async fn knowledge_search(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<KnowledgeSearchBody>,
) -> Result<Json<ai::knowledge_search::KnowledgeSearchResult>, ApiError> {
    require_operator(&actor)?;
    ai::knowledge_search::search(&state.pool, &body.query)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn service_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::service_graph::ServiceGraph>, ApiError> {
    require_operator(&actor)?;
    ai::service_graph::build(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MemoryQuery {
    #[serde(default = "default_memory_limit")]
    pub limit: i64,
}

fn default_memory_limit() -> i64 {
    20
}

pub async fn infrastructure_memory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<MemoryQuery>,
) -> Result<Json<ai::infrastructure_memory::InfrastructureMemory>, ApiError> {
    require_operator(&actor)?;
    ai::infrastructure_memory::recall(&state.pool, q.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MissionStackBody {
    pub query: String,
}

pub async fn mission_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<MissionStackBody>,
) -> Result<Json<ai::mission_stack::MissionStackPlan>, ApiError> {
    require_operator(&actor)?;
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .ok_or_else(|| ApiError::bad_request("no cluster configured — run machina-controller bootstrap"))?;
    Ok(Json(ai::mission_stack::plan_mission_stack(
        &body.query,
        rates.0,
        rates.1,
    )))
}

pub async fn mission_stack_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::mission_stack::MissionStackExecuteBody>,
) -> Result<Json<ai::mission_stack::MissionStackExecuteResult>, ApiError> {
    require_operator(&actor)?;
    ai::mission_stack::execute_stack(&state, &actor, &body)
        .await
        .map(Json)
}

pub async fn cost_attribution_export_csv(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<axum::response::Response, ApiError> {
    require_operator(&actor)?;
    let csv = ai::cost_attribution::export_csv(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    axum::response::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            http::header::CONTENT_DISPOSITION,
            "attachment; filename=\"machina-cost-attribution.csv\"",
        )
        .body(axum::body::Body::from(csv))
        .map_err(|e| ApiError::internal(e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct GpuPlacementQuery {
    #[serde(default = "default_gpu_workload")]
    pub workload: String,
}

fn default_gpu_workload() -> String {
    "inference".into()
}

pub async fn fleet_gpu_placement(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<GpuPlacementQuery>,
) -> Result<Json<ai::fleet_placement::GpuPlacementReport>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_placement::advise_gpu(&state.pool, &q.workload)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn knowledge_diagnose(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<KnowledgeSearchBody>,
) -> Result<Json<ai::knowledge_diagnose::KnowledgeDiagnosis>, ApiError> {
    require_operator(&actor)?;
    ai::knowledge_diagnose::diagnose(&state.pool, &body.query)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn service_impact(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::service_impact::ServiceImpactQuery>,
) -> Result<Json<ai::service_impact::ServiceImpactResult>, ApiError> {
    require_operator(&actor)?;
    ai::service_impact::simulate(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct SimilarMemoryQuery {
    pub q: String,
    #[serde(default = "default_memory_limit")]
    pub limit: i64,
}

pub async fn memory_similar(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<SimilarMemoryQuery>,
) -> Result<Json<ai::infrastructure_memory::SimilarIncidentsResult>, ApiError> {
    require_operator(&actor)?;
    ai::infrastructure_memory::similar(&state.pool, &q.q, q.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn remediate_hub(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::remediate_hub::RemediateHub>, ApiError> {
    require_operator(&actor)?;
    ai::remediate_hub::hub(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn knowledge_runbook(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<KnowledgeSearchBody>,
) -> Result<Json<ai::knowledge_runbook::KnowledgeRunbook>, ApiError> {
    require_operator(&actor)?;
    ai::knowledge_runbook::from_query(&state.pool, &body.query)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn cost_budget(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::cost_budget::CostBudgetReport>, ApiError> {
    require_operator(&actor)?;
    ai::cost_budget::analyze(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn mission_stack_status(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::mission_stack_status::MissionStackStatus>, ApiError> {
    require_operator(&actor)?;
    ai::mission_stack_status::status(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

// --- Zyra AI redesign APIs ---

pub async fn list_ai_providers(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::providers::AiProviderRow>>, ApiError> {
    require_operator(&actor)?;
    ai::providers::list_providers(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

/// Provider kinds that were removed with the local-first change must be a clean 400, not a 500.
fn reject_removed_provider_kind(kind: Option<&str>) -> Result<(), ApiError> {
    if kind.map(str::trim) == Some("azure_openai") {
        return Err(ApiError::bad_request(
            "azure_openai is no longer supported (local-first); use ollama, vllm or openai_compatible",
        )
        .with_code("provider_kind_removed"));
    }
    Ok(())
}

pub async fn create_ai_provider(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::providers::CreateProviderBody>,
) -> Result<Json<ai::providers::AiProviderRow>, ApiError> {
    crate::auth::require_admin(&actor)?;
    reject_removed_provider_kind(Some(body.kind.as_str()))?;
    ai::providers::create_provider(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_ai_provider(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<ai::providers::PatchProviderBody>,
) -> Result<Json<ai::providers::AiProviderRow>, ApiError> {
    crate::auth::require_admin(&actor)?;
    reject_removed_provider_kind(body.kind.as_deref())?;
    ai::providers::patch_provider(&state.pool, id, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn delete_ai_provider(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_admin(&actor)?;
    let ok = ai::providers::delete_provider(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Report the true outcome instead of an unconditional 200: a nonexistent
    // id previously still came back as {"deleted": false} with a success
    // status, masking the no-op from the caller.
    if !ok {
        return Err(ApiError::not_found("provider not found"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn list_ai_provider_models(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Vec<ai::providers::AiModelRow>>, ApiError> {
    require_operator(&actor)?;
    ai::providers::list_models(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn test_ai_provider(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::providers::test_provider(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn list_routing_rules(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::routing::RoutingRuleRow>>, ApiError> {
    require_operator(&actor)?;
    ai::routing::list_rules(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_routing_rule(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(task_class): axum::extract::Path<String>,
    Json(body): Json<ai::routing::PatchRoutingRuleBody>,
) -> Result<Json<ai::routing::RoutingRuleRow>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::routing::patch_rule(&state.pool, &task_class, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn list_zyra_agents(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::agents::ZyraAgentInfo>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(ai::agents::catalog()))
}

pub async fn zyra_chat(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::agents::ZyraChatBody>,
) -> Result<Json<ai::agents::ZyraChatResponse>, ApiError> {
    require_operator(&actor)?;
    ai::agents::chat(&state.pool, &state.config, &body, Some(&actor.username))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn list_ai_prompts(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::prompts::PromptRow>>, ApiError> {
    require_operator(&actor)?;
    ai::prompts::list_prompts(&state.pool, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn create_ai_prompt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::prompts::CreatePromptBody>,
) -> Result<Json<ai::prompts::PromptRow>, ApiError> {
    require_operator(&actor)?;
    ai::prompts::create_prompt(&state.pool, &actor.username, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_ai_prompt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<ai::prompts::PatchPromptBody>,
) -> Result<Json<ai::prompts::PromptRow>, ApiError> {
    require_operator(&actor)?;
    ai::prompts::patch_prompt(&state.pool, id, &actor.username, &actor.role, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn delete_ai_prompt(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let ok = ai::prompts::delete_prompt(&state.pool, id, &actor.username, &actor.role)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Report the true outcome instead of an unconditional 200: a nonexistent
    // (or not-owned) id previously still came back as {"deleted": false}
    // with a success status, masking the no-op from the caller.
    if !ok {
        return Err(ApiError::not_found("prompt not found or not owned by you"));
    }
    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn get_memory_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::memory_store::MemorySettings>, ApiError> {
    require_operator(&actor)?;
    ai::memory_store::get_settings(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_memory_settings(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::memory_store::MemorySettingsPatch>,
) -> Result<Json<ai::memory_store::MemorySettings>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::memory_store::patch_settings(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MemoryPurgeQuery {
    pub scope: String,
}

pub async fn purge_memory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<MemoryPurgeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_admin(&actor)?;
    let deleted = ai::memory_store::purge(&state.pool, &q.scope, Some(&actor.username))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn fleet_guest_query(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::fleet_guest_query::FleetGuestQueryRequest>,
) -> Result<Json<ai::fleet_guest_query::FleetGuestQueryReport>, ApiError> {
    require_operator(&actor)?;
    ai::fleet_guest_query::execute(&state.pool, &state.config, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn migration_readiness_report(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::migration_readiness::MigrationReadinessRequest>,
) -> Result<Json<ai::migration_readiness::MigrationReadinessReport>, ApiError> {
    require_operator(&actor)?;
    ai::migration_readiness::generate(&state.pool, &state.config, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_approval_hub(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::actions::approval_hub(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn create_zyra_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::actions::CreateActionBody>,
) -> Result<Json<ai::actions::ZyraActionRow>, ApiError> {
    require_operator(&actor)?;
    ai::actions::create_action(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn execute_zyra_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::actions::approve_and_execute(&state, id, &actor)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, serde::Deserialize)]
pub struct AgentRunBody {
    pub prompt: String,
}

/// Run the tool-calling Zyra agent. It can look at the fleet and queue proposals for approval; it can
/// never execute a change itself.
pub async fn run_zyra_agent(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<AgentRunBody>,
) -> Result<Json<ai::agent_loop::AgentRun>, ApiError> {
    require_operator(&actor)?;
    if body.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }
    ai::agent_loop::run(
        &state,
        &actor.username,
        Some(actor.username.clone()),
        &body.prompt,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))
    .map(Json)
}

/// Like `run_zyra_agent`, but streams each step as it happens (server-sent events): `{"type":"step",...}` per
/// tool call/result, then `{"type":"done","run":{...}}` or `{"type":"error","message":"..."}`.
pub async fn run_zyra_agent_stream(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<AgentRunBody>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_operator(&actor)?;
    if body.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        let step_tx = tx.clone();
        let on_step = move |s: &ai::agent_loop::AgentStep| {
            let mut v = serde_json::to_value(s).unwrap_or_default();
            v["type"] = "step".into();
            let _ = step_tx.try_send(Ok(Event::default().data(v.to_string())));
        };
        let res = ai::agent_loop::run_streaming(
            &state,
            &actor.username,
            Some(actor.username.clone()),
            &body.prompt,
            &on_step,
        )
        .await;
        let payload = match res {
            Ok(run) => serde_json::json!({"type": "done", "run": run}),
            Err(e) => serde_json::json!({"type": "error", "message": e.to_string()}),
        };
        let _ = tx.send(Ok(Event::default().data(payload.to_string()))).await;
    });
    Ok(Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub async fn zyra_action_history(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::action_audit::ActionHistoryRow>>, ApiError> {
    require_operator(&actor)?;
    ai::action_audit::history(&state, 50)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn verify_zyra_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::action_audit::verify(&state, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn undo_zyra_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::action_audit::undo(&state, id, &actor)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn reject_zyra_action(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let ok = ai::actions::reject(&state.pool, id, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "rejected": ok })))
}

pub async fn list_agent_marketplace(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::agent_marketplace::AgentPluginRow>>, ApiError> {
    require_operator(&actor)?;
    ai::agent_marketplace::list_agents(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn install_agent_marketplace(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> Result<Json<ai::agent_marketplace::AgentPluginRow>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::agent_marketplace::install(&state.pool, &slug)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn uninstall_agent_marketplace(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(slug): axum::extract::Path<String>,
) -> Result<Json<ai::agent_marketplace::AgentPluginRow>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::agent_marketplace::uninstall(&state.pool, &slug)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_enterprise_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::enterprise_zyra::ZyraEnterpriseOverview>, ApiError> {
    require_operator(&actor)?;
    ai::enterprise_zyra::overview(&state.pool, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn patch_zyra_enterprise_overview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::enterprise_zyra::ZyraEnterprisePatch>,
) -> Result<Json<ai::enterprise_zyra::ZyraEnterpriseOverview>, ApiError> {
    require_admin(&actor)?;
    ai::enterprise_zyra::require_zyra_admin(&actor)?;
    ai::enterprise_zyra::patch(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ai::enterprise_zyra::overview(&state.pool, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_autonomous_plan(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::autonomous::AutonomousPlanBody>,
) -> Result<Json<ai::autonomous::AutonomousPlanResult>, ApiError> {
    require_operator(&actor)?;
    ai::autonomous::plan(&state.pool, &state.config, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_autonomous_execute(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::autonomous::AutonomousExecuteBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::autonomous::execute_approved_plan(&state.pool, &state.config, &state, &actor, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

// --- Infrastructure Graph Brain (AI-138) ---

#[derive(Debug, Deserialize)]
pub struct GraphScopeQuery {
    pub host_id: Option<Uuid>,
    pub vm_id: Option<Uuid>,
}

pub async fn infra_graph(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<GraphScopeQuery>,
) -> Result<Json<ai::infra_graph::InfraGraph>, ApiError> {
    require_operator(&actor)?;
    ai::infra_graph::build_enriched(
        &state.pool,
        &state.config,
        &ai::infra_graph::GraphScope {
            host_id: q.host_id,
            vm_id: q.vm_id,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))
    .map(Json)
}

pub async fn infra_graph_path(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::infra_graph::PathRequest>,
) -> Result<Json<ai::infra_graph::PathResult>, ApiError> {
    require_operator(&actor)?;
    ai::infra_graph::explain_path(&state.pool, &state.config, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn infra_graph_query(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::infra_graph::GraphQueryRequest>,
) -> Result<Json<ai::infra_graph::GraphQueryResult>, ApiError> {
    require_operator(&actor)?;
    ai::infra_graph::query(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn infra_graph_object(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path((kind, id)): axum::extract::Path<(String, String)>,
) -> Result<Json<ai::infra_graph::ObjectExplain>, ApiError> {
    require_operator(&actor)?;
    ai::infra_graph::explain_object(&state.pool, &kind, &id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn infra_graph_at(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(ts): axum::extract::Path<String>,
) -> Result<Json<ai::infra_graph::GraphAtTime>, ApiError> {
    require_operator(&actor)?;
    let parsed = chrono::DateTime::parse_from_rfc3339(&ts)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    ai::infra_graph::graph_at(&state.pool, parsed)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct TimelineReplayQuery {
    pub from: String,
    pub to: String,
    pub resource: Option<String>,
}

pub async fn timeline_replay(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<TimelineReplayQuery>,
) -> Result<Json<ai::infra_graph::TimelineReplay>, ApiError> {
    require_operator(&actor)?;
    let from = chrono::DateTime::parse_from_rfc3339(&q.from)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let to = chrono::DateTime::parse_from_rfc3339(&q.to)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    ai::infra_graph::timeline_replay(&state.pool, from, to, q.resource.as_deref())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn analyze_incident_post(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::root_cause::AnalyzeIncidentBody>,
) -> Result<Json<ai::root_cause::IncidentAnalysis>, ApiError> {
    require_operator(&actor)?;
    let mut result = ai::root_cause::analyze_post(&state.pool, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let pw = crate::engine::bpf::telemetry::anomalies(&state.pool).await;
    if pw
        .get("anomalies")
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty())
    {
        ai::root_cause::merge_bpf_anomalies(&mut result.timeline, &pw);
    }
    let _ = ai::memory_store::remember(
        &state.pool,
        &actor.username,
        "incident",
        &result.root_cause,
        &format!("RCA: {}", result.root_cause),
        None,
    )
    .await;
    let _ = ai::incident_commander::create(
        &state.pool,
        &ai::incident_commander::CreateIncidentRequest {
            title: "Infrastructure incident".into(),
            summary: result.root_cause.clone(),
            severity: if result.confidence >= 0.7 {
                "high".into()
            } else {
                "medium".into()
            },
            affected_resources: result.contributing_factors.clone(),
            root_cause: Some(result.root_cause.clone()),
            window_start: Some(chrono::Utc::now() - chrono::Duration::hours(body.hours as i64)),
            window_end: Some(chrono::Utc::now()),
        },
    )
    .await;
    Ok(Json(result))
}

pub async fn troubleshoot_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::troubleshoot::TroubleshootRequest>,
) -> Result<Json<ai::troubleshoot::DiagnosisReport>, ApiError> {
    require_operator(&actor)?;
    ai::troubleshoot::diagnose(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn predictions_unified(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::predictions::PredictionsReport>, ApiError> {
    require_operator(&actor)?;
    let report = ai::predictions::unified(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(report))
}

pub async fn incidents_active(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::incident_commander::ActiveIncident>>, ApiError> {
    require_operator(&actor)?;
    let _ = ai::incident_commander::correlate_and_open(&state.pool).await;
    ai::incident_commander::list_active(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn rightsizing_report(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<ai::predictions::RightsizingReport>, ApiError> {
    require_operator(&actor)?;
    ai::predictions::rightsizing_report(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn incident_room(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<ai::incident_commander::IncidentRoom>, ApiError> {
    require_operator(&actor)?;
    ai::incident_commander::open_room(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn incident_ack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    ai::incident_commander::ack(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "acknowledged": true })))
}

pub async fn twin_simulate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::digital_twin::SimulateRequest>,
) -> Result<Json<ai::digital_twin::SimulateResult>, ApiError> {
    require_operator(&actor)?;
    ai::digital_twin::simulate_batch(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn nl_ops(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ai::nl_ops::NlOpsRequest>,
) -> Result<Json<ai::nl_ops::NlOpsPlan>, ApiError> {
    require_operator(&actor)?;
    ai::nl_ops::execute(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct MemoryBeforeQuery {
    pub incident_id: Option<Uuid>,
    #[serde(default = "default_hours_before")]
    pub hours_before: i32,
}

fn default_hours_before() -> i32 {
    4
}

pub async fn memory_changes_before(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<MemoryBeforeQuery>,
) -> Result<Json<ai::infrastructure_memory::ChangeBeforeOutage>, ApiError> {
    require_operator(&actor)?;
    ai::infrastructure_memory::changes_before_outage(&state.pool, q.incident_id, q.hours_before)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn zyra_trust_list(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<ai::trust::TrustClass>>, ApiError> {
    require_operator(&actor)?;
    ai::trust::classes(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

#[derive(Deserialize)]
pub struct TrustBody {
    pub level: String,
    #[serde(default)]
    pub max_per_run: Option<i64>,
}

pub async fn zyra_trust_set(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Path(action_type): axum::extract::Path<String>,
    Json(body): Json<TrustBody>,
) -> Result<Json<Vec<ai::trust::TrustClass>>, ApiError> {
    crate::auth::require_admin(&actor)?;
    ai::trust::set_rule(&state.pool, &actor.username, &action_type, &body.level, body.max_per_run)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    crate::tasks::enqueue::write_audit(
        &state,
        &actor.username,
        "ai.trust.set",
        "ai",
        None,
        serde_json::json!({ "action_type": action_type, "level": body.level, "max_per_run": body.max_per_run }),
    )
    .await?;
    ai::trust::classes(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}
