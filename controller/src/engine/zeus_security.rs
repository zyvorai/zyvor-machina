// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Zeus Security Fabric — fleet risk aggregation over native eBPF telemetry.

use serde::Serialize;
use crate::db::DbPool;

use crate::config::ControllerConfig;
use crate::engine::ai::security_graph;
use crate::engine::bpf::{self, telemetry};
use crate::engine::zeus_firewall;

#[derive(Debug, Serialize)]
pub struct ZeusSecurityStatus {
    pub native_bpf: bpf::FleetBpfStatus,
    pub zeus_firewall: serde_json::Value,
    pub fabric_reachable: bool,
}

pub async fn status(pool: &DbPool) -> ZeusSecurityStatus {
    let native = bpf::fleet_status(pool).await;
    ZeusSecurityStatus {
        fabric_reachable: native.reachable,
        native_bpf: native,
        zeus_firewall: zeus_firewall::zeus_firewall_status().await,
    }
}

#[derive(Debug, Serialize)]
pub struct FleetThreatSummary {
    pub fleet_threat_score: f32,
    pub firewall_targets: usize,
    pub critical_events: Vec<serde_json::Value>,
    pub native_bpf: serde_json::Value,
    pub security_graph_summary: String,
}

pub async fn fleet_threat(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetThreatSummary> {
    let threat = telemetry::fleet_threat_summary(pool).await;
    let overview = zeus_firewall::overview(pool, cfg).await?;
    let graph = security_graph::build_graph(pool).await?;

    let fleet_score = threat
        .get("fleet_threat_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(100.0) as f32;

    let critical = threat
        .get("critical_events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(FleetThreatSummary {
        fleet_threat_score: fleet_score,
        firewall_targets: overview.targets.len(),
        critical_events: critical,
        native_bpf: threat,
        security_graph_summary: format!(
            "{} nodes · {} edges in infrastructure security graph",
            graph.nodes.len(),
            graph.edges.len()
        ),
    })
}

pub async fn host_summary(pool: &DbPool, host_id: &str) -> serde_json::Value {
    telemetry::host_resource(pool, host_id, "summary", 0).await
}

pub async fn host_resource(
    pool: &DbPool,
    host_id: &str,
    resource: &str,
    hours: u32,
) -> serde_json::Value {
    telemetry::host_resource(pool, host_id, resource, hours).await
}

pub async fn fleet_timeline(pool: &DbPool, hours: u32) -> serde_json::Value {
    telemetry::fleet_timeline(pool, hours).await
}

pub async fn sync_security_alerts(pool: &DbPool) -> anyhow::Result<usize> {
    let anomalies = telemetry::anomalies(pool)
        .await
        .get("anomalies")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut inserted = 0usize;
    for a in anomalies.iter().filter(|x| {
        x.get("severity")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == "critical" || s == "high")
    }) {
        let summary = a
            .get("summary")
            .or_else(|| a.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("Security alert");
        let host_id = a.get("host_id").and_then(|v| v.as_str()).unwrap_or("");
        if insert_security_alert(pool, summary, host_id, a).await? {
            inserted += 1;
        }
    }

    let health = telemetry::fabric_health(pool).await;
    if let Some(issues) = health.get("issues").and_then(|v| v.as_array()) {
        for issue in issues.iter().filter(|x| {
            x.get("severity")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "critical" || s == "high" || s == "warning")
        }) {
            let summary = issue
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Fabric health issue");
            let host_id = issue.get("host_id").and_then(|v| v.as_str()).unwrap_or("");
            if insert_security_alert(pool, summary, host_id, issue).await? {
                inserted += 1;
            }
        }
    }
    Ok(inserted)
}

async fn insert_security_alert(
    pool: &DbPool,
    summary: &str,
    host_id: &str,
    detail: &serde_json::Value,
) -> anyhow::Result<bool> {
    let id = uuid::Uuid::new_v4();
    let payload = serde_json::json!({
        "title": summary,
        "host_id": host_id,
        "severity": detail.get("severity"),
        "kind": detail.get("kind"),
        "source": telemetry::SOURCE,
    });
    let exists: bool = crate::db::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM notification_outbox
            WHERE kind = 'security.alert' AND json_extract(payload, '$.title') = ? AND created_at > datetime('now', '-1 hours')
        )",
    )
    .bind(summary)
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if exists {
        return Ok(false);
    }
    crate::db::query("INSERT INTO notification_outbox (id, kind, payload) VALUES (?, ?, ?)")
        .bind(id)
        .bind("security.alert")
        .bind(payload)
        .execute(pool)
        .await?;
    Ok(true)
}
