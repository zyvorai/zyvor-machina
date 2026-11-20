// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use super::capacity;
use super::cost;
use super::sre_predict;
use super::sre_remediate;

#[derive(Debug, Serialize)]
pub struct Prediction {
    pub resource: String,
    pub resource_kind: String,
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub hours_until_critical: Option<f64>,
    pub confidence: f64,
    pub evidence: String,
}

#[derive(Debug, Serialize)]
pub struct PredictionsReport {
    pub predictions: Vec<Prediction>,
    pub summary: String,
}

pub async fn unified(pool: &DbPool) -> anyhow::Result<PredictionsReport> {
    let mut predictions = Vec::new();

    let sre = sre_predict::forecast(pool).await?;
    for f in sre.forecasts {
        predictions.push(Prediction {
            resource: f.vm_name.clone(),
            resource_kind: if f.vm_id == "cluster" {
                "cluster".into()
            } else {
                "vm".into()
            },
            kind: f.resource.clone(),
            severity: f.severity.clone(),
            message: f.message.clone(),
            hours_until_critical: f.hours_until_critical,
            confidence: f.confidence,
            evidence: "vm_metrics trend / storage pool ratio".into(),
        });
    }

    let cap = capacity::plan(pool).await?;
    if cap.storage_runway_days.unwrap_or(999) < 30 {
        predictions.push(Prediction {
            resource: "storage".into(),
            resource_kind: "storage".into(),
            kind: "exhaustion".into(),
            severity: "high".into(),
            message: format!(
                "Storage runway ~{} days at current growth",
                cap.storage_runway_days.unwrap_or(0)
            ),
            hours_until_critical: cap.storage_runway_days.map(|d| d as f64 * 24.0),
            confidence: 0.72,
            evidence: "capacity planner storage runway".into(),
        });
    }

    // Host saturation
    let hot_hosts: Vec<(String, f64)> = crate::db::query_as(
        "SELECT hostname, cpu_percent FROM hosts WHERE state = 'online' AND cpu_percent >= 80 ORDER BY cpu_percent DESC LIMIT 5",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (name, cpu) in hot_hosts {
        predictions.push(Prediction {
            resource: name.clone(),
            resource_kind: "host".into(),
            kind: "saturation".into(),
            severity: if cpu >= 90.0 { "critical" } else { "high" }.into(),
            message: format!("Host {name} CPU at {cpu:.0}%"),
            hours_until_critical: Some(24.0),
            confidence: 0.65,
            evidence: "host cpu_percent".into(),
        });
    }

    // SMART / linux health stub from fleet
    let smart_warn: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM hosts WHERE state = 'online' AND tags LIKE '%smart_warn%'",
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .unwrap_or(0);
    if smart_warn > 0 {
        predictions.push(Prediction {
            resource: format!("{smart_warn} hosts"),
            resource_kind: "host".into(),
            kind: "disk_failure".into(),
            severity: "medium".into(),
            message: "SMART warnings detected on fleet hosts".into(),
            hours_until_critical: Some(168.0),
            confidence: 0.55,
            evidence: "host tags smart_warn".into(),
        });
    }

    let summary = if predictions.is_empty() {
        "No critical predictions in the next 72 hours.".into()
    } else {
        format!(
            "{} prediction(s) — top: {}",
            predictions.len(),
            predictions
                .first()
                .map(|p| p.message.as_str())
                .unwrap_or("")
        )
    };

    promote_critical(pool, &predictions).await.ok();

    Ok(PredictionsReport {
        predictions,
        summary,
    })
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Open ai_incidents for high/critical predictions within 72h horizon (deduped by resource).
pub async fn promote_critical(pool: &DbPool, predictions: &[Prediction]) -> anyhow::Result<()> {
    use super::incident_commander::{self, CreateIncidentRequest};

    for p in predictions {
        let urgent = (p.severity == "critical" || p.severity == "high")
            && p.hours_until_critical.map(|h| h <= 72.0).unwrap_or(true);
        if !urgent {
            continue;
        }
        // Resource names routinely contain `_` (e.g. "db_primary") which SQLite
        // LIKE treats as a single-char wildcard when unescaped — that made this
        // dedup check match unrelated resources (e.g. "db-primary" vs
        // "dbXprimary") and silently skip opening a real incident.
        let pattern = format!("%{}%", escape_like(&p.resource));
        let exists: bool = crate::db::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM ai_incidents
                WHERE status IN ('open', 'investigating')
                  AND (title LIKE ? ESCAPE '\\' OR summary LIKE ? ESCAPE '\\' OR affected_resources LIKE ? ESCAPE '\\')
            )",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
        if exists {
            continue;
        }
        let id = incident_commander::create(
            pool,
            &CreateIncidentRequest {
                title: format!("Predicted {} failure", p.kind),
                summary: p.message.clone(),
                severity: p.severity.clone(),
                affected_resources: vec![p.resource.clone()],
                root_cause: Some(format!(
                    "{} (confidence {:.0}%, horizon {:.0}h)",
                    p.evidence,
                    p.confidence * 100.0,
                    p.hours_until_critical.unwrap_or(72.0)
                )),
                window_start: None,
                window_end: None,
            },
        )
        .await?;
        let _ = crate::db::query(
            "INSERT INTO events (id, kind, message, resource_type, resource_id, payload) VALUES (?, 'prediction', ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&p.message)
        .bind(&p.resource_kind)
        .bind(p.resource.clone())
        .bind(serde_json::json!({"severity": &p.severity}))
        .execute(pool)
        .await;
        let _ = id;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RightsizingRecommendation {
    pub vm_id: String,
    pub vm_name: String,
    pub current_memory_mib: i64,
    pub suggested_memory_mib: i64,
    pub savings_usd: f64,
    pub risk: String,
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct RightsizingReport {
    pub recommendations: Vec<RightsizingRecommendation>,
    pub idle_vm_count: i64,
    pub oversized_vm_count: i64,
    pub estimated_monthly_savings_usd: f64,
}

pub async fn rightsizing_report(pool: &DbPool) -> anyhow::Result<RightsizingReport> {
    let cost_analysis = cost::analyze(pool).await?;
    let sre = match sre_remediate::propose(pool).await {
        Ok(r) => r,
        Err(_) => sre_remediate::SreRemediationReport {
            remediations: vec![],
            summary: String::new(),
        },
    };

    let mut recommendations = Vec::new();

    let oversized: Vec<(Uuid, String, i64, Option<i64>)> = crate::db::query_as(
        "SELECT v.id, v.name, v.memory_mib, m.memory_used_mib FROM vms v
         JOIN vm_metrics m ON m.vm_id = v.id
         WHERE v.observed_state = 'running'
           AND CAST(m.memory_used_mib AS REAL) / NULLIF(v.memory_mib, 0) < 0.35
         ORDER BY v.memory_mib DESC LIMIT 30",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for (id, name, mem, used) in oversized {
        let used = used.unwrap_or(mem / 4);
        let suggested = ((used as f64 * 1.4) as i64).max(512);
        let savings = ((mem - suggested).max(0) as f64) * 0.012;
        recommendations.push(RightsizingRecommendation {
            vm_id: id.to_string(),
            vm_name: name,
            current_memory_mib: mem,
            suggested_memory_mib: suggested,
            savings_usd: savings,
            risk: "low".into(),
            action: "rightsize".into(),
            detail: format!("Memory utilization below 35% — suggest {suggested} MiB"),
        });
    }

    let idle: Vec<(Uuid, String)> = crate::db::query_as(
        "SELECT id, name FROM vms WHERE observed_state = 'stopped'
         AND updated_at < datetime('now', '-30 days') LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (id, name) in idle {
        recommendations.push(RightsizingRecommendation {
            vm_id: id.to_string(),
            vm_name: name.clone(),
            current_memory_mib: 0,
            suggested_memory_mib: 0,
            savings_usd: 15.0,
            risk: "medium".into(),
            action: "power_off".into(),
            detail: format!("VM {name} stopped 30+ days — consider archive or delete"),
        });
    }

    for rem in sre.remediations.iter().take(5) {
        if rem.action.contains("rightsize") || rem.action.contains("migrate") {
            recommendations.push(RightsizingRecommendation {
                vm_id: rem.vm_id.clone().unwrap_or_default(),
                vm_name: rem.vm_name.clone(),
                current_memory_mib: 0,
                suggested_memory_mib: 0,
                savings_usd: 0.0,
                risk: rem.risk.clone(),
                action: rem.action.clone(),
                detail: rem.review.clone(),
            });
        }
    }

    let savings: f64 = recommendations.iter().map(|r| r.savings_usd).sum();

    Ok(RightsizingReport {
        recommendations,
        idle_vm_count: cost_analysis.idle_vm_count as i64,
        oversized_vm_count: cost_analysis.oversized_vm_count as i64,
        estimated_monthly_savings_usd: savings,
    })
}
