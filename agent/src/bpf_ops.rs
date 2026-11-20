// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native eBPF RPCs: thin bridge from gRPC to the host's machina-bpfd socket.

use std::collections::HashSet;

use machina_bpf::api::{Policy, Request};
use machina_bpf::BpfdClient;
use serde_json::{json, Value};

/// Forward one machina-bpfd request (JSON) and return its `data`.
pub async fn call(request_json: &str) -> anyhow::Result<Value> {
    let req: Request = serde_json::from_str(request_json)
        .map_err(|e| anyhow::anyhow!("invalid machina-bpfd request: {e}"))?;
    if matches!(req, Request::Subscribe { .. }) {
        anyhow::bail!("use BpfSubscribe for event streams");
    }
    BpfdClient::from_env().call(&req).await
}

/// Today's firewall activity from bpfd deny events and anomalies. Without
/// bpfd the counters are zero and `note` says why.
pub async fn firewall_activity() -> Value {
    let client = BpfdClient::from_env();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let events: Vec<Value> = match client
        .call_as(&Request::Events {
            limit: Some(5000),
            kind: None,
        })
        .await
    {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "blocked_today": 0,
                "allowed_today": 0,
                "suspicious_scans": 0,
                "new_open_ports": 0,
                "events": [],
                "note": format!("native eBPF unavailable: {e:#}"),
            })
        }
    };
    let anomalies: Vec<Value> = client
        .call_as(&Request::Anomalies { limit: Some(1000) })
        .await
        .unwrap_or_default();
    let is_today = |e: &Value| e["ts"].as_str().is_some_and(|t| t.starts_with(&today));
    let blocked = events
        .iter()
        .filter(|e| is_today(e) && e["verdict"] == "drop")
        .count();
    let allowed = events
        .iter()
        .filter(|e| is_today(e) && e["kind"] == "flow_open")
        .count();
    let scans = anomalies
        .iter()
        .filter(|a| is_today(a) && matches!(a["kind"].as_str(), Some("port_scan" | "inbound_scan")))
        .count();
    let recent: Vec<&Value> = events
        .iter()
        .filter(|e| e["kind"] != "flow_open" && e["kind"] != "flow_close")
        .take(50)
        .collect();
    json!({
        "blocked_today": blocked,
        "allowed_today": allowed,
        "suspicious_scans": scans,
        "new_open_ports": 0,
        "events": recent,
        "source": "machina-bpf",
    })
}

/// Make the controller-owned subset of bpfd policies equal `policies`.
pub async fn sync_policies(policies_json: &str, owned_prefix: &str) -> anyhow::Result<Value> {
    if owned_prefix.is_empty() {
        anyhow::bail!("owned_prefix is required (refusing to prune unowned policies)");
    }
    let desired: Vec<Policy> = serde_json::from_str(policies_json)
        .map_err(|e| anyhow::anyhow!("invalid policies: {e}"))?;
    if let Some(p) = desired.iter().find(|p| !p.id.starts_with(owned_prefix)) {
        anyhow::bail!("policy {} is outside the owned prefix {owned_prefix}", p.id);
    }
    let client = BpfdClient::from_env();
    let current: Vec<Policy> = client.call_as(&Request::ListPolicies).await?;
    let wanted: HashSet<&str> = desired.iter().map(|p| p.id.as_str()).collect();

    let mut removed = Vec::new();
    for p in current.iter().filter(|p| p.id.starts_with(owned_prefix)) {
        if !wanted.contains(p.id.as_str()) {
            client
                .call(&Request::RemovePolicy { id: p.id.clone() })
                .await?;
            removed.push(p.id.clone());
        }
    }
    let mut applied = Vec::new();
    let mut errors = Vec::new();
    for p in desired {
        let unchanged = current.iter().any(|c| {
            c.id == p.id
                && c.kind == p.kind
                && c.match_value == p.match_value
                && c.enabled == p.enabled
                && c.scope == p.scope
        });
        if unchanged {
            continue;
        }
        let id = p.id.clone();
        match client.call(&Request::ApplyPolicy { policy: p }).await {
            Ok(_) => applied.push(id),
            Err(e) => errors.push(json!({ "id": id, "error": format!("{e:#}") })),
        }
    }
    Ok(json!({ "applied": applied, "removed": removed, "errors": errors }))
}
