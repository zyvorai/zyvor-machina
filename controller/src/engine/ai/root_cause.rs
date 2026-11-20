// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub at: DateTime<Utc>,
    pub source: String,
    pub kind: String,
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Serialize)]
pub struct IncidentAnalysis {
    pub window_hours: i32,
    pub timeline: Vec<TimelineEntry>,
    pub root_cause: String,
    pub confidence: f64,
    pub contributing_factors: Vec<String>,
    pub suggested_actions: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeIncidentQuery {
    #[serde(default = "default_hours")]
    pub hours: i32,
    pub vm_id: Option<Uuid>,
    pub vm_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeIncidentBody {
    #[serde(default = "default_hours")]
    pub hours: i32,
    pub vm_id: Option<Uuid>,
    pub vm_name: Option<String>,
    pub symptoms: Vec<String>,
}

fn default_hours() -> i32 {
    4
}

pub async fn analyze(
    pool: &DbPool,
    q: &AnalyzeIncidentQuery,
) -> anyhow::Result<IncidentAnalysis> {
    analyze_with_symptoms(pool, q, &[]).await
}

pub async fn analyze_post(
    pool: &DbPool,
    body: &AnalyzeIncidentBody,
) -> anyhow::Result<IncidentAnalysis> {
    let q = AnalyzeIncidentQuery {
        hours: body.hours,
        vm_id: body.vm_id,
        vm_name: body.vm_name.clone(),
    };
    analyze_with_symptoms(pool, &q, &body.symptoms).await
}

async fn analyze_with_symptoms(
    pool: &DbPool,
    q: &AnalyzeIncidentQuery,
    symptoms: &[String],
) -> anyhow::Result<IncidentAnalysis> {
    let hours = q.hours.clamp(1, 72);
    let vm_id = resolve_vm(pool, q.vm_id, q.vm_name.as_deref()).await?;

    let mut timeline = Vec::new();

    let audits: Vec<(DateTime<Utc>, String, String, Option<String>)> = crate::db::query_as(
        "SELECT created_at, actor, action, resource_type
         FROM audit_logs
         WHERE created_at > datetime('now', '-' || ? || ' hours')
         ORDER BY created_at DESC LIMIT 80",
    )
    .bind(hours)
    .fetch_all(pool)
    .await?;
    for (at, actor, action, rt) in audits {
        timeline.push(TimelineEntry {
            at,
            source: "audit".into(),
            kind: action.clone(),
            message: format!(
                "{actor} — {action}{}",
                rt.map(|r| format!(" ({r})")).unwrap_or_default()
            ),
            severity: audit_severity(&action),
        });
    }

    let events: Vec<(DateTime<Utc>, String, String)> = crate::db::query_as(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', created_at), kind, message FROM events
         WHERE created_at > datetime('now', '-' || ? || ' hours')
         ORDER BY created_at DESC LIMIT 60",
    )
    .bind(hours)
    .fetch_all(pool)
    .await?;
    for (at, kind, message) in events {
        timeline.push(TimelineEntry {
            at,
            source: "event".into(),
            kind: kind.clone(),
            message: message.clone(),
            severity: if kind.contains("error") || kind.contains("fail") {
                "high".into()
            } else {
                "medium".into()
            },
        });
    }

    let tasks: Vec<(DateTime<Utc>, String, String, Option<Uuid>)> = if let Some(vid) = vm_id {
        crate::db::query_as(
            "SELECT created_at, operation, status, resource_id FROM tasks
             WHERE created_at > datetime('now', '-' || ? || ' hours')
               AND (resource_id = ? OR status = 'failed')
             ORDER BY created_at DESC LIMIT 40",
        )
        .bind(hours)
        .bind(vid)
        .fetch_all(pool)
        .await?
    } else {
        crate::db::query_as(
            "SELECT created_at, operation, status, resource_id FROM tasks
             WHERE created_at > datetime('now', '-' || ? || ' hours')
             ORDER BY created_at DESC LIMIT 40",
        )
        .bind(hours)
        .fetch_all(pool)
        .await?
    };
    for (at, op, status, _) in tasks {
        timeline.push(TimelineEntry {
            at,
            source: "task".into(),
            kind: op.clone(),
            message: format!("{op} — {status}"),
            severity: if status == "failed" {
                "high".into()
            } else {
                "low".into()
            },
        });
    }

    timeline.sort_by_key(|x| std::cmp::Reverse(x.at));
    timeline.truncate(100);

    // Host / NIC evidence from events
    let mut evidence = Vec::new();
    for e in &timeline {
        let msg = e.message.to_lowercase();
        if msg.contains("carrier")
            || msg.contains("nic")
            || msg.contains("bridge")
            || msg.contains("eno")
        {
            evidence.push(format!("{}: {}", e.source, e.message));
        }
    }

    // Fence events
    let fences: Vec<(DateTime<Utc>, String)> = crate::db::query_as(
        "SELECT created_at, COALESCE(message, action) FROM fence_events
         WHERE created_at > datetime('now', '-' || ? || ' hours')
         ORDER BY created_at DESC LIMIT 10",
    )
    .bind(hours)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (at, message) in fences {
        evidence.push(format!("fence @ {at}: {message}"));
        timeline.push(TimelineEntry {
            at,
            source: "fence".into(),
            kind: "fence".into(),
            message,
            severity: "critical".into(),
        });
    }

    // VM metrics spike
    if let Some(vid) = vm_id {
        if let Ok(Some(c)) =
            crate::db::query_scalar::<_, f64>("SELECT cpu_percent FROM vm_metrics WHERE vm_id = ?")
                .bind(vid)
                .fetch_optional(pool)
                .await
        {
            if c >= 95.0 {
                evidence.push(format!("VM CPU at {c:.0}% during incident window"));
            }
        }
    }

    for s in symptoms {
        evidence.push(format!("Reported symptom: {s}"));
    }

    timeline.sort_by_key(|x| std::cmp::Reverse(x.at));
    timeline.truncate(100);

    let (root_cause, confidence, factors, actions) =
        infer_root_cause(&timeline, vm_id.is_some(), &evidence);
    Ok(IncidentAnalysis {
        window_hours: hours,
        timeline,
        root_cause,
        confidence,
        contributing_factors: factors,
        suggested_actions: actions,
        evidence,
    })
}

/// Merge native eBPF anomalies into the incident timeline.
pub fn merge_bpf_anomalies(timeline: &mut Vec<TimelineEntry>, anomalies: &serde_json::Value) {
    let Some(items) = anomalies.get("anomalies").and_then(|v| v.as_array()) else {
        return;
    };
    for a in items {
        let summary = a
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("eBPF security event");
        let severity = a
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("security");
        let at = a
            .get("ts")
            .and_then(|v| v.as_str())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);
        timeline.push(TimelineEntry {
            at,
            source: "machina-bpf".into(),
            kind: kind.into(),
            message: summary.into(),
            severity: severity.into(),
        });
    }
    timeline.sort_by_key(|x| std::cmp::Reverse(x.at));
    timeline.truncate(120);
}

async fn resolve_vm(
    pool: &DbPool,
    vm_id: Option<Uuid>,
    vm_name: Option<&str>,
) -> anyhow::Result<Option<Uuid>> {
    if let Some(id) = vm_id {
        return Ok(Some(id));
    }
    if let Some(name) = vm_name.filter(|n| !n.is_empty()) {
        let id: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
        return Ok(id);
    }
    Ok(None)
}

fn audit_severity(action: &str) -> String {
    let a = action.to_lowercase();
    if a.contains("delete") || a.contains("fence") {
        "critical".into()
    } else if a.contains("migrate") || a.contains("network") || a.contains("firewall") {
        "high".into()
    } else {
        "medium".into()
    }
}

fn infer_root_cause(
    timeline: &[TimelineEntry],
    vm_scoped: bool,
    evidence: &[String],
) -> (String, f64, Vec<String>, Vec<String>) {
    let mut factors = Vec::new();
    let mut actions = Vec::new();

    let storage_hit = timeline.iter().any(|e| {
        e.message.to_lowercase().contains("storage")
            || e.message.to_lowercase().contains("disk")
            || e.message.to_lowercase().contains("pool full")
    });
    let network_hit = timeline.iter().any(|e| {
        e.kind.to_lowercase().contains("network")
            || e.message.to_lowercase().contains("firewall")
            || e.message.to_lowercase().contains("connect")
    });
    let vm_restart = timeline.iter().any(|e| {
        e.message.to_lowercase().contains("restart")
            || e.message.to_lowercase().contains("vm.start")
            || e.message.to_lowercase().contains("vm.stop")
    });
    let failed_task = timeline
        .iter()
        .any(|e| e.source == "task" && e.severity == "high");

    let nic_hit = evidence.iter().any(|e| {
        let el = e.to_lowercase();
        el.contains("carrier") || el.contains("nic") || el.contains("bridge") || el.contains("eno")
    });

    if nic_hit {
        factors.push("Host NIC or bridge state change detected.".into());
        actions.push("Check physical link and `ip link` on hypervisor.".into());
        actions.push("Verify bridge br0 carrier after NIC reset.".into());
        return (
            "VM became unreachable because bridge lost carrier after NIC reset.".into(),
            0.85,
            factors,
            actions,
        );
    }

    if storage_hit && vm_restart {
        factors.push("Storage pressure correlated with VM lifecycle change.".into());
        actions.push("Expand storage pool or prune snapshots.".into());
        actions.push("Verify VM disk quotas and backup retention.".into());
        return (
            "Storage pool pressure likely caused VM restart and downstream outage.".into(),
            0.82,
            factors,
            actions,
        );
    }

    if network_hit {
        factors.push("Network or firewall change detected in timeline.".into());
        actions.push("Review recent network policy and route changes.".into());
        actions.push("Run Network Lens reachability check.".into());
        return (
            "Network configuration change likely caused connectivity loss.".into(),
            0.76,
            factors,
            actions,
        );
    }

    if failed_task {
        factors.push("Failed orchestration task in window.".into());
        actions.push("Open Tasks drawer and retry or rollback.".into());
        actions.push("Generate runbook from failed operation.".into());
        return (
            "Failed platform task is the most likely trigger — check task detail and agent logs."
                .into(),
            0.71,
            factors,
            actions,
        );
    }

    if vm_scoped {
        actions.push("Open Zyra SRE for VM health score.".into());
        return (
            "No dominant correlation — review timeline and VM metrics for gradual degradation."
                .into(),
            0.45,
            factors,
            actions,
        );
    }

    actions.push("Narrow analysis with vm_name or vm_id query parameter.".into());
    (
        "Insufficient signal in cluster window — expand hours or scope to a VM.".into(),
        0.35,
        factors,
        actions,
    )
}
