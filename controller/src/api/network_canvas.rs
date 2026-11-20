// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::api::topology::{build_topology, TopologyGraph};
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::bpf::{self, telemetry};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct NetworkCanvasResponse {
    pub topology: TopologyGraph,
    pub flows: serde_json::Value,
    pub flow_stats: serde_json::Value,
    pub anomalies: serde_json::Value,
    pub native_bpf: bpf::FleetBpfStatus,
    pub network_pulse: serde_json::Value,
}

fn copy_text_field(obj: &mut serde_json::Map<String, Value>, from: &str, to: &str) {
    if obj.contains_key(to) {
        return;
    }
    if let Some(v) = obj
        .get(from)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        obj.insert(to.to_string(), Value::String(v.to_string()));
    }
}

fn normalize_anomaly_item(item: &mut Value) {
    let Some(obj) = item.as_object_mut() else {
        return;
    };
    copy_text_field(obj, "description", "summary");
    copy_text_field(obj, "summary", "description");
}

fn normalize_anomalies(mut value: Value) -> Value {
    if let Some(items) = value.get_mut("anomalies").and_then(|v| v.as_array_mut()) {
        for item in items.iter_mut() {
            normalize_anomaly_item(item);
        }
    }
    value
}

fn normalize_threat_item(item: &mut Value) {
    let Some(obj) = item.as_object_mut() else {
        return;
    };
    copy_text_field(obj, "description", "title");
    copy_text_field(obj, "description", "summary");
    if !obj.contains_key("title") {
        if let Some(kind) = obj.get("kind").and_then(|v| v.as_str()) {
            obj.insert("title".to_string(), Value::String(kind.to_string()));
        }
    }
}

fn normalize_timeline_event(item: &mut Value) {
    let Some(obj) = item.as_object_mut() else {
        return;
    };
    copy_text_field(obj, "message", "summary");
    copy_text_field(obj, "type", "kind");
}

fn service_map_node_name(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn resolve_service_map_node_key(
    nodes: &[Value],
    workload: &str,
    namespace: Option<&str>,
) -> String {
    if let Some(ns) = namespace.filter(|s| !s.is_empty()) {
        let qualified = format!("{ns}/{workload}");
        if nodes.iter().any(|n| {
            n.get("namespace")
                .and_then(|v| v.as_str())
                .is_some_and(|nns| nns == ns)
                && service_map_node_name(n).as_deref() == Some(workload)
        }) {
            return qualified;
        }
    }
    for node in nodes {
        if service_map_node_name(node).as_deref() == Some(workload) {
            if let Some(ns) = node.get("namespace").and_then(|v| v.as_str()) {
                return format!("{ns}/{workload}");
            }
        }
    }
    workload.to_string()
}

fn normalize_service_map_edges(service_map: &mut Value) {
    let Some(obj) = service_map.as_object_mut() else {
        return;
    };
    let nodes = obj
        .get("nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let Some(edges) = obj.get_mut("edges").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for edge in edges.iter_mut() {
        let Some(edge_obj) = edge.as_object_mut() else {
            continue;
        };
        if let Some(source) = edge_obj.get("source").and_then(|v| v.as_str()) {
            let ns = edge_obj.get("source_namespace").and_then(|v| v.as_str());
            let key = resolve_service_map_node_key(&nodes, source, ns);
            edge_obj.insert("source_key".to_string(), Value::String(key));
        }
        if let Some(target) = edge_obj.get("target").and_then(|v| v.as_str()) {
            let ns = edge_obj.get("target_namespace").and_then(|v| v.as_str());
            let key = resolve_service_map_node_key(&nodes, target, ns);
            edge_obj.insert("target_key".to_string(), Value::String(key));
        }
    }
}

fn normalize_network_pulse(mut pulse: Value) -> Value {
    if let Some(threats) = pulse
        .pointer_mut("/threats/threats")
        .and_then(|v| v.as_array_mut())
    {
        for item in threats.iter_mut() {
            normalize_threat_item(item);
        }
    }
    if let Some(events) = pulse
        .pointer_mut("/timeline/events")
        .and_then(|v| v.as_array_mut())
    {
        for item in events.iter_mut() {
            normalize_timeline_event(item);
        }
    }
    if let Some(items) = pulse
        .pointer_mut("/anomalies/anomalies")
        .and_then(|v| v.as_array_mut())
    {
        for item in items.iter_mut() {
            normalize_anomaly_item(item);
        }
    }
    if let Some(service_map) = pulse.get_mut("service_map") {
        normalize_service_map_edges(service_map);
    }
    pulse
}

pub async fn network_canvas(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<NetworkCanvasResponse>, ApiError> {
    require_operator(&actor)?;
    let topology = build_topology(&state.pool, None).await?;
    let (flows, flow_stats, anomalies, network_pulse, native_bpf) = tokio::join!(
        telemetry::flows(&state.pool, 40),
        telemetry::flow_stats(&state.pool),
        telemetry::anomalies(&state.pool),
        telemetry::network_pulse(&state.pool),
        bpf::fleet_status(&state.pool),
    );

    Ok(Json(NetworkCanvasResponse {
        topology,
        flows,
        flow_stats,
        anomalies: normalize_anomalies(anomalies),
        native_bpf,
        network_pulse: normalize_network_pulse(network_pulse),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_production_anomaly_description() {
        let mut raw = json!({"anomalies":[{"description":"pod scan","severity":"high"}]});
        raw = normalize_anomalies(raw);
        assert_eq!(raw["anomalies"][0]["summary"].as_str(), Some("pod scan"));
    }

    #[test]
    fn normalizes_service_map_edge_keys() {
        let mut pulse = json!({
            "service_map": {
                "nodes": [
                    {"namespace":"host","name":"node1"},
                    {"namespace":"hermes-system","name":"hermes-64c699dd97"}
                ],
                "edges": [
                    {
                        "source":"node1",
                        "source_namespace":"host",
                        "target":"hermes-64c699dd97",
                        "target_namespace":"hermes-system"
                    }
                ]
            }
        });
        pulse = normalize_network_pulse(pulse);
        assert_eq!(
            pulse["service_map"]["edges"][0]["source_key"].as_str(),
            Some("host/node1")
        );
        assert_eq!(
            pulse["service_map"]["edges"][0]["target_key"].as_str(),
            Some("hermes-system/hermes-64c699dd97")
        );
    }
}
