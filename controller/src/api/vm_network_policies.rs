// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Fleet VM network policies (CiliumNetworkPolicy schema), VM labels and
//! the fleet flow API (Hubble-style, polled from every host's bpfd).

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures_util::stream::Stream;
use machina_bpf::api::{
    Request, VmEdgeStatus, VmFlowAlert, VmFlowEdge, VmFlowRecord, VmQuarantineBody,
};
use machina_bpf::netpol::evidence;
use machina_bpf::netpol::jit::{self, JitRequest};
use machina_bpf::netpol::tenant::{self, Isolation, ProjectNet};
use machina_bpf::netpol::{
    self, nl, FlowFilter, LearnOptions, NetpolVm, ReplayInputs, TraceQuery, VmNetworkPolicy,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::bpf;
use crate::engine::vm_netpol::{self, Fleet};
use crate::state::AppState;

fn body_text(body: &Bytes) -> String {
    let text = String::from_utf8_lossy(body).into_owned();
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) if m.get("yaml").is_some_and(Value::is_string) => {
            m["yaml"].as_str().unwrap_or_default().into()
        }
        _ => text,
    }
}

fn selected_map(c: &netpol::Compiled) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &c.endpoints {
        for p in &e.policies {
            out.entry(p.clone()).or_default().push(e.name.clone());
        }
    }
    out
}

fn policy_json(
    p: &VmNetworkPolicy,
    enabled: bool,
    generation: i64,
    updated: &str,
    sel: Option<&Vec<String>>,
) -> Value {
    json!({
        "name": p.name,
        "kind": p.kind,
        "description": p.description(),
        "labels": p.labels,
        "annotations": p.annotations,
        "specs": p.specs,
        "yaml": p.to_yaml(),
        "enabled": enabled,
        "generation": generation,
        "updated_at": updated,
        "selected_vms": sel.cloned().unwrap_or_default(),
    })
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = vm_netpol::policies(&state.pool).await?;
    let fleet = Fleet::load(&state.pool).await;
    let c = fleet.compile(None);
    let sel = selected_map(&c);
    let items: Vec<Value> = rows
        .iter()
        .map(|(p, en, g, u)| policy_json(p, *en, *g, u, sel.get(&p.name)))
        .collect();
    Ok(Json(json!({ "items": items, "warnings": c.warnings })))
}

#[derive(Deserialize, Default)]
pub struct ApplyQuery {
    #[serde(default)]
    dry_run: Option<String>,
}

async fn preview(state: &AppState, parsed: &[VmNetworkPolicy], v: &netpol::Validation) -> Value {
    let mut fleet = Fleet::load(&state.pool).await;
    fleet
        .policies
        .retain(|p| !parsed.iter().any(|n| n.name == p.name));
    fleet.policies.extend(parsed.iter().cloned());
    let c = fleet.compile(None);
    let sel = selected_map(&c);
    json!({
        "valid": v.ok(),
        "errors": v.errors,
        "warnings": v.warnings,
        "compile_warnings": c.warnings,
        "policies": parsed.iter().map(|p| policy_json(p, true, 0, "", sel.get(&p.name))).collect::<Vec<_>>(),
        "rules": c.state.policy.len(),
        "endpoints": c.endpoints,
        "selectors": c.selectors.into_iter().filter(|s| parsed.iter().any(|p| p.name == s.policy)).collect::<Vec<_>>(),
    })
}

pub async fn apply(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ApplyQuery>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let (parsed, v) = netpol::parse_documents(&body_text(&body));
    if matches!(q.dry_run.as_deref(), Some("1" | "true" | "yes" | "")) {
        return Ok(Json(preview(&state, &parsed, &v).await).into_response());
    }
    require_admin(&actor)?;
    if !v.ok() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid policy", "errors": v.errors, "warnings": v.warnings })),
        )
            .into_response());
    }
    let mut created = Vec::new();
    for p in &parsed {
        if vm_netpol::upsert(&state.pool, p, &actor.username).await? {
            created.push(p.name.clone());
        }
    }
    let names: Vec<String> = parsed.iter().map(|p| p.name.clone()).collect();
    tracing::info!(actor = %actor.username, policies = ?names, "vm network policies applied");
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    Ok(
        Json(json!({ "applied": names, "created": created, "warnings": v.warnings, "sync": sync }))
            .into_response(),
    )
}

pub async fn validate(State(state): State<AppState>, body: Bytes) -> Json<Value> {
    let (parsed, v) = netpol::parse_documents(&body_text(&body));
    Json(preview(&state, &parsed, &v).await)
}

#[derive(Deserialize, Default)]
pub struct GetQuery {
    #[serde(default)]
    format: Option<String>,
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<GetQuery>,
) -> Result<Response, ApiError> {
    let rows = vm_netpol::policies(&state.pool).await?;
    let (p, en, g, u) = rows
        .iter()
        .find(|r| r.0.name == name)
        .ok_or_else(|| ApiError::not_found(format!("VM network policy `{name}`")))?;
    if q.format.as_deref() == Some("yaml") {
        return Ok(([(header::CONTENT_TYPE, "application/yaml")], p.to_yaml()).into_response());
    }
    let c = Fleet::load(&state.pool).await.compile(None);
    let sel = selected_map(&c);
    Ok(Json(policy_json(p, *en, *g, u, sel.get(&p.name))).into_response())
}

pub async fn delete_one(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    if !vm_netpol::delete(&state.pool, &name).await? {
        return Err(ApiError::not_found(format!("VM network policy `{name}`")));
    }
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    Ok(Json(json!({ "deleted": name, "sync": sync })))
}

#[derive(Deserialize)]
pub struct EnabledBody {
    enabled: bool,
}

pub async fn set_enabled(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(b): Json<EnabledBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    if !vm_netpol::set_enabled(&state.pool, &name, b.enabled).await? {
        return Err(ApiError::not_found(format!("VM network policy `{name}`")));
    }
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    Ok(Json(
        json!({ "name": name, "enabled": b.enabled, "sync": sync }),
    ))
}

pub async fn trace(
    State(state): State<AppState>,
    Json(q): Json<TraceQuery>,
) -> Result<Json<Value>, ApiError> {
    let fleet = Fleet::load(&state.pool).await;
    let r = netpol::trace(
        &fleet.policies,
        &fleet.vms,
        &fleet.services,
        &fleet.all_host_addresses(),
        &[],
        &q,
    )
    .map_err(ApiError::bad_request)?;
    Ok(Json(serde_json::to_value(r).unwrap_or_default()))
}

pub async fn endpoints(State(state): State<AppState>) -> Json<Value> {
    let c = Fleet::load(&state.pool).await.compile(None);
    Json(json!({ "items": c.endpoints }))
}

pub async fn selectors(State(state): State<AppState>) -> Json<Value> {
    let c = Fleet::load(&state.pool).await.compile(None);
    Json(json!({ "items": c.selectors }))
}

/// Sync record plus live edge state of every host.
async fn host_rows(state: &AppState) -> Result<Vec<Value>, ApiError> {
    type Row = (
        String,
        String,
        Option<String>,
        bool,
        Option<String>,
        i64,
        i64,
        i64,
        String,
    );
    let rows: Vec<Row> = crate::db::query_as(
        "SELECT host_id, hostname, synced_at, ok, error, vms, rules, peers, warnings FROM vm_netpol_host_status ORDER BY hostname",
    )
    .fetch_all(&state.pool)
    .await?;
    let synced: HashMap<String, Value> = rows
        .into_iter()
        .map(|(id, hn, at, ok, err, vms, rules, peers, w)| {
            let warnings: Vec<String> = serde_json::from_str(&w).unwrap_or_default();
            (id.clone(), json!({ "host_id": id, "hostname": hn, "synced_at": at, "ok": ok, "error": err, "vms": vms, "rules": rules, "peers": peers, "warnings": warnings }))
        })
        .collect();
    let mut hosts = Vec::new();
    for (h, res) in bpf::fan_out(&state.pool, &Request::VmEdgeStatus).await {
        let edge: Option<VmEdgeStatus> = res.ok().and_then(|v| serde_json::from_value(v).ok());
        if let Some(e) = &edge {
            vm_netpol::note_node_addrs(&h.id, &e.node_addrs);
        }
        let mut row = synced
            .get(&h.id)
            .cloned()
            .unwrap_or_else(|| json!({ "host_id": h.id, "hostname": h.hostname }));
        row["reachable"] = json!(edge.is_some());
        row["owner"] = json!(edge.as_ref().map(|e| e.owner.clone()));
        row["enforcing"] = json!(edge.as_ref().is_some_and(|e| e.enforcing));
        row["taps"] = json!(edge.as_ref().map_or(0, |e| e.taps.len()));
        row["missing"] = json!(edge.as_ref().map(|e| e.missing.clone()).unwrap_or_default());
        row["cilium"] = json!(edge.as_ref().and_then(|e| e.cilium.clone()));
        row["addresses"] = json!(edge
            .as_ref()
            .map(|e| e.node_addrs.clone())
            .unwrap_or_default());
        hosts.push(row);
    }
    Ok(hosts)
}

pub async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let hosts = host_rows(&state).await?;
    let policies = vm_netpol::policies(&state.pool).await?.len();
    Ok(Json(
        json!({ "policies": policies, "managed_by": "controller", "hosts": hosts }),
    ))
}

pub async fn sync_now(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    Ok(Json(
        json!({ "sync": vm_netpol::reconcile(&state.pool, true).await }),
    ))
}

// ---- labels -------------------------------------------------------------------

pub async fn get_labels(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row: Option<(String, Option<String>)> =
        crate::db::query_as("SELECT name, labels FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let (name, labels) = row.ok_or_else(|| ApiError::not_found("vm not found"))?;
    let labels: BTreeMap<String, String> = labels
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(Json(json!({ "id": id, "name": name, "labels": labels })))
}

#[derive(Deserialize)]
pub struct LabelsBody {
    pub labels: BTreeMap<String, String>,
}

pub fn validate_labels(labels: &BTreeMap<String, String>) -> Result<(), ApiError> {
    let part = |s: &str, max: usize| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    };
    for (k, v) in labels {
        let name = match k.split_once('/') {
            Some((p, n)) if part(p, 253) => n,
            Some(_) => {
                return Err(ApiError::bad_request(format!(
                    "label key `{k}`: bad prefix"
                )))
            }
            None => k.as_str(),
        };
        if !part(name, 63) {
            return Err(ApiError::bad_request(format!(
                "label key `{k}` is not a valid label name"
            )));
        }
        if !v.is_empty() && !part(v, 63) {
            return Err(ApiError::bad_request(format!(
                "label `{k}`: value `{v}` is not a valid label value"
            )));
        }
    }
    Ok(())
}

pub async fn put_labels(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(b): Json<LabelsBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    validate_labels(&b.labels)?;
    let r = crate::db::query("UPDATE vms SET labels = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(serde_json::to_string(&b.labels).unwrap_or_else(|_| "{}".into()))
        .bind(id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("vm not found"));
    }
    let pool = state.pool.clone();
    tokio::spawn(async move {
        vm_netpol::reconcile(&pool, false).await;
    });
    Ok(Json(json!({ "id": id, "labels": b.labels })))
}

// ---- flows --------------------------------------------------------------------

#[derive(Deserialize, Default, Clone)]
pub struct FlowQuery {
    limit: Option<usize>,
    last: Option<usize>,
    host: Option<String>,
    vm: Option<String>,
    from_vm: Option<String>,
    to_vm: Option<String>,
    label: Option<String>,
    ip: Option<String>,
    cidr: Option<String>,
    port: Option<String>,
    protocol: Option<String>,
    verdict: Option<String>,
    drop_reason: Option<String>,
    policy: Option<String>,
    direction: Option<String>,
}

impl FlowQuery {
    fn filter(&self) -> FlowFilter {
        let s = |v: &Option<String>| v.clone().filter(|x| !x.is_empty());
        FlowFilter {
            vm: s(&self.vm),
            from_vm: s(&self.from_vm),
            to_vm: s(&self.to_vm),
            label: s(&self.label),
            ip: s(&self.ip),
            cidr: s(&self.cidr),
            port: self.port.as_deref().and_then(|p| p.parse().ok()),
            protocol: s(&self.protocol),
            verdict: s(&self.verdict),
            drop_reason: s(&self.drop_reason),
            policy: s(&self.policy),
            direction: s(&self.direction),
            host: s(&self.host),
        }
    }
}

/// Recent flows from every online host, newest first, each tagged with its host.
async fn fleet_flows(state: &AppState, f: &FlowFilter, per_host: usize) -> Vec<VmFlowRecord> {
    let req = Request::VmFlows {
        limit: Some(per_host),
        vm: None,
        verdict: None,
    };
    let mut out = Vec::new();
    for (h, res) in bpf::fan_out(&state.pool, &req).await {
        let Ok(v) = res else { continue };
        let recs: Vec<VmFlowRecord> = serde_json::from_value(v).unwrap_or_default();
        for mut r in recs {
            r.host = Some(h.hostname.clone());
            if f.matches(&r) {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

#[derive(Deserialize, Default)]
pub struct EdgeQuery {
    vm: Option<String>,
    host: Option<String>,
    limit: Option<usize>,
}

/// Flow history edges from every online host, each tagged with its host.
pub async fn fleet_edges(state: &AppState, vm: Option<String>) -> Vec<VmFlowEdge> {
    let req = Request::VmFlowEdges { vm };
    let mut out = Vec::new();
    for (h, res) in bpf::fan_out(&state.pool, &req).await {
        let Ok(v) = res else { continue };
        let edges: Vec<VmFlowEdge> = serde_json::from_value(v).unwrap_or_default();
        out.extend(edges.into_iter().map(|mut e| {
            e.host = Some(h.hostname.clone());
            e
        }));
    }
    out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    out
}

pub async fn flow_edges(State(state): State<AppState>, Query(q): Query<EdgeQuery>) -> Json<Value> {
    let mut items = fleet_edges(&state, q.vm.filter(|v| !v.is_empty())).await;
    if let Some(h) = q.host.filter(|h| !h.is_empty()) {
        items.retain(|e| e.host.as_deref() == Some(h.as_str()));
    }
    Json(json!({ "items": items }))
}

pub async fn flow_edges_reset(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let res = bpf::fan_out(&state.pool, &Request::VmFlowEdgesReset {}).await;
    let ok = res.iter().filter(|(_, r)| r.is_ok()).count();
    Ok(Json(json!({ "ok": true, "hosts": ok })))
}

pub async fn fleet_alerts(state: &AppState, limit: usize) -> Vec<VmFlowAlert> {
    let req = Request::VmFlowAlerts { limit: Some(limit) };
    let mut out = Vec::new();
    for (h, res) in bpf::fan_out(&state.pool, &req).await {
        let Ok(v) = res else { continue };
        let alerts: Vec<VmFlowAlert> = serde_json::from_value(v).unwrap_or_default();
        out.extend(alerts.into_iter().map(|mut a| {
            a.host = Some(h.hostname.clone());
            a
        }));
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

pub async fn flow_alerts(State(state): State<AppState>, Query(q): Query<EdgeQuery>) -> Json<Value> {
    let limit = q.limit.unwrap_or(200).min(1000);
    let mut items = fleet_alerts(&state, limit).await;
    items.truncate(limit);
    Json(json!({ "items": items }))
}

#[derive(Deserialize, Default)]
pub struct HostQuery {
    host: Option<String>,
}

/// Online hosts matching `host` (id or hostname); all of them by default,
/// so a quarantine follows the VM wherever it runs or migrates.
async fn target_hosts(state: &AppState, host: Option<String>) -> Vec<bpf::HostRef> {
    let hosts = bpf::online_hosts(&state.pool).await;
    match host.filter(|h| !h.is_empty()) {
        Some(h) => hosts
            .into_iter()
            .filter(|x| x.id == h || x.hostname == h)
            .collect(),
        None => hosts,
    }
}

/// `req` on the target hosts; per-host results, error unless one succeeded.
async fn on_hosts(
    state: &AppState,
    host: Option<String>,
    req: &Request,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let hosts = target_hosts(state, host).await;
    if hosts.is_empty() {
        return Err(ApiError::not_found("no online host matches"));
    }
    let results = futures_util::future::join_all(hosts.iter().map(|h| bpf::call(h, req))).await;
    let (mut ok, mut errors) = (Vec::new(), Vec::new());
    for (h, r) in hosts.iter().zip(results) {
        match r {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.insert("host_id".into(), json!(h.id));
                    o.insert("hostname".into(), json!(h.hostname));
                }
                ok.push(v);
            }
            Err(e) => errors.push(json!({ "hostname": h.hostname, "error": format!("{e:#}") })),
        }
    }
    if ok.is_empty() {
        let first = errors
            .first()
            .and_then(|e| e["error"].as_str())
            .unwrap_or("failed")
            .to_string();
        return Err(ApiError::bad_request(first));
    }
    Ok((ok, errors))
}

/// Quarantine `vm` on the target hosts (also the `vm.quarantine` action).
pub(crate) async fn quarantine_vm(
    state: &AppState,
    vm: &str,
    host: Option<String>,
    b: VmQuarantineBody,
    by: &str,
) -> Result<Value, ApiError> {
    let reason = b.reason.clone();
    let req = b.into_request(vm.to_string(), by.to_string());
    let (hosts, errors) = on_hosts(state, host, &req).await?;
    let secs = match &req {
        Request::VmQuarantine { secs, .. } => *secs,
        _ => 0,
    };
    state.emit_event(
        "netpol.quarantine",
        format!(
            "{by} quarantined {vm} for {secs}s{}",
            if reason.is_empty() {
                String::new()
            } else {
                format!(": {reason}")
            }
        ),
    );
    Ok(json!({ "vm": vm, "hosts": hosts, "errors": errors }))
}

pub async fn quarantine(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
    Query(q): Query<HostQuery>,
    Json(b): Json<VmQuarantineBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    Ok(Json(
        quarantine_vm(&state, &name, q.host, b, &actor.username).await?,
    ))
}

pub async fn quarantine_release(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
    Query(q): Query<HostQuery>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let req = Request::VmQuarantineRelease { vm: name.clone() };
    let (hosts, errors) = on_hosts(&state, q.host, &req).await?;
    let released = hosts.iter().any(|h| h["released"].as_bool() == Some(true));
    if released {
        state.emit_event(
            "netpol.quarantine",
            format!("{} released the quarantine of {name}", actor.username),
        );
    }
    Ok(Json(
        json!({ "vm": name, "released": released, "hosts": hosts, "errors": errors }),
    ))
}

pub async fn quarantines(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "items": bpf::fan_out_items(&state.pool, &Request::VmQuarantines).await }))
}

// ---- DNS threat feeds -----------------------------------------------------------

/// `req` on every online host: (host results, errors), never failing.
async fn fan_out_report(state: &AppState, req: &Request) -> (Vec<Value>, Vec<Value>) {
    let (mut ok, mut errors) = (Vec::new(), Vec::new());
    for (h, r) in bpf::fan_out(&state.pool, req).await {
        match r {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.insert("host_id".into(), json!(h.id));
                    o.insert("hostname".into(), json!(h.hostname));
                }
                ok.push(v);
            }
            Err(e) => errors.push(json!({ "hostname": h.hostname, "error": format!("{e:#}") })),
        }
    }
    (ok, errors)
}

pub async fn threat_feeds(State(state): State<AppState>) -> Json<Value> {
    let feeds = vm_netpol::threat_feeds(&state.pool).await;
    let (hosts, errors) = fan_out_report(&state, &Request::VmThreatFeeds).await;
    let mut blocked = Vec::new();
    let mut watched = 0;
    for h in &hosts {
        watched += h["watched_vms"].as_u64().unwrap_or(0);
        for b in h["blocked"].as_array().into_iter().flatten() {
            let mut b = b.clone();
            if let Some(o) = b.as_object_mut() {
                o.insert("hostname".into(), h["hostname"].clone());
            }
            blocked.push(b);
        }
    }
    Json(json!({
        "feeds": feeds,
        "blocked": blocked,
        "watched_vms": watched,
        "hosts": hosts,
        "errors": errors,
    }))
}

async fn push_threat_feed(
    state: &AppState,
    f: &vm_netpol::ThreatFeedRow,
    what: &str,
) -> Json<Value> {
    let (hosts, errors) = fan_out_report(state, &f.request()).await;
    state.emit_event(
        "netpol.threat",
        format!(
            "{what} threat feed {} ({} domains, {})",
            f.name,
            f.domain_count,
            if f.block { "blocking" } else { "alert only" }
        ),
    );
    Json(json!({ "feed": f, "hosts": hosts, "errors": errors }))
}

pub async fn threat_feed_set(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(b): Json<machina_bpf::netpol::threat::FeedBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    machina_bpf::netpol::threat::check_name(&name).map_err(ApiError::bad_request)?;
    b.validate().map_err(ApiError::bad_request)?;
    let fetched = match &b.url {
        Some(u) => Some(
            vm_netpol::fetch_feed(u)
                .await
                .map_err(|e| ApiError::bad_request(format!("fetch {u}: {e:#}")))?,
        ),
        None => None,
    };
    let domains = b
        .domains(fetched.as_deref())
        .map_err(ApiError::bad_request)?;
    vm_netpol::threat_feed_put(
        &state.pool,
        &name,
        &b.source(),
        b.block,
        &domains,
        &actor.username,
    )
    .await
    .map_err(|e| ApiError::internal(format!("{e:#}")))?;
    let f = vm_netpol::threat_feeds(&state.pool)
        .await
        .into_iter()
        .find(|f| f.name == name)
        .ok_or_else(|| ApiError::internal("threat feed vanished"))?;
    Ok(push_threat_feed(&state, &f, &format!("{} set", actor.username)).await)
}

pub async fn threat_feed_remove(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let removed = vm_netpol::threat_feed_delete(&state.pool, &name)
        .await
        .map_err(|e| ApiError::internal(format!("{e:#}")))?;
    let (hosts, errors) =
        fan_out_report(&state, &Request::VmThreatFeedRemove { name: name.clone() }).await;
    let removed = removed || hosts.iter().any(|h| h["removed"].as_bool() == Some(true));
    if removed {
        state.emit_event(
            "netpol.threat",
            format!("{} removed threat feed {name}", actor.username),
        );
    }
    Ok(Json(
        json!({ "name": name, "removed": removed, "hosts": hosts, "errors": errors }),
    ))
}

pub async fn threat_feed_refresh(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let f = vm_netpol::threat_feeds(&state.pool)
        .await
        .into_iter()
        .find(|f| f.name == name)
        .ok_or_else(|| ApiError::not_found(format!("threat feed `{name}`")))?;
    if f.source.is_empty() {
        return Err(ApiError::bad_request(format!(
            "threat feed `{name}` is an inline list; set it again to change it"
        )));
    }
    let f = vm_netpol::refresh_threat_feed(&state.pool, &f, &actor.username)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(push_threat_feed(&state, &f, &format!("{} refreshed", actor.username)).await)
}

// ---- just-in-time access --------------------------------------------------------

// ---- Fleet Cloud project networking ------------------------------------------------

fn describe_project(s: &ProjectNet) -> String {
    let mut parts = vec![match s.isolation {
        Isolation::Isolated if s.allow_host => "isolated (host allowed)".to_string(),
        Isolation::Isolated => "isolated (host blocked)".to_string(),
        Isolation::Open => "open".to_string(),
        Isolation::Inherit => "follows the default".to_string(),
    }];
    if s.egress_restricted {
        parts.push(format!(
            "egress limited to {} destination(s)",
            s.egress_allow.len()
        ));
    }
    if !s.egress_ips.is_empty() {
        let ips: Vec<String> = s
            .egress_ips
            .iter()
            .map(|(h, ip)| format!("{ip} on {h}"))
            .collect();
        parts.push(format!("egress IP {}", ips.join(", ")));
    }
    parts.join(", ")
}

/// Every project with its settings, what it inherits and its VMs.
pub async fn projects(State(state): State<AppState>) -> Json<Value> {
    let fleet = Fleet::load(&state.pool).await;
    let names = vm_netpol::project_names(&state.pool, &fleet.vms).await;
    let mut all: std::collections::BTreeSet<String> = names;
    all.extend(
        fleet
            .projects
            .iter()
            .filter(|p| !p.is_default())
            .map(|p| p.project.clone()),
    );
    let default = fleet
        .projects
        .iter()
        .find(|p| p.is_default())
        .cloned()
        .unwrap_or_else(|| ProjectNet {
            project: tenant::DEFAULT_PROJECT.into(),
            isolation: Isolation::Open,
            ..Default::default()
        });
    let names = hostnames(&state).await;
    let gaps = fleet.egress_gaps();
    let spans: HashMap<String, vm_netpol::NatSpan> = fleet
        .nat_spans()
        .into_iter()
        .map(|mut n| {
            for h in &mut n.hosts {
                if let Some(name) = names.get(h) {
                    *h = name.clone();
                }
            }
            (n.project.clone(), n)
        })
        .collect();
    let items: Vec<Value> = all
        .iter()
        .map(|name| {
            let own = fleet.projects.iter().find(|p| &p.project == name);
            let (isolated, allow_host) = tenant::effective(&fleet.projects, name);
            let vms: Vec<&str> = fleet
                .vms
                .iter()
                .filter(|v| v.project.as_deref() == Some(name.as_str()))
                .map(|v| v.name.as_str())
                .collect();
            let policies: Vec<&str> = fleet
                .policies
                .iter()
                .filter(|p| {
                    p.labels.get(tenant::LABEL_MANAGED).map(String::as_str)
                        == Some(tenant::MANAGED_VALUE)
                        && p.labels.get(netpol::LABEL_PROJECT) == Some(name)
                })
                .map(|p| p.name.as_str())
                .collect();
            json!({
                "project": name,
                "explicit": own.is_some(),
                "settings": own.cloned().unwrap_or_else(|| ProjectNet { project: name.clone(), ..Default::default() }),
                "isolated": isolated,
                "allow_host": allow_host,
                "vms": vms,
                "policies": policies,
                "cross_host_nat": spans.get(name),
                "egress_gaps": gaps.iter().filter(|g| &g.project == name).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(json!({ "default": default, "items": items, "warnings": fleet_warnings(&fleet, &names) }))
}

pub const PROJECT_ACTION: &str = "vm_netpol.project";

#[derive(Deserialize, Default)]
pub struct ProjectSetQuery {
    /// Ask a second admin instead of applying.
    #[serde(default, deserialize_with = "query_flag")]
    propose: bool,
}

/// `?flag=1` as well as `?flag=true`.
fn query_flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match String::deserialize(d)?.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        v => Err(serde::de::Error::custom(format!(
            "expected true/false or 1/0, got `{v}`"
        ))),
    }
}

/// Every change waits for a second admin (`MACHINA_NETPOL_PROJECT_APPROVAL=1`).
fn project_approval_required() -> bool {
    std::env::var("MACHINA_NETPOL_PROJECT_APPROVAL").is_ok_and(|v| v == "1" || v == "true")
}

/// The caller's role in a Fleet Cloud project (`admin`, `operator`, `viewer`).
pub async fn project_role(state: &AppState, username: &str, project: &str) -> Option<String> {
    crate::db::query_scalar(
        "SELECT a.role FROM project_role_assignments a
           JOIN users u ON u.id = a.user_id
           JOIN projects p ON p.id = a.project_id
          WHERE u.username = ? AND p.name = ?
          ORDER BY CASE a.role WHEN 'admin' THEN 0 WHEN 'operator' THEN 1 ELSE 2 END
          LIMIT 1",
    )
    .bind(username)
    .bind(project)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
}

/// Fleet admins change any project. A project's own admins change its
/// isolation and egress allowlist, not its egress IPs (host addresses)
/// and not the default.
async fn check_project_change(
    state: &AppState,
    actor: &AuthUser,
    project: &str,
    cur: Option<&ProjectNet>,
    new: Option<&ProjectNet>,
) -> Result<(), ApiError> {
    if actor.role == "admin" {
        return Ok(());
    }
    if project == tenant::DEFAULT_PROJECT {
        return Err(ApiError::forbidden(
            "only fleet admins change the project default",
        ));
    }
    if project_role(state, &actor.username, project)
        .await
        .as_deref()
        != Some("admin")
    {
        return Err(ApiError::forbidden(
            "admin role required (fleet-wide or in this project)",
        ));
    }
    let ips = |p: Option<&ProjectNet>| p.map(|p| p.egress_ips.clone()).unwrap_or_default();
    if ips(cur) != ips(new) {
        return Err(
            ApiError::forbidden("egress IPs are set by fleet admins").with_remediation(
                "ask a fleet admin; project admins manage isolation and the egress allowlist",
            ),
        );
    }
    Ok(())
}

/// The policy set with `next` in place of the project's settings (`None`
/// = reset) and what it would change in the recorded traffic.
async fn project_replay(
    state: &AppState,
    project: &str,
    next: Option<&ProjectNet>,
    limit: usize,
) -> Result<Value, ApiError> {
    let fleet = Fleet::load(&state.pool).await;
    let mut settings: Vec<ProjectNet> = fleet
        .projects
        .iter()
        .filter(|s| s.project != project)
        .cloned()
        .collect();
    settings.extend(next.cloned());
    let names = vm_netpol::project_names(&state.pool, &fleet.vms).await;
    let mut draft: Vec<VmNetworkPolicy> = fleet
        .policies
        .iter()
        .filter(|p| {
            p.labels.get(tenant::LABEL_MANAGED).map(String::as_str) != Some(tenant::MANAGED_VALUE)
        })
        .cloned()
        .collect();
    let mut generated = tenant::policies(&settings, &names);
    generated.extend(tenant::egress_ip_guards(&settings, &fleet.hostnames));
    for g in generated {
        if !draft.iter().any(|p| p.name == g.name) {
            draft.push(g);
        }
    }
    replay_set(state, fleet, draft, limit).await
}

pub(crate) fn replay_summary(r: &Value) -> String {
    let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
    format!(
        "would break {} recorded connection{} ({} flows) and newly allow {}",
        n("would_break"),
        if n("would_break") == 1 { "" } else { "s" },
        r["flows_breaking"].as_u64().unwrap_or(0),
        n("would_allow")
    )
}

/// What a project change would do to recorded traffic; nothing is applied.
pub async fn project_preview(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
    Json(mut b): Json<ProjectNet>,
) -> Result<Json<Value>, ApiError> {
    b.project = project.trim().to_string();
    b.validate().map_err(ApiError::bad_request)?;
    if require_operator(&actor).is_err()
        && (b.is_default()
            || project_role(&state, &actor.username, &b.project)
                .await
                .is_none())
    {
        return Err(ApiError::forbidden("not a member of this project"));
    }
    let r = project_replay(&state, &b.project, Some(&b), 200).await?;
    Ok(Json(json!({
        "project": b.project,
        "describe": describe_project(&b),
        "summary": replay_summary(&r),
        "replay": r,
    })))
}

pub async fn project_set(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
    Query(q): Query<ProjectSetQuery>,
    Json(mut b): Json<ProjectNet>,
) -> Result<Json<Value>, ApiError> {
    b.project = project.trim().to_string();
    b.validate().map_err(ApiError::bad_request)?;
    if !b.egress_ips.is_empty() {
        let hosts = bpf::hosts(&state.pool).await;
        for h in b.egress_ips.keys() {
            if !hosts.iter().any(|x| &x.id == h || &x.hostname == h) {
                return Err(ApiError::bad_request(format!(
                    "egress IP host `{h}` is not a fleet host (use its name or id)"
                )));
            }
        }
    }
    let cur = vm_netpol::project_settings(&state.pool)
        .await
        .into_iter()
        .find(|s| s.project == b.project);
    check_project_change(&state, &actor, &b.project, cur.as_ref(), Some(&b)).await?;
    if q.propose || project_approval_required() {
        let r = project_replay(&state, &b.project, Some(&b), 50).await?;
        return propose_project(&state, &actor, &b.project, Some(&b), &r).await;
    }
    apply_project(&state, &b.project, Some(&b), &actor.username).await
}

pub async fn project_remove(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<String>,
    Query(q): Query<ProjectSetQuery>,
) -> Result<Json<Value>, ApiError> {
    let cur = vm_netpol::project_settings(&state.pool)
        .await
        .into_iter()
        .find(|s| s.project == project);
    check_project_change(&state, &actor, &project, cur.as_ref(), None).await?;
    if q.propose || project_approval_required() {
        let r = project_replay(&state, &project, None, 50).await?;
        return propose_project(&state, &actor, &project, None, &r).await;
    }
    apply_project(&state, &project, None, &actor.username).await
}

async fn propose_project(
    state: &AppState,
    actor: &AuthUser,
    project: &str,
    next: Option<&ProjectNet>,
    replay: &Value,
) -> Result<Json<Value>, ApiError> {
    let what = if project == tenant::DEFAULT_PROJECT {
        "the project default".to_string()
    } else {
        format!("project {project}")
    };
    let change = next.map_or_else(
        || "reset every network setting".to_string(),
        describe_project,
    );
    let body = crate::engine::ai::actions::CreateActionBody {
        action_type: PROJECT_ACTION.into(),
        label: format!("Change network settings of {what}"),
        review: format!(
            "{} asks to change {what}: {change}.\n\nAgainst the flow history this {}.",
            actor.username,
            replay_summary(replay)
        ),
        risk: "Changes which VMs can talk to each other or reach the internet".into(),
        object_ref: json!({ "project": project, "settings": next }),
        source: "netpol".into(),
    };
    let action = crate::engine::ai::actions::create_action(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "netpol.project",
        format!("{} asked to change {what}: {change}", actor.username),
    );
    Ok(Json(
        json!({ "pending": action, "preview": replay_summary(replay) }),
    ))
}

async fn apply_project(
    state: &AppState,
    project: &str,
    next: Option<&ProjectNet>,
    by: &str,
) -> Result<Json<Value>, ApiError> {
    let what = if project == tenant::DEFAULT_PROJECT {
        "the project default".to_string()
    } else {
        format!("project {project}")
    };
    let removed = match next {
        Some(b) => {
            vm_netpol::project_put(&state.pool, b, by)
                .await
                .map_err(|e| ApiError::internal(format!("{e:#}")))?;
            state.emit_event(
                "netpol.project",
                format!("{by} set {what}: {}", describe_project(b)),
            );
            false
        }
        None => {
            let removed = vm_netpol::project_delete(&state.pool, project)
                .await
                .map_err(|e| ApiError::internal(format!("{e:#}")))?;
            if removed {
                state.emit_event(
                    "netpol.project",
                    format!("{by} reset network settings of {what}"),
                );
            }
            removed
        }
    };
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    Ok(Json(match next {
        Some(b) => json!({ "project": b, "sync": sync }),
        None => json!({ "project": project, "removed": removed, "sync": sync }),
    }))
}

/// Apply an approved `vm_netpol.project` action.
pub(crate) async fn project_approved(
    state: &AppState,
    object_ref: &Value,
    by: &str,
) -> Result<Value, ApiError> {
    let project = object_ref["project"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let next: Option<ProjectNet> = match &object_ref["settings"] {
        Value::Null => None,
        v => {
            let mut s: ProjectNet = serde_json::from_value(v.clone())
                .map_err(|e| ApiError::bad_request(format!("project settings: {e}")))?;
            s.project = project.clone();
            s.validate().map_err(ApiError::bad_request)?;
            Some(s)
        }
    };
    Ok(apply_project(state, &project, next.as_ref(), by).await?.0)
}

/// Egress SNAT state of every online host.
pub async fn egress_ips(State(state): State<AppState>) -> Json<Value> {
    let (hosts, errors) = fan_out_report(&state, &Request::VmEgressSnatStatus).await;
    Json(json!({ "items": hosts, "errors": errors }))
}

// ---- WireGuard overlay -------------------------------------------------------------------

/// Overlay settings, every host's state, and the fleet address of each VM.
pub async fn overlay(State(state): State<AppState>) -> Json<Value> {
    let s = crate::engine::vm_overlay::settings(&state.pool).await;
    let (hosts, errors) = if s.enabled {
        fan_out_report(&state, &Request::VmOverlayStatus).await
    } else {
        (Vec::new(), Vec::new())
    };
    Json(json!({ "settings": s, "items": hosts, "errors": errors }))
}

#[derive(Deserialize, Default)]
pub struct OverlayBody {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    prefix4: Option<String>,
    #[serde(default)]
    prefix6: Option<String>,
    #[serde(default)]
    port: Option<u16>,
}

/// Turn the overlay on or off, or change its prefixes or port. Changing a
/// prefix renumbers every fleet address.
pub async fn overlay_set(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<OverlayBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let mut s = crate::engine::vm_overlay::settings(&state.pool).await;
    if let Some(e) = b.enabled {
        s.enabled = e;
    }
    if let Some(p) = b.prefix4 {
        s.prefix4 = p.trim().to_string();
    }
    if let Some(p) = b.prefix6 {
        s.prefix6 = p.trim().to_string();
    }
    if let Some(p) = b.port {
        s.port = p;
    }
    s.validate()
        .map_err(|e| ApiError::bad_request(e).with_code("invalid_overlay"))?;
    s.updated_by = actor.username.clone();
    crate::engine::vm_overlay::put_settings(&state.pool, &s)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "netpol.overlay",
        format!(
            "{} turned the WireGuard overlay {} ({}, {}, port {})",
            actor.username,
            if s.enabled { "on" } else { "off" },
            s.prefix4,
            s.prefix6,
            s.port
        ),
    );
    // The first push makes hosts create their keys; the second gives them peers.
    let mut sync = vm_netpol::reconcile(&state.pool, false).await;
    if s.enabled {
        sync = vm_netpol::reconcile(&state.pool, false).await;
    }
    Ok(Json(json!({ "settings": s, "sync": sync })))
}

// ---- Segmentation evidence ------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct EvidenceQuery {
    #[serde(default)]
    format: Option<String>,
    /// `tcp/22,udp/53` (default: the standard probes).
    #[serde(default)]
    probes: Option<String>,
    /// One project only (its members may export it).
    #[serde(default)]
    project: Option<String>,
}

/// Answer an evidence report as JSON or (`format=md`) Markdown.
pub fn evidence_response(e: &evidence::Evidence, format: Option<&str>) -> Response {
    let stamp = e.generated_at.replace([':', '-'], "");
    match format {
        Some("md" | "markdown") => (
            [
                (
                    header::CONTENT_TYPE,
                    "text/markdown; charset=utf-8".to_string(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"segmentation-evidence-{stamp}.md\""),
                ),
            ],
            evidence::markdown(e),
        )
            .into_response(),
        _ => Json(e).into_response(),
    }
}

pub async fn evidence(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<EvidenceQuery>,
) -> Result<Response, ApiError> {
    let probes =
        evidence::parse_probes(q.probes.as_deref().unwrap_or("")).map_err(ApiError::bad_request)?;
    let project = q
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    match project {
        None => require_operator(&actor).map_err(|e| {
            e.with_remediation("project members can export their project: ?project=NAME")
        })?,
        Some(p) => {
            if require_operator(&actor).is_err()
                && project_role(&state, &actor.username, p).await.is_none()
            {
                return Err(ApiError::forbidden("not a member of this project"));
            }
        }
    }
    let e = build_evidence(&state, &actor.username, &probes, project).await?;
    state.emit_event(
        "netpol.evidence",
        format!(
            "{} exported segmentation evidence {}",
            actor.username,
            &e.digest[..16]
        ),
    );
    Ok(evidence_response(&e, q.format.as_deref()))
}

/// Scheduled reports kept on the controller, newest first.
pub async fn evidence_archive(
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let items: Vec<Value> = vm_netpol::evidence_archive()
        .into_iter()
        .map(|(name, size, at)| {
            let at: chrono::DateTime<chrono::Utc> = at.into();
            json!({ "name": name, "bytes": size, "modified": at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) })
        })
        .collect();
    Ok(Json(
        json!({ "dir": vm_netpol::evidence_dir(), "items": items }),
    ))
}

pub async fn evidence_archived(
    Extension(actor): Extension<AuthUser>,
    Path(name): Path<String>,
    Query(q): Query<EvidenceQuery>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    let known = vm_netpol::evidence_archive()
        .into_iter()
        .any(|(n, _, _)| n == name);
    if !known {
        return Err(ApiError::not_found("no such stored evidence report"));
    }
    let body = std::fs::read(vm_netpol::evidence_dir().join(&name))
        .map_err(|e| ApiError::internal(format!("read {name}: {e}")))?;
    let e: evidence::Evidence =
        serde_json::from_slice(&body).map_err(|e| ApiError::internal(format!("{name}: {e}")))?;
    if q.format
        .as_deref()
        .is_some_and(|f| f == "md" || f == "markdown")
    {
        return Ok(evidence_response(&e, q.format.as_deref()));
    }
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// Projects spread over hosts on per-host NAT networks and VMs without
/// their project's egress IP, as report lines.
pub fn fleet_warnings(fleet: &Fleet, hostnames: &HashMap<String, String>) -> Vec<String> {
    let gaps = fleet.egress_gaps().into_iter().map(|g| {
        format!(
            "VM {} of project {} runs on {}, which has none of the project's egress IPs ({})",
            g.vm,
            g.project,
            g.host,
            if g.blocked {
                "internet egress blocked"
            } else {
                "leaves with the host's address"
            }
        )
    });
    fleet
        .nat_spans()
        .iter()
        .map(|n| {
            let hosts: Vec<&str> = n
                .hosts
                .iter()
                .map(|h| hostnames.get(h).map_or(h.as_str(), String::as_str))
                .collect();
            format!(
                "project {} has VMs on {} behind per-host NAT networks ({}): traffic between those hosts arrives from the other host's address, so it is matched as remote-node and isolation cannot tell projects apart across hosts; use routed or bridged VM networks",
                n.project,
                hosts.join(", "),
                n.subnets.join(", ")
            )
        })
        .chain(gaps)
        .collect()
}

pub async fn hostnames(state: &AppState) -> HashMap<String, String> {
    bpf::hosts(&state.pool)
        .await
        .into_iter()
        .map(|h| (h.id, h.hostname))
        .collect()
}

/// Build, seal and (when the CA is available) sign a fleet report.
pub async fn build_evidence(
    state: &AppState,
    actor: &str,
    probes: &[evidence::Probe],
    project: Option<&str>,
) -> Result<evidence::Evidence, ApiError> {
    let state = state.clone();
    let fleet = Fleet::load(&state.pool).await;
    let rows = vm_netpol::policies(&state.pool).await?;
    let c = fleet.compile(None);
    let sel = selected_map(&c);
    let mut policies: Vec<evidence::PolicyEvidence> = rows
        .iter()
        .map(|(p, en, _, u)| evidence::PolicyEvidence {
            name: p.name.clone(),
            kind: p.kind.clone(),
            enabled: *en,
            generated: false,
            description: p.description(),
            sha256: evidence::policy_hash(p),
            selected_vms: sel.get(&p.name).cloned().unwrap_or_default(),
            updated_at: u.clone(),
        })
        .collect();
    policies.extend(
        fleet
            .policies
            .iter()
            .filter(|p| {
                p.labels.get(tenant::LABEL_MANAGED).map(String::as_str)
                    == Some(tenant::MANAGED_VALUE)
            })
            .map(|p| evidence::PolicyEvidence {
                name: p.name.clone(),
                kind: p.kind.clone(),
                enabled: true,
                generated: true,
                description: p.description(),
                sha256: evidence::policy_hash(p),
                selected_vms: sel.get(&p.name).cloned().unwrap_or_default(),
                updated_at: String::new(),
            }),
    );
    let nothing_to_push = fleet.policies.is_empty();
    let hosts = host_rows(&state)
        .await?
        .iter()
        .map(|h| evidence::HostEvidence {
            hostname: h["hostname"].as_str().unwrap_or_default().to_string(),
            reachable: h["reachable"].as_bool().unwrap_or(false),
            enforcing: h["enforcing"].as_bool().unwrap_or(false),
            owner: h["owner"].as_str().unwrap_or_default().to_string(),
            synced_at: h["synced_at"].as_str().map(str::to_string),
            in_sync: h["reachable"].as_bool().unwrap_or(false)
                && (h["ok"].as_bool().unwrap_or(false) || nothing_to_push),
            error: h["error"].as_str().map(str::to_string),
        })
        .collect();
    let projects: Vec<Value> = projects(State(state.clone()))
        .await
        .0["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let s: ProjectNet = serde_json::from_value(p["settings"].clone()).unwrap_or_default();
            let iso = if p["isolated"].as_bool() == Some(true) {
                if p["allow_host"].as_bool() == Some(true) { "isolated" } else { "isolated (no host)" }
            } else {
                "open"
            };
            json!({
                "project": p["project"],
                "vms": p["vms"],
                "isolated": p["isolated"],
                "isolation": format!("{iso}{}", if s.isolation == Isolation::Inherit { " (default)" } else { "" }),
                "egress": if s.egress_restricted {
                    s.egress_allow.iter().map(|e| if e.ports.is_empty() { e.to.clone() } else { format!("{} ({})", e.to, e.ports.join(", ")) }).collect::<Vec<_>>().join(", ")
                } else { "any".into() },
                "egress_ips": s.egress_ips.iter().map(|(h, ip)| format!("{ip}@{h}")).collect::<Vec<_>>().join(", "),
                "policies": p["policies"],
            })
        })
        .collect();
    let (kind, groups) = match project {
        Some(p) => evidence::groups_for_project(&fleet.vms, p),
        None => evidence::groups(&fleet.vms),
    };
    let (pol, vms, svcs, haddr, pr) = (
        fleet.policies.clone(),
        fleet.vms.clone(),
        fleet.services.clone(),
        fleet.all_host_addresses(),
        probes.to_vec(),
    );
    let matrix = tokio::task::spawn_blocking(move || {
        evidence::matrix(&pol, &vms, &svcs, &haddr, &groups, &pr)
    })
    .await
    .map_err(|e| ApiError::internal(format!("matrix: {e}")))?;
    let denied = evidence::denied(&fleet_edges(&state, None).await);
    let now = chrono::Utc::now();
    let stored: Vec<VmNetworkPolicy> = rows.iter().map(|r| r.0.clone()).collect();
    let approvals: Vec<(String, String, String, Option<String>, String, String)> = crate::db::query_as(
        "SELECT action_type, label, requested_by, approved_by, status, created_at FROM ai_actions
         WHERE action_type IN (?, ?, ?, 'vm.quarantine') AND created_at >= datetime('now', '-90 days')
         ORDER BY created_at DESC LIMIT 500",
    )
    .bind(JIT_ACTION)
    .bind(APPLY_ACTION)
    .bind(PROJECT_ACTION)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    let (egress_hosts, _) = fan_out_report(&state, &Request::VmEgressSnatStatus).await;
    let mut e = evidence::Evidence {
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        generated_by: actor.to_string(),
        scope: "fleet".into(),
        source: "machina-controller".into(),
        hosts,
        policies,
        projects,
        matrix_groups: kind,
        matrix,
        alerts: fleet_alerts(&state, 50)
            .await
            .iter()
            .map(|a| serde_json::to_value(a).unwrap_or_default())
            .collect(),
        quarantines: bpf::fan_out_items(&state.pool, &Request::VmQuarantines).await,
        temporary_access: jit::grants(&stored, now)
            .iter()
            .map(|g| serde_json::to_value(g).unwrap_or_default())
            .collect(),
        threat_feeds: vm_netpol::threat_feeds(&state.pool)
            .await
            .iter()
            .map(|f| json!({ "name": f.name, "source": f.source, "block": f.block, "domains": f.domain_count, "updated": f.updated_at }))
            .collect(),
        egress_ips: egress_hosts
            .iter()
            .flat_map(|h| {
                h["rules"].as_array().cloned().unwrap_or_default().into_iter().map(move |r| {
                    json!({ "hostname": h["hostname"], "project": r["project"], "egress_ip": r["egress_ip"], "sources": r["sources"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")) })
                })
            })
            .collect(),
        approvals: approvals
            .into_iter()
            .map(|(t, l, r, a, st, at)| json!({ "action_type": t, "label": l, "requested_by": r, "approved_by": a, "status": st, "created_at": at }))
            .collect(),
        denied: denied.clone(),
        matrix_probes: probes.iter().map(|(p, n)| format!("{p}/{n}")).collect(),
        warnings: fleet_warnings(&fleet, &hostnames(&state).await),
        ..Default::default()
    };
    e.summary.vms = fleet.vms.len();
    let mut denied_total = denied.len();
    if let Some(p) = project {
        let members: std::collections::BTreeSet<String> = fleet
            .vms
            .iter()
            .filter(|v| v.project.as_deref() == Some(p))
            .map(|v| v.name.clone())
            .collect();
        if members.is_empty() && !fleet.projects.iter().any(|s| s.project == p) {
            return Err(ApiError::not_found(format!("no project {p}")));
        }
        evidence::scope_to_project(&mut e, p, &members);
        denied_total = e.denied.len();
    }
    e.seal(denied_total);
    match vm_netpol::evidence_signer() {
        Ok((signer, ca_pem)) => {
            if let Err(err) = e.sign(&signer, &ca_pem) {
                tracing::warn!("segmentation evidence signature: {err:#}");
            }
        }
        Err(err) => tracing::warn!("segmentation evidence signer: {err:#}"),
    }
    Ok(e)
}

pub const JIT_ACTION: &str = "vm_netpol.jit";

async fn check_jit(state: &AppState, req: &JitRequest) -> Result<(), ApiError> {
    req.validate().map_err(ApiError::bad_request)?;
    let inv = vm_netpol::inventory(&state.pool).await;
    for vm in [&req.to, &req.from] {
        if vm != "host" && !inv.iter().any(|v| &v.name == vm) {
            return Err(ApiError::not_found(format!("VM `{vm}`")));
        }
    }
    Ok(())
}

/// Store the grant and push it (also the approved `vm_netpol.jit` action).
pub(crate) async fn jit_grant(
    state: &AppState,
    req: &JitRequest,
    by: &str,
) -> Result<Value, ApiError> {
    check_jit(state, req).await?;
    let now = chrono::Utc::now();
    let p = req.policy(by, now).map_err(ApiError::bad_request)?;
    vm_netpol::upsert(&state.pool, &p, by).await?;
    state.emit_event(
        "netpol.jit",
        format!(
            "{by} granted {} for {}{}",
            req.what(),
            jit::duration_label(req.secs()),
            if req.reason.is_empty() {
                String::new()
            } else {
                format!(": {}", req.reason)
            }
        ),
    );
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    let grant = jit::grants(std::slice::from_ref(&p), now).pop();
    Ok(json!({ "granted": grant, "policy": p.name, "sync": sync }))
}

#[derive(Deserialize)]
pub struct JitBody {
    #[serde(flatten)]
    req: JitRequest,
    /// Admins only: grant now instead of asking for approval.
    #[serde(default)]
    grant: bool,
}

pub async fn jit_request(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<JitBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    if b.grant {
        require_admin(&actor)?;
        return Ok(Json(jit_grant(&state, &b.req, &actor.username).await?));
    }
    check_jit(&state, &b.req).await?;
    let req = &b.req;
    let body = crate::engine::ai::actions::CreateActionBody {
        action_type: JIT_ACTION.into(),
        label: format!(
            "Allow {} for {}",
            req.what(),
            jit::duration_label(req.secs())
        ),
        review: format!(
            "{} asks for temporary network access: {}. It is removed after {}.{}",
            actor.username,
            req.what(),
            jit::duration_label(req.secs()),
            if req.reason.is_empty() {
                String::new()
            } else {
                format!(" Reason: {}", req.reason)
            }
        ),
        risk: "Opens network access until it expires".into(),
        object_ref: serde_json::to_value(req).unwrap_or_default(),
        source: "netpol".into(),
    };
    let action = crate::engine::ai::actions::create_action(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "netpol.jit",
        format!("{} requested {}", actor.username, req.what()),
    );
    Ok(Json(json!({ "pending": action })))
}

pub async fn jit_list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let policies: Vec<VmNetworkPolicy> = vm_netpol::policies(&state.pool)
        .await?
        .into_iter()
        .filter(|r| r.1)
        .map(|r| r.0)
        .collect();
    let pending: Vec<_> = crate::engine::ai::actions::list_pending(&state.pool)
        .await?
        .into_iter()
        .filter(|a| a.action_type == JIT_ACTION)
        .collect();
    Ok(Json(json!({
        "items": jit::grants(&policies, chrono::Utc::now()),
        "pending": pending,
    })))
}

/// Address → DNS names from every host's `toFQDNs` cache.
async fn fqdn_names(state: &AppState) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in bpf::fan_out_items(&state.pool, &Request::VmFqdnCache).await {
        let (Some(addr), Some(name)) = (v["address"].as_str(), v["name"].as_str()) else {
            continue;
        };
        let names = m.entry(addr.to_string()).or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    m
}

pub async fn learn(
    State(state): State<AppState>,
    Json(mut o): Json<LearnOptions>,
) -> Result<Json<Value>, ApiError> {
    let edges = fleet_edges(&state, None).await;
    let fleet = Fleet::load(&state.pool).await;
    o.fqdn.extend(fqdn_names(&state).await);
    let r = tokio::task::spawn_blocking(move || netpol::learn(&edges, &fleet.vms, &o))
        .await
        .map_err(|e| ApiError::internal(format!("learn: {e}")))?;
    Ok(Json(serde_json::to_value(r).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct ReplayBody {
    yaml: String,
    #[serde(default)]
    limit: Option<usize>,
}

pub async fn replay(
    State(state): State<AppState>,
    Json(b): Json<ReplayBody>,
) -> Result<Response, ApiError> {
    let (parsed, v) = netpol::parse_documents(&b.yaml);
    if !v.ok() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid policy", "errors": v.errors, "warnings": v.warnings })),
        )
            .into_response());
    }
    let limit = b.limit.unwrap_or(5000).min(20_000);
    Ok(Json(replay_draft(&state, parsed, limit).await?).into_response())
}

/// Replay recorded flows against the current policies with `parsed` added
/// or replacing same-named ones.
pub(crate) async fn replay_draft(
    state: &AppState,
    parsed: Vec<VmNetworkPolicy>,
    limit: usize,
) -> Result<Value, ApiError> {
    let fleet = Fleet::load(&state.pool).await;
    let mut draft = fleet.policies.clone();
    draft.retain(|p| !parsed.iter().any(|n| n.name == p.name));
    draft.extend(parsed);
    replay_set(state, fleet, draft, limit).await
}

/// Replay the flow history against the fleet's policies and `draft`.
async fn replay_set(
    state: &AppState,
    fleet: Fleet,
    draft: Vec<VmNetworkPolicy>,
    limit: usize,
) -> Result<Value, ApiError> {
    let edges = fleet_edges(state, None).await;
    let fqdn = fqdn_names(state).await;
    let r = tokio::task::spawn_blocking(move || {
        let hosts = fleet.all_host_addresses();
        netpol::replay(
            &edges,
            &fleet.policies,
            &draft,
            &ReplayInputs {
                vms: &fleet.vms,
                services: &fleet.services,
                host_addresses: &hosts,
                remote_node_addresses: &[],
                fqdn: &fqdn,
                limit,
            },
        )
    })
    .await
    .map_err(|e| ApiError::internal(format!("replay: {e}")))?;
    Ok(serde_json::to_value(r).unwrap_or_default())
}

pub const APPLY_ACTION: &str = "vm_netpol.apply";

#[derive(Deserialize)]
pub struct DraftBody {
    prompt: String,
    /// Skip the LLM and use the built-in sentence parser.
    #[serde(default)]
    rules_only: bool,
}

/// Ask the configured LLM for a draft, with one repair round. Returns the
/// draft or why there is none (`None, None` when no LLM is configured).
async fn llm_draft(
    state: &AppState,
    prompt: &str,
    vms: &[NetpolVm],
    existing: &[String],
) -> (Option<nl::Draft>, Option<String>) {
    let system = nl::llm_system_prompt(vms, existing);
    let Ok(Some(reply)) =
        crate::engine::ai::llm::complete_simple(&state.pool, &system, prompt).await
    else {
        return (None, None);
    };
    if let Some(why) = nl::llm_declined(&reply) {
        return (None, Some(format!("Zyvor declined: {why}")));
    }
    let yaml = nl::extract_yaml(&reply);
    let errors = match nl::accept_llm(&yaml, vms) {
        Ok(d) => return (Some(d), None),
        Err(e) => e,
    };
    let repair = nl::llm_repair_prompt(prompt, &yaml, &errors);
    let errors = match crate::engine::ai::llm::complete_simple(&state.pool, &system, &repair).await
    {
        Ok(Some(r)) => match nl::accept_llm(&nl::extract_yaml(&r), vms) {
            Ok(d) => return (Some(d), None),
            Err(e) => e,
        },
        _ => errors,
    };
    (
        None,
        Some(format!("Zyvor's draft was rejected: {}", errors.join("; "))),
    )
}

/// Draft policies from plain English: Zyvor's LLM when configured, the
/// sentence parser otherwise or when the LLM draft is unusable.
pub(crate) async fn compose(state: &AppState, prompt: &str, rules_only: bool) -> nl::Draft {
    let fleet = Fleet::load(&state.pool).await;
    let existing: Vec<String> = fleet.policies.iter().map(|p| p.name.clone()).collect();
    let (llm, why) = if rules_only {
        (None, None)
    } else {
        llm_draft(state, prompt, &fleet.vms, &existing).await
    };
    let mut d = llm.unwrap_or_else(|| nl::draft_rules(prompt, &fleet.vms));
    d.notes.extend(why);
    d
}

/// Draft policies from plain English, then validate and replay them. Nothing
/// is applied.
pub async fn draft(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<DraftBody>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    let prompt = b.prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError::bad_request(
            "describe the policy in plain English",
        ));
    }
    if prompt.chars().count() > nl::MAX_PROMPT {
        return Err(ApiError::bad_request(format!(
            "keep the description under {} characters",
            nl::MAX_PROMPT
        )));
    }
    let d = compose(&state, prompt, b.rules_only).await;
    if d.policies.is_empty() {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": format!(
                    "could not draft a policy from that description: {}",
                    d.unparsed.iter().chain(&d.notes).cloned().collect::<Vec<_>>().join("; ")
                ),
                "unparsed": d.unparsed,
                "notes": d.notes,
            })),
        )
            .into_response());
    }
    let (parsed, v) = netpol::parse_documents(&d.yaml);
    let pv = preview(&state, &parsed, &v).await;
    let rp = replay_draft(&state, parsed, 5000).await?;
    Ok(Json(json!({
        "yaml": d.yaml,
        "source": d.source,
        "notes": d.notes,
        "unparsed": d.unparsed,
        "preview": pv,
        "replay": rp,
    }))
    .into_response())
}

#[derive(Deserialize)]
pub struct ProposeBody {
    yaml: String,
    #[serde(default)]
    prompt: String,
}

/// Ask a second admin to apply a drafted policy.
pub async fn draft_propose(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(b): Json<ProposeBody>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    let (parsed, v) = netpol::parse_documents(&b.yaml);
    if !v.ok() || parsed.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid policy", "errors": v.errors, "warnings": v.warnings })),
        )
            .into_response());
    }
    let names: Vec<String> = parsed.iter().map(|p| p.name.clone()).collect();
    let prompt: String = b.prompt.trim().chars().take(nl::MAX_PROMPT).collect();
    let body = crate::engine::ai::actions::CreateActionBody {
        action_type: APPLY_ACTION.into(),
        label: format!("Apply network policy {}", names.join(", ")),
        review: format!(
            "{} asks to apply {} VM network {}{}.\n\n{}",
            actor.username,
            names.len(),
            if names.len() == 1 {
                "policy"
            } else {
                "policies"
            },
            if prompt.is_empty() {
                String::new()
            } else {
                format!(" written from “{prompt}”")
            },
            nl::to_yaml(&parsed)
        ),
        risk: "Changes which VMs can talk to each other".into(),
        object_ref: json!({ "yaml": b.yaml, "prompt": prompt, "policies": names }),
        source: "netpol".into(),
    };
    let action = crate::engine::ai::actions::create_action(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event(
        "netpol.nl",
        format!("{} asked to apply {}", actor.username, names.join(", ")),
    );
    Ok(Json(json!({ "pending": action })).into_response())
}

/// Drafts waiting for approval.
pub async fn draft_pending(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pending: Vec<_> = crate::engine::ai::actions::list_pending(&state.pool)
        .await?
        .into_iter()
        .filter(|a| a.action_type == APPLY_ACTION)
        .collect();
    Ok(Json(json!({ "pending": pending })))
}

/// Apply an approved `vm_netpol.apply` action.
pub(crate) async fn apply_approved(
    state: &AppState,
    object_ref: &Value,
    by: &str,
) -> Result<Value, ApiError> {
    let (parsed, v) = netpol::parse_documents(object_ref["yaml"].as_str().unwrap_or_default());
    if !v.ok() || parsed.is_empty() {
        let errors: Vec<String> = v.errors.iter().map(|i| i.to_string()).collect();
        return Err(ApiError::bad_request(format!(
            "the drafted policy is no longer valid: {}",
            errors.join("; ")
        )));
    }
    for p in &parsed {
        vm_netpol::upsert(&state.pool, p, by).await?;
    }
    let names: Vec<String> = parsed.iter().map(|p| p.name.clone()).collect();
    state.emit_event(
        "netpol.nl",
        format!("{by} approved and applied {}", names.join(", ")),
    );
    let sync = vm_netpol::reconcile(&state.pool, false).await;
    Ok(json!({ "applied": names, "sync": sync }))
}

/// `toFQDNs` bindings learned on every online host.
pub async fn fqdn_cache(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "items": bpf::fan_out_items(&state.pool, &Request::VmFqdnCache).await }))
}

/// Mutual-authentication table of every online host.
pub async fn auth_table(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "items": bpf::fan_out_items(&state.pool, &Request::VmAuthTable).await }))
}

pub async fn flows(State(state): State<AppState>, Query(q): Query<FlowQuery>) -> Json<Value> {
    let limit = q.limit.unwrap_or(200).min(5000);
    let mut items = fleet_flows(&state, &q.filter(), 5000).await;
    items.truncate(limit);
    Json(json!({ "items": items }))
}

/// Live fleet flows: polls each host's bpfd once a second and emits records
/// newer than the last one seen per host.
pub async fn flow_stream(
    State(state): State<AppState>,
    Query(q): Query<FlowQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    struct Poll {
        state: AppState,
        f: FlowFilter,
        last: usize,
        seen: HashMap<String, String>,
        first: bool,
        queue: std::collections::VecDeque<VmFlowRecord>,
    }
    let init = Poll {
        state,
        f: q.filter(),
        last: q.last.unwrap_or(0).min(5000),
        seen: HashMap::new(),
        first: true,
        queue: Default::default(),
    };
    let stream = futures_util::stream::unfold(init, |mut p| async move {
        loop {
            if let Some(r) = p.queue.pop_front() {
                let ev = Event::default()
                    .event("flow")
                    .data(serde_json::to_string(&r).unwrap_or_default());
                return Some((Ok(ev), p));
            }
            if !p.first {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            let mut batch = fleet_flows(&p.state, &p.f, 500).await;
            batch.reverse();
            if p.first {
                for r in &batch {
                    let h = r.host.clone().unwrap_or_default();
                    if p.seen.get(&h).is_none_or(|t| r.ts > *t) {
                        p.seen.insert(h, r.ts.clone());
                    }
                }
                let skip = batch.len().saturating_sub(p.last);
                p.queue.extend(batch.into_iter().skip(skip));
                p.first = false;
            } else {
                for r in batch {
                    let h = r.host.clone().unwrap_or_default();
                    if p.seen.get(&h).is_some_and(|t| r.ts <= *t) {
                        continue;
                    }
                    p.seen.insert(h, r.ts.clone());
                    p.queue.push_back(r);
                }
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn project_propose_query_takes_1_and_true() {
        use axum::extract::Query;
        let q = |s: &str| {
            Query::<super::ProjectSetQuery>::try_from_uri(
                &format!("http://x/p?{s}").parse().unwrap(),
            )
            .map(|q| q.0.propose)
        };
        assert_eq!(q("propose=1").ok(), Some(true));
        assert_eq!(q("propose=true").ok(), Some(true));
        assert_eq!(q("propose=0").ok(), Some(false));
        assert_eq!(q("").ok(), Some(false));
        assert!(q("propose=maybe").is_err());
    }
}
