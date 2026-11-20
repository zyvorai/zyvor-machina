// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM network policies (CiliumNetworkPolicy schema) for this host, compiled
// into the local machina-bpfd VM edge, plus VM labels and the flow API.
// When the controller owns the host's VM edge, local policies stay stored
// but inactive so the two writers never replace each other's state.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::Stream;
use machina_bpf::api::{
    Request, VmEdgeStatus, VmFlowEdge, VmFlowRecord, VmFqdnEntry, VmQuarantineBody, VmThreatStatus,
};
use machina_bpf::netpol::{
    self, evidence, jit, nl, threat, FlowFilter, Inputs, LearnOptions, NetpolService, NetpolVm,
    ReplayInputs, TraceQuery, VmNetworkPolicy,
};
use machina_bpf::BpfdClient;
use machina_core::{LibvirtError, LibvirtManager};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::bpf::require_admin;
use crate::auth::RequestActor;
use crate::error::AppError;

const STORE: &str = "/var/lib/machina/vm-netpol.json";
const OWNER: &str = "daemon";
const RESYNC_SECS: u64 = 60;

#[derive(Serialize, Deserialize, Default)]
struct Store {
    policies: Vec<VmNetworkPolicy>,
    /// The local bpfd VM edge currently carries our compiled state.
    #[serde(default)]
    synced: bool,
}

#[derive(Serialize, Clone, Default)]
struct SyncReport {
    at: Option<String>,
    ok: bool,
    error: Option<String>,
    /// Why nothing was pushed (no policies, controller-managed, ...).
    skipped: Option<String>,
    vms: usize,
    rules: usize,
    peers: usize,
    warnings: Vec<String>,
}

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Wakes the resync loop so it re-arms for a new expiry.
static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static LAST: Mutex<Option<SyncReport>> = Mutex::new(None);
static INVENTORY: Mutex<Vec<NetpolVm>> = Mutex::new(Vec::new());
static MANAGER: OnceLock<LibvirtManager> = OnceLock::new();
static SERVICES: Mutex<Option<(Instant, Vec<NetpolService>)>> = Mutex::new(None);

/// Kubernetes services for `toServices`, refreshed at most once a minute
/// and only fetched while a policy uses them.
async fn services(policies: &[VmNetworkPolicy]) -> Vec<NetpolService> {
    if !netpol::uses_services(policies) {
        return Vec::new();
    }
    if let Some((at, s)) = SERVICES.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return s.clone();
        }
    }
    let s = super::k8s::netpol_services().await;
    *SERVICES.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), s.clone()));
    s
}

fn load_store_all() -> Store {
    std::fs::read_to_string(STORE)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default()
}

/// The store without expired temporary policies (`sync` deletes those).
fn load_store() -> Store {
    let mut s = load_store_all();
    let now = chrono::Utc::now();
    s.policies.retain(|p| !jit::expired(p, now));
    s
}

/// Seconds until the next temporary policy expires.
fn next_expiry_secs() -> Option<u64> {
    let now = chrono::Utc::now();
    load_store_all()
        .policies
        .iter()
        .filter_map(jit::expires_at)
        .map(|t| (t - now).num_seconds().max(0) as u64)
        .min()
}

fn save_store(s: &Store) -> Result<(), LibvirtError> {
    if let Some(dir) = std::path::Path::new(STORE).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let data =
        serde_json::to_string_pretty(s).map_err(LibvirtError::map_op("serialize policies"))?;
    let tmp = format!("{STORE}.tmp");
    std::fs::write(&tmp, data).map_err(LibvirtError::map_op("write policies"))?;
    std::fs::rename(&tmp, STORE).map_err(LibvirtError::map_op("write policies"))?;
    Ok(())
}

/// Global unicast addresses of this host (`host` entity).
fn host_addresses() -> Vec<String> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-j", "addr", "show"])
        .output()
    else {
        return Vec::new();
    };
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let mut addrs = Vec::new();
    for link in v.as_array().into_iter().flatten() {
        if link["ifname"].as_str() == Some("lo") {
            continue;
        }
        for a in link["addr_info"].as_array().into_iter().flatten() {
            if a["scope"].as_str() == Some("global") {
                if let Some(ip) = a["local"].as_str() {
                    addrs.push(ip.to_string());
                }
            }
        }
    }
    addrs.sort();
    addrs.dedup();
    addrs
}

fn usable_addr(a: &str) -> bool {
    match a.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(v6)) => !v6.is_loopback() && (v6.segments()[0] & 0xffc0) != 0xfe80,
        Err(_) => false,
    }
}

/// Local VMs with labels and guest addresses (lease / ARP / agent).
fn inventory(m: &LibvirtManager) -> Vec<NetpolVm> {
    let labels = machina_core::libvirt::extras::load_labels();
    let vms = m.list_all_vms().unwrap_or_default();
    vms.into_iter()
        .map(|vm| {
            let mut addresses: Vec<String> = Vec::new();
            if vm.state == "running" {
                if let Ok(rows) = m.with_conn(|c| {
                    machina_core::libvirt::guest_agent::get_guest_interfaces(c, &vm.name)
                }) {
                    addresses.extend(rows.into_iter().map(|r| r.address));
                }
                addresses.extend(vm.guest_ip.clone());
            }
            addresses.retain(|a| usable_addr(a));
            addresses.sort();
            addresses.dedup();
            NetpolVm {
                labels: labels.get(&vm.name).cloned().unwrap_or_default(),
                name: vm.name,
                addresses,
                ..Default::default()
            }
        })
        .collect()
}

async fn refresh_inventory(m: &LibvirtManager) -> Vec<NetpolVm> {
    let m = m.clone();
    let inv = tokio::task::spawn_blocking(move || inventory(&m))
        .await
        .unwrap_or_default();
    *INVENTORY.lock().unwrap_or_else(|e| e.into_inner()) = inv.clone();
    inv
}

async fn cached_inventory(m: &LibvirtManager) -> Vec<NetpolVm> {
    let cached = INVENTORY.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if cached.is_empty() {
        refresh_inventory(m).await
    } else {
        cached
    }
}

async fn edge_status() -> Option<VmEdgeStatus> {
    let v = BpfdClient::from_env()
        .call(&Request::VmEdgeStatus)
        .await
        .ok()?;
    serde_json::from_value(v).ok()
}

/// Compile the stored policies and push them to the local bpfd.
async fn sync(m: &LibvirtManager) -> SyncReport {
    let _g = STORE_LOCK.lock().await;
    let mut store = load_store_all();
    let now = chrono::Utc::now();
    let (expired, live): (Vec<_>, Vec<_>) =
        store.policies.drain(..).partition(|p| jit::expired(p, now));
    store.policies = live;
    if !expired.is_empty() {
        for p in &expired {
            tracing::info!(policy = %p.name, "temporary VM network policy expired");
        }
        if let Err(e) = save_store(&store) {
            tracing::warn!("vm netpol store: {e}");
        }
    }
    let mut rep = SyncReport {
        at: Some(chrono::Utc::now().to_rfc3339()),
        ..Default::default()
    };
    if store.policies.is_empty() && !store.synced {
        rep.ok = true;
        rep.skipped = Some("no policies".into());
    } else if edge_status().await.is_some_and(|s| s.owner == "controller") {
        rep.ok = true;
        rep.skipped =
            Some("the controller manages this host's VM edge; local policies are inactive".into());
    } else {
        let inv = refresh_inventory(m).await;
        let hosts = tokio::task::spawn_blocking(host_addresses)
            .await
            .unwrap_or_default();
        let svcs = services(&store.policies).await;
        let c = netpol::compile(&Inputs {
            policies: &store.policies,
            vms: &inv,
            services: &svcs,
            host: None,
            host_addresses: &hosts,
            remote_node_addresses: &[],
        });
        let mut state = c.state;
        state.owner = if store.policies.is_empty() {
            String::new()
        } else {
            OWNER.into()
        };
        if store.policies.is_empty() {
            state.flow_log = false;
        }
        rep.vms = state.vms.len();
        rep.rules = state.policy.len();
        rep.peers = state.peers.len();
        rep.warnings = c.warnings;
        match BpfdClient::from_env()
            .call(&Request::VmEdgeSync { state })
            .await
        {
            Ok(_) => {
                rep.ok = true;
                store.synced = !store.policies.is_empty();
                if let Err(e) = save_store(&store) {
                    tracing::warn!("vm netpol store: {e}");
                }
            }
            Err(e) => rep.error = Some(format!("{e:#}")),
        }
    }
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(rep.clone());
    rep
}

/// Periodic resync (guest addresses change with DHCP) and lifecycle hooks.
pub fn spawn_resync_loop(m: LibvirtManager) {
    let _ = MANAGER.set(m.clone());
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            let rep = sync(&m).await;
            if let Some(e) = rep.error {
                tracing::debug!("vm netpol sync: {e}");
            }
            let wait = next_expiry_secs().map_or(RESYNC_SECS, |s| s.clamp(1, RESYNC_SECS));
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {}
                _ = WAKE.notified() => {}
            }
        }
    });
    tokio::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            threat_refresh_due().await;
        }
    });
}

/// Called after VM start/stop/label changes.
pub fn trigger_resync() {
    if let Some(m) = MANAGER.get().cloned() {
        tokio::spawn(async move {
            sync(&m).await;
        });
    }
}

fn bad_request(msg: &str, v: &netpol::Validation) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg, "errors": v.errors, "warnings": v.warnings })),
    )
        .into_response()
}

/// Body: raw YAML/JSON policy text, or `{"yaml": "..."}`.
fn body_text(body: &Bytes) -> String {
    let text = String::from_utf8_lossy(body).into_owned();
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) if m.get("yaml").is_some_and(Value::is_string) => {
            m["yaml"].as_str().unwrap_or_default().to_string()
        }
        _ => text,
    }
}

fn policy_json(p: &VmNetworkPolicy, selected: Option<&Vec<String>>) -> Value {
    json!({
        "name": p.name,
        "kind": p.kind,
        "description": p.description(),
        "labels": p.labels,
        "annotations": p.annotations,
        "specs": p.specs,
        "yaml": p.to_yaml(),
        "selected_vms": selected.cloned().unwrap_or_default(),
    })
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

async fn compile_cached(m: &LibvirtManager, policies: &[VmNetworkPolicy]) -> netpol::Compiled {
    let inv = cached_inventory(m).await;
    let hosts = tokio::task::spawn_blocking(host_addresses)
        .await
        .unwrap_or_default();
    let svcs = services(policies).await;
    netpol::compile(&Inputs {
        policies,
        vms: &inv,
        services: &svcs,
        host: None,
        host_addresses: &hosts,
        remote_node_addresses: &[],
    })
}

async fn list(State(m): State<LibvirtManager>) -> Result<Json<Value>, AppError> {
    let store = load_store();
    let c = compile_cached(&m, &store.policies).await;
    let sel = selected_map(&c);
    let items: Vec<Value> = store
        .policies
        .iter()
        .map(|p| policy_json(p, sel.get(&p.name)))
        .collect();
    Ok(Json(json!({ "items": items, "warnings": c.warnings })))
}

#[derive(Deserialize, Default)]
struct ApplyQuery {
    #[serde(default)]
    dry_run: Option<String>,
}

fn truthy(s: &Option<String>) -> bool {
    matches!(s.as_deref(), Some("1" | "true" | "yes" | ""))
}

async fn preview(m: &LibvirtManager, parsed: &[VmNetworkPolicy], v: &netpol::Validation) -> Value {
    let mut all = load_store().policies;
    all.retain(|p| !parsed.iter().any(|n| n.name == p.name));
    all.extend(parsed.iter().cloned());
    let c = compile_cached(m, &all).await;
    let sel = selected_map(&c);
    json!({
        "valid": v.ok(),
        "errors": v.errors,
        "warnings": v.warnings,
        "compile_warnings": c.warnings,
        "policies": parsed.iter().map(|p| policy_json(p, sel.get(&p.name))).collect::<Vec<_>>(),
        "rules": c.state.policy.len(),
        "endpoints": c.endpoints,
        "selectors": c.selectors.into_iter().filter(|s| parsed.iter().any(|p| p.name == s.policy)).collect::<Vec<_>>(),
    })
}

async fn apply(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<ApplyQuery>,
    body: Bytes,
) -> Result<Response, AppError> {
    let (parsed, v) = netpol::parse_documents(&body_text(&body));
    if truthy(&q.dry_run) {
        return Ok(Json(preview(&m, &parsed, &v).await).into_response());
    }
    require_admin(&actor, "Changing VM network policies")?;
    if !v.ok() {
        return Ok(bad_request("invalid policy", &v));
    }
    let names: Vec<String> = parsed.iter().map(|p| p.name.clone()).collect();
    {
        let _g = STORE_LOCK.lock().await;
        let mut store = load_store();
        let mut created = Vec::new();
        for p in parsed {
            match store.policies.iter_mut().find(|x| x.name == p.name) {
                Some(x) => *x = p,
                None => {
                    created.push(p.name.clone());
                    store.policies.push(p);
                }
            }
        }
        store.policies.sort_by(|a, b| a.name.cmp(&b.name));
        save_store(&store)?;
        tracing::info!(actor = %actor.username, policies = ?names, "vm network policies applied");
    }
    let rep = sync(&m).await;
    Ok(Json(json!({ "applied": names, "warnings": v.warnings, "sync": rep })).into_response())
}

async fn validate(State(m): State<LibvirtManager>, body: Bytes) -> Json<Value> {
    let (parsed, v) = netpol::parse_documents(&body_text(&body));
    Json(preview(&m, &parsed, &v).await)
}

#[derive(Deserialize, Default)]
struct GetQuery {
    #[serde(default)]
    format: Option<String>,
    /// Evidence matrix probes, `tcp/22,udp/53`.
    #[serde(default)]
    probes: Option<String>,
}

async fn get_one(
    State(m): State<LibvirtManager>,
    Path(name): Path<String>,
    Query(q): Query<GetQuery>,
) -> Result<Response, AppError> {
    let store = load_store();
    let p = store
        .policies
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| LibvirtError::NotFound(format!("VM network policy `{name}`")))?;
    if q.format.as_deref() == Some("yaml") {
        return Ok(([(header::CONTENT_TYPE, "application/yaml")], p.to_yaml()).into_response());
    }
    let c = compile_cached(&m, &store.policies).await;
    let sel = selected_map(&c);
    Ok(Json(policy_json(p, sel.get(&p.name))).into_response())
}

async fn delete_one(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing VM network policies")?;
    {
        let _g = STORE_LOCK.lock().await;
        let mut store = load_store();
        let before = store.policies.len();
        store.policies.retain(|p| p.name != name);
        if store.policies.len() == before {
            return Err(LibvirtError::NotFound(format!("VM network policy `{name}`")).into());
        }
        save_store(&store)?;
    }
    let rep = sync(&m).await;
    Ok(Json(json!({ "deleted": name, "sync": rep })))
}

async fn trace(
    State(m): State<LibvirtManager>,
    Json(q): Json<TraceQuery>,
) -> Result<Json<Value>, AppError> {
    let store = load_store();
    let inv = cached_inventory(&m).await;
    let hosts = tokio::task::spawn_blocking(host_addresses)
        .await
        .unwrap_or_default();
    let svcs = services(&store.policies).await;
    let r = netpol::trace(&store.policies, &inv, &svcs, &hosts, &[], &q)
        .map_err(LibvirtError::Invalid)?;
    Ok(Json(serde_json::to_value(r).unwrap_or_default()))
}

async fn endpoints(State(m): State<LibvirtManager>) -> Json<Value> {
    let c = compile_cached(&m, &load_store().policies).await;
    Json(json!({ "items": c.endpoints }))
}

async fn selectors(State(m): State<LibvirtManager>) -> Json<Value> {
    let c = compile_cached(&m, &load_store().policies).await;
    Json(json!({ "items": c.selectors }))
}

async fn status() -> Json<Value> {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let edge = edge_status().await;
    let mode = BpfdClient::from_env()
        .call(&Request::Status)
        .await
        .ok()
        .map(|s| json!({ "mode": s["mode"], "lease_remaining_secs": s["lease_remaining_secs"] }));
    let store = load_store();
    let managed_by = match edge.as_ref().map(|e| e.owner.as_str()) {
        Some("controller") => "controller",
        _ => "local",
    };
    Json(json!({
        "policies": store.policies.len(),
        "managed_by": managed_by,
        "last_sync": last,
        "edge": edge,
        "enforcement": mode,
        "bpfd_available": edge.is_some(),
        "cilium": netpol::cilium_present(),
    }))
}

async fn fqdn_cache() -> Result<Json<Value>, AppError> {
    let v = BpfdClient::from_env()
        .call(&Request::VmFqdnCache)
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))?;
    Ok(Json(json!({ "items": v })))
}

async fn auth_table() -> Result<Json<Value>, AppError> {
    let v = BpfdClient::from_env()
        .call(&Request::VmAuthTable)
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))?;
    Ok(Json(json!({ "items": v })))
}

#[derive(Deserialize, Default)]
struct FlowQuery {
    limit: Option<usize>,
    /// Backfill this many recent flows before streaming.
    last: Option<usize>,
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
            host: None,
        }
    }
}

async fn recent_flows(f: &FlowFilter, limit: usize) -> Result<Vec<VmFlowRecord>, AppError> {
    let v = BpfdClient::from_env()
        .call(&Request::VmFlows {
            limit: Some(5000),
            vm: None,
            verdict: None,
        })
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))?;
    let all: Vec<VmFlowRecord> = serde_json::from_value(v).unwrap_or_default();
    Ok(all
        .into_iter()
        .filter(|r| f.matches(r))
        .take(limit)
        .collect())
}

async fn flows(Query(q): Query<FlowQuery>) -> Result<Json<Value>, AppError> {
    let items = recent_flows(&q.filter(), q.limit.unwrap_or(200).min(5000)).await?;
    Ok(Json(json!({ "items": items })))
}

async fn flow_stream(
    Query(q): Query<FlowQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let f = q.filter();
    let mut backfill = match q.last.filter(|n| *n > 0) {
        Some(n) => recent_flows(&f, n.min(5000)).await.unwrap_or_default(),
        None => Vec::new(),
    };
    backfill.reverse();
    let rx = BpfdClient::from_env()
        .subscribe(&["flow"])
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))?;
    let ev = |r: &VmFlowRecord| {
        Event::default()
            .event("flow")
            .data(serde_json::to_string(r).unwrap_or_default())
    };
    let head = futures_util::stream::iter(backfill.iter().map(ev).map(Ok).collect::<Vec<_>>());
    let live = futures_util::StreamExt::filter_map(
        tokio_stream::wrappers::ReceiverStream::new(rx),
        move |e| {
            let out = serde_json::from_value::<VmFlowRecord>(e.event)
                .ok()
                .filter(|r| f.matches(r))
                .map(|r| Ok(ev(&r)));
            std::future::ready(out)
        },
    );
    Ok(Sse::new(futures_util::StreamExt::chain(head, live)).keep_alive(KeepAlive::default()))
}

async fn bpfd_call(req: &Request) -> Result<Value, AppError> {
    BpfdClient::from_env()
        .call(req)
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")).into())
}

#[derive(Deserialize, Default)]
struct EdgeQuery {
    vm: Option<String>,
    limit: Option<usize>,
}

async fn history(vm: Option<String>) -> Result<Vec<VmFlowEdge>, AppError> {
    let v = bpfd_call(&Request::VmFlowEdges { vm }).await?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

async fn flow_edges(Query(q): Query<EdgeQuery>) -> Result<Json<Value>, AppError> {
    let items = history(q.vm.filter(|v| !v.is_empty())).await?;
    Ok(Json(json!({ "items": items })))
}

async fn flow_edges_reset(
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Clearing the flow history")?;
    bpfd_call(&Request::VmFlowEdgesReset {}).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn flow_alerts(Query(q): Query<EdgeQuery>) -> Result<Json<Value>, AppError> {
    let v = bpfd_call(&Request::VmFlowAlerts {
        limit: Some(q.limit.unwrap_or(200).min(1000)),
    })
    .await?;
    Ok(Json(json!({ "items": v })))
}

async fn quarantine(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(b): Json<VmQuarantineBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Quarantining a VM")?;
    let reason = b.reason.clone();
    let v = bpfd_call(&b.into_request(name.clone(), actor.username.clone())).await?;
    tracing::warn!(actor = %actor.username, vm = %name, %reason, "VM quarantined");
    Ok(Json(v))
}

async fn quarantine_release(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Releasing a VM quarantine")?;
    let v = bpfd_call(&Request::VmQuarantineRelease { vm: name.clone() }).await?;
    tracing::warn!(actor = %actor.username, vm = %name, "VM quarantine released");
    Ok(Json(v))
}

async fn quarantines() -> Result<Json<Value>, AppError> {
    let v = bpfd_call(&Request::VmQuarantines).await?;
    Ok(Json(json!({ "items": v })))
}

async fn jit_grant(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<jit::JitRequest>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Granting temporary network access")?;
    req.validate().map_err(LibvirtError::Invalid)?;
    if edge_status().await.is_some_and(|s| s.owner == "controller") {
        return Err(LibvirtError::Invalid(
            "the controller manages this host's VM edge; request access through the controller (--fleet)".into(),
        )
        .into());
    }
    let inv = refresh_inventory(&m).await;
    for vm in [&req.to, &req.from] {
        if vm != "host" && !inv.iter().any(|v| &v.name == vm) {
            return Err(LibvirtError::NotFound(format!("VM `{vm}`")).into());
        }
    }
    let now = chrono::Utc::now();
    let p = req
        .policy(&actor.username, now)
        .map_err(LibvirtError::Invalid)?;
    {
        let _g = STORE_LOCK.lock().await;
        let mut store = load_store_all();
        store.policies.push(p.clone());
        store.policies.sort_by(|a, b| a.name.cmp(&b.name));
        save_store(&store)?;
    }
    tracing::info!(actor = %actor.username, policy = %p.name, access = %req.what(),
        secs = req.secs(), reason = %req.reason, "temporary VM network access granted");
    let rep = sync(&m).await;
    WAKE.notify_one();
    let grant = jit::grants(std::slice::from_ref(&p), now).pop();
    Ok(Json(
        json!({ "granted": grant, "policy": p.name, "sync": rep }),
    ))
}

async fn jit_list() -> Json<Value> {
    let items = jit::grants(&load_store().policies, chrono::Utc::now());
    Json(json!({ "items": items }))
}

async fn controller_owned() -> bool {
    edge_status().await.is_some_and(|s| s.owner == "controller")
}

async fn fetch_feed(url: &str) -> Result<String, LibvirtError> {
    let op = LibvirtError::Operation;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| op(format!("feed client: {e}")))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| op(format!("fetch {url}: {e}")))?;
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| op(format!("fetch {url}: {e}")))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > threat::MAX_FEED_BYTES {
            return Err(op(format!(
                "feed {url} is larger than {} MiB",
                threat::MAX_FEED_BYTES >> 20
            )));
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn bpfd_raw(req: &Request) -> Result<Value, LibvirtError> {
    BpfdClient::from_env()
        .call(req)
        .await
        .map_err(|e| LibvirtError::Operation(format!("{e:#}")))
}

async fn threat_status() -> Result<VmThreatStatus, LibvirtError> {
    let v = bpfd_raw(&Request::VmThreatFeeds).await?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

async fn threat_push(name: &str, b: &threat::FeedBody) -> Result<Value, LibvirtError> {
    b.validate().map_err(LibvirtError::Invalid)?;
    let fetched = match &b.url {
        Some(u) => Some(fetch_feed(u).await?),
        None => None,
    };
    let domains = b
        .domains(fetched.as_deref())
        .map_err(LibvirtError::Invalid)?;
    bpfd_raw(&Request::VmThreatFeedSet {
        name: name.to_string(),
        source: b.source(),
        block: b.block,
        domains,
    })
    .await
}

async fn threat_feeds() -> Result<Json<Value>, AppError> {
    Ok(Json(
        serde_json::to_value(threat_status().await?).unwrap_or_default(),
    ))
}

fn fleet_managed() -> AppError {
    LibvirtError::Invalid(
        "the controller manages this host's VM edge; change threat feeds through the controller (--fleet)".into(),
    )
    .into()
}

async fn threat_feed_set(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(b): Json<threat::FeedBody>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing DNS threat feeds")?;
    if controller_owned().await {
        return Err(fleet_managed());
    }
    let v = threat_push(&name, &b).await?;
    tracing::warn!(actor = %actor.username, feed = %name, block = b.block, "DNS threat feed set");
    Ok(Json(v))
}

async fn threat_feed_remove(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Changing DNS threat feeds")?;
    if controller_owned().await {
        return Err(fleet_managed());
    }
    let v = bpfd_call(&Request::VmThreatFeedRemove { name: name.clone() }).await?;
    tracing::warn!(actor = %actor.username, feed = %name, "DNS threat feed removed");
    Ok(Json(v))
}

/// Fetch a URL feed again, keeping its block setting.
async fn threat_refresh_one(name: &str) -> Result<Value, LibvirtError> {
    let st = threat_status().await?;
    let f = st
        .feeds
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| LibvirtError::NotFound(format!("threat feed `{name}`")))?;
    if f.source.is_empty() {
        return Err(LibvirtError::Invalid(format!(
            "threat feed `{name}` is an inline list; set it again to change it"
        )));
    }
    let b = threat::FeedBody {
        url: Some(f.source.clone()),
        block: f.block,
        ..Default::default()
    };
    threat_push(name, &b).await
}

async fn threat_feed_refresh(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Refreshing a DNS threat feed")?;
    if controller_owned().await {
        return Err(fleet_managed());
    }
    Ok(Json(threat_refresh_one(&name).await?))
}

/// Refetch URL feeds older than [`threat::REFRESH_SECS`].
async fn threat_refresh_due() {
    if controller_owned().await {
        return;
    }
    let Ok(st) = threat_status().await else {
        return;
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(threat::REFRESH_SECS as i64);
    for f in st.feeds.iter().filter(|f| !f.source.is_empty()) {
        if !matches!(chrono::DateTime::parse_from_rfc3339(&f.updated), Ok(t) if t >= cutoff) {
            match threat_refresh_one(&f.name).await {
                Ok(_) => tracing::info!(feed = %f.name, "DNS threat feed refreshed"),
                Err(e) => tracing::warn!(feed = %f.name, "DNS threat feed refresh: {e}"),
            }
        }
    }
}

/// Address → DNS names from the `toFQDNs` cache.
async fn fqdn_names() -> BTreeMap<String, Vec<String>> {
    let entries: Vec<VmFqdnEntry> = match bpfd_call(&Request::VmFqdnCache).await {
        Ok(v) => serde_json::from_value(v).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in entries {
        let names = m.entry(e.address).or_default();
        if !names.contains(&e.name) {
            names.push(e.name);
        }
    }
    m
}

async fn learn(
    State(m): State<LibvirtManager>,
    Json(mut o): Json<LearnOptions>,
) -> Result<Json<Value>, AppError> {
    let edges = history(None).await?;
    let inv = cached_inventory(&m).await;
    o.fqdn.extend(fqdn_names().await);
    let r = tokio::task::spawn_blocking(move || netpol::learn(&edges, &inv, &o))
        .await
        .map_err(|e| LibvirtError::Operation(format!("learn: {e}")))?;
    Ok(Json(serde_json::to_value(r).unwrap_or_default()))
}

#[derive(Deserialize)]
struct ReplayBody {
    yaml: String,
    #[serde(default)]
    limit: Option<usize>,
}

async fn replay(
    State(m): State<LibvirtManager>,
    Json(b): Json<ReplayBody>,
) -> Result<Response, AppError> {
    let (parsed, v) = netpol::parse_documents(&b.yaml);
    if !v.ok() {
        return Ok(bad_request("invalid policy", &v));
    }
    let limit = b.limit.unwrap_or(5000).min(20_000);
    Ok(Json(replay_draft(&m, parsed, limit).await?).into_response())
}

/// Replay recorded flows against the stored policies with `parsed` added
/// or replacing same-named ones.
async fn replay_draft(
    m: &LibvirtManager,
    parsed: Vec<VmNetworkPolicy>,
    limit: usize,
) -> Result<Value, AppError> {
    let current = load_store().policies;
    let mut draft = current.clone();
    draft.retain(|p| !parsed.iter().any(|n| n.name == p.name));
    draft.extend(parsed);
    let edges = history(None).await?;
    let inv = cached_inventory(m).await;
    let hosts = tokio::task::spawn_blocking(host_addresses)
        .await
        .unwrap_or_default();
    let svcs = services(&draft).await;
    let fqdn = fqdn_names().await;
    let r = tokio::task::spawn_blocking(move || {
        netpol::replay(
            &edges,
            &current,
            &draft,
            &ReplayInputs {
                vms: &inv,
                services: &svcs,
                host_addresses: &hosts,
                remote_node_addresses: &[],
                fqdn: &fqdn,
                limit,
            },
        )
    })
    .await
    .map_err(|e| LibvirtError::Operation(format!("replay: {e}")))?;
    Ok(serde_json::to_value(r).unwrap_or_default())
}

#[derive(Deserialize)]
struct DraftBody {
    prompt: String,
}

/// Draft policies from plain English with the sentence parser (the LLM
/// lives in the controller), then validate and replay them.
async fn draft(
    State(m): State<LibvirtManager>,
    Json(b): Json<DraftBody>,
) -> Result<Response, AppError> {
    let prompt = b.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > nl::MAX_PROMPT {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("describe the policy in 1–{} characters", nl::MAX_PROMPT) })),
        )
            .into_response());
    }
    let inv = cached_inventory(&m).await;
    let d = nl::draft_rules(prompt, &inv);
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
    let pv = preview(&m, &parsed, &v).await;
    let rp = replay_draft(&m, parsed, 5000).await?;
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

async fn evidence(
    State(m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<GetQuery>,
) -> Result<Response, AppError> {
    let probes =
        evidence::parse_probes(q.probes.as_deref().unwrap_or("")).map_err(LibvirtError::Invalid)?;
    let store = load_store();
    let inv = refresh_inventory(&m).await;
    let haddr = tokio::task::spawn_blocking(host_addresses)
        .await
        .unwrap_or_default();
    let svcs = services(&store.policies).await;
    let c = compile_cached(&m, &store.policies).await;
    let sel = selected_map(&c);
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default();
    let edge = edge_status().await;
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let host = evidence::HostEvidence {
        hostname: hostname.clone(),
        reachable: edge.is_some(),
        enforcing: edge.as_ref().is_some_and(|e| e.enforcing),
        owner: edge.as_ref().map(|e| e.owner.clone()).unwrap_or_default(),
        synced_at: last.as_ref().and_then(|l| l.at.clone()),
        in_sync: edge.is_some() && last.as_ref().is_some_and(|l| l.ok),
        error: last.as_ref().and_then(|l| l.error.clone()),
    };
    let policies = store
        .policies
        .iter()
        .map(|p| evidence::PolicyEvidence {
            name: p.name.clone(),
            kind: p.kind.clone(),
            enabled: true,
            generated: false,
            description: p.description(),
            sha256: evidence::policy_hash(p),
            selected_vms: sel.get(&p.name).cloned().unwrap_or_default(),
            updated_at: String::new(),
        })
        .collect();
    let (kind, groups) = evidence::groups(&inv);
    let (pol, vms, pr) = (store.policies.clone(), inv.clone(), probes.clone());
    let matrix = tokio::task::spawn_blocking(move || {
        evidence::matrix(&pol, &vms, &svcs, &haddr, &groups, &pr)
    })
    .await
    .map_err(|e| LibvirtError::Operation(format!("matrix: {e}")))?;
    let denied = evidence::denied(&history(None).await.unwrap_or_default());
    let items = |v: Result<Value, AppError>| {
        v.ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };
    let now = chrono::Utc::now();
    let mut e = evidence::Evidence {
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        generated_by: actor.username.clone(),
        scope: format!("host {hostname}"),
        source: "machina-daemon".into(),
        hosts: vec![host],
        policies,
        matrix_groups: kind,
        matrix,
        alerts: items(bpfd_call(&Request::VmFlowAlerts { limit: Some(50) }).await),
        quarantines: items(bpfd_call(&Request::VmQuarantines).await),
        temporary_access: jit::grants(&store.policies, now)
            .iter()
            .map(|g| serde_json::to_value(g).unwrap_or_default())
            .collect(),
        threat_feeds: threat_status()
            .await
            .map(|t| t.feeds)
            .unwrap_or_default()
            .iter()
            .map(|f| serde_json::to_value(f).unwrap_or_default())
            .collect(),
        egress_ips: bpfd_call(&Request::VmEgressSnatStatus)
            .await
            .ok()
            .and_then(|v| v["rules"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|r| json!({ "hostname": hostname, "project": r["project"], "egress_ip": r["egress_ip"], "sources": r["sources"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")) }))
            .collect(),
        denied: denied.clone(),
        matrix_probes: probes.iter().map(|(p, n)| format!("{p}/{n}")).collect(),
        ..Default::default()
    };
    e.summary.vms = inv.len();
    e.seal(denied.len());
    tracing::info!(actor = %actor.username, digest = %e.digest, "segmentation evidence exported");
    let stamp = e.generated_at.replace([':', '-'], "");
    Ok(match q.format.as_deref() {
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
            evidence::markdown(&e),
        )
            .into_response(),
        _ => Json(e).into_response(),
    })
}

async fn get_labels(Path(name): Path<String>) -> Json<Value> {
    let labels = machina_core::libvirt::extras::get_vm_labels(&name);
    Json(json!({ "name": name, "labels": labels }))
}

#[derive(Deserialize)]
struct LabelsBody {
    labels: BTreeMap<String, String>,
}

async fn put_labels(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(b): Json<LabelsBody>,
) -> Result<Json<Value>, AppError> {
    crate::auth::require_write(&actor, "vms:write")?;
    if name.contains('/') || name.contains("..") {
        return Err(LibvirtError::Invalid("bad VM name".into()).into());
    }
    machina_core::libvirt::extras::set_vm_labels(&name, b.labels.clone())?;
    if let Ok(mut inv) = INVENTORY.lock() {
        if let Some(vm) = inv.iter_mut().find(|v| v.name == name) {
            vm.labels = b.labels.clone();
        }
    }
    trigger_resync();
    Ok(Json(
        json!({ "status": "ok", "name": name, "labels": b.labels }),
    ))
}

pub fn netpol_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/vm-network-policies", get(list).post(apply))
        .route("/vm-network-policies/validate", post(validate))
        .route("/vm-network-policies/trace", post(trace))
        .route("/vm-network-policies/endpoints", get(endpoints))
        .route("/vm-network-policies/selectors", get(selectors))
        .route("/vm-network-policies/status", get(status))
        .route("/vm-network-policies/fqdn-cache", get(fqdn_cache))
        .route("/vm-network-policies/auth", get(auth_table))
        .route("/vm-network-policies/learn", post(learn))
        .route("/vm-network-policies/replay", post(replay))
        .route("/vm-network-policies/draft", post(draft))
        .route("/vm-network-policies/evidence", get(evidence))
        .route("/vm-network-policies/quarantines", get(quarantines))
        .route("/vm-network-policies/jit", get(jit_list).post(jit_grant))
        .route("/vm-network-policies/threat-feeds", get(threat_feeds))
        .route(
            "/vm-network-policies/threat-feeds/{name}",
            axum::routing::put(threat_feed_set)
                .delete(threat_feed_remove)
                .layer(axum::extract::DefaultBodyLimit::max(threat::MAX_FEED_BYTES)),
        )
        .route(
            "/vm-network-policies/threat-feeds/{name}/refresh",
            post(threat_feed_refresh),
        )
        .route(
            "/vms/{name}/quarantine",
            post(quarantine).delete(quarantine_release),
        )
        .route(
            "/vm-network-policies/{name}",
            get(get_one).delete(delete_one),
        )
        .route("/flows", get(flows))
        .route("/flows/stream", get(flow_stream))
        .route("/flows/edges", get(flow_edges).delete(flow_edges_reset))
        .route("/flows/alerts", get(flow_alerts))
        .route("/vms/{name}/labels", get(get_labels).put(put_labels))
}
