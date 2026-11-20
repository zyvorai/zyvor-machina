// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

pub async fn ingest_recent(
    pool: &DbPool,
    _cfg: &ControllerConfig,
) -> anyhow::Result<IngestStats> {
    let mut stats = IngestStats::default();
    stats.firewall += ingest_firewall_timeline(pool).await?;
    stats.audit += ingest_audit_logs(pool).await?;
    stats.platform += ingest_platform_events(pool).await?;
    stats.native_bpf += ingest_bpf_anomalies(pool).await?;
    Ok(stats)
}

#[derive(Debug, Default, serde::Serialize)]
pub struct IngestStats {
    pub firewall: usize,
    pub audit: usize,
    pub platform: usize,
    pub native_bpf: usize,
}

async fn watermark(pool: &DbPool, source: &str) -> anyhow::Result<DateTime<Utc>> {
    let ts: Option<DateTime<Utc>> =
        crate::db::query_scalar("SELECT last_at FROM soc_ingest_watermarks WHERE source = ?")
            .bind(source)
            .fetch_optional(pool)
            .await?;
    Ok(ts.unwrap_or_else(|| Utc::now() - chrono::Duration::days(7)))
}

async fn advance_watermark(
    pool: &DbPool,
    source: &str,
    ts: DateTime<Utc>,
) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE soc_ingest_watermarks SET last_at = CASE WHEN last_at > ? THEN last_at ELSE ? END WHERE source = ?",
    )
    .bind(ts)
    .bind(ts)
    .bind(source)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_event(
    pool: &DbPool,
    occurred_at: DateTime<Utc>,
    source: &str,
    category: &str,
    severity: &str,
    host_id: Option<Uuid>,
    vm_id: Option<Uuid>,
    actor: Option<&str>,
    summary: &str,
    ecs: Value,
    raw_ref: Value,
    dedupe_key: Option<&str>,
) -> anyhow::Result<bool> {
    let r = crate::db::query(
        "INSERT INTO soc_events (id, occurred_at, source, category, severity, host_id, vm_id, actor, summary, ecs_json, raw_ref, dedupe_key)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(occurred_at)
    .bind(source)
    .bind(category)
    .bind(severity)
    .bind(host_id)
    .bind(vm_id)
    .bind(actor)
    .bind(summary)
    .bind(ecs)
    .bind(raw_ref)
    .bind(dedupe_key)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

async fn ingest_firewall_timeline(pool: &DbPool) -> anyhow::Result<usize> {
    let since = watermark(pool, "firewall_timeline").await?;
    let rows: Vec<(
        String,
        Uuid,
        String,
        String,
        Option<String>,
        DateTime<Utc>,
        Value,
    )> = crate::db::query_as(
        "SELECT target_kind, target_id, kind, summary, actor,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at, detail_json
         FROM firewall_timeline WHERE strftime('%Y-%m-%dT%H:%M:%SZ', created_at) > ? ORDER BY created_at ASC LIMIT 2000",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut n = 0usize;
    let mut max_ts = since;
    for (target_kind, target_id, kind, summary, actor, created_at, detail) in rows {
        max_ts = max_ts.max(created_at);
        let severity = firewall_severity(&kind, &detail);
        let host_id = if target_kind == "host" {
            Some(target_id)
        } else {
            None
        };
        let dedupe = format!("fw:{}:{}:{}", target_id, kind, created_at.timestamp());
        let ecs = json!({
            "@timestamp": created_at.to_rfc3339(),
            "event.dataset": "machina.firewall",
            "event.category": ["network"],
            "event.action": kind,
            "event.severity": severity_to_ecs(&severity),
            "message": summary,
            "machina.target.kind": target_kind,
            "machina.target.id": target_id.to_string(),
        });
        if insert_event(
            pool,
            created_at,
            "firewall",
            "firewall",
            &severity,
            host_id,
            None,
            actor.as_deref(),
            &summary,
            ecs,
            json!({ "target_kind": target_kind, "target_id": target_id, "detail": detail }),
            Some(&dedupe),
        )
        .await?
        {
            n += 1;
        }
    }
    if max_ts > since {
        advance_watermark(pool, "firewall_timeline", max_ts).await?;
    }
    Ok(n)
}

fn firewall_severity(kind: &str, detail: &Value) -> String {
    let k = kind.to_lowercase();
    if k.contains("deny") || k.contains("block") {
        return "high".into();
    }
    if detail
        .get("severity")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == "critical" || s == "high")
    {
        return "high".into();
    }
    if k.contains("warn") {
        return "medium".into();
    }
    "low".into()
}

async fn ingest_audit_logs(pool: &DbPool) -> anyhow::Result<usize> {
    let since = watermark(pool, "audit_logs").await?;
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        Option<Uuid>,
        Value,
        DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT id, actor, action, resource_type,
                CASE WHEN typeof(resource_id) = 'blob' AND length(resource_id) = 16 THEN resource_id END AS resource_id,
                detail,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM audit_logs WHERE strftime('%Y-%m-%dT%H:%M:%SZ', created_at) > ? ORDER BY created_at ASC LIMIT 2000",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut n = 0usize;
    let mut max_ts = since;
    for (id, actor, action, resource_type, resource_id, detail, created_at) in rows {
        max_ts = max_ts.max(created_at);
        let severity = audit_severity(&action);
        let dedupe = format!("audit:{}", id);
        let ecs = json!({
            "@timestamp": created_at.to_rfc3339(),
            "event.dataset": "machina.audit",
            "event.category": ["authentication", "iam"],
            "event.action": action,
            "event.severity": severity_to_ecs(&severity),
            "user.name": actor,
            "message": format!("{actor} {action}"),
        });
        if insert_event(
            pool,
            created_at,
            "audit",
            "iam",
            &severity,
            None,
            resource_type.as_deref().and(resource_id),
            Some(&actor),
            &format!("{actor} {action}"),
            ecs,
            json!({ "audit_id": id, "resource_type": resource_type, "detail": detail }),
            Some(&dedupe),
        )
        .await?
        {
            n += 1;
        }
    }
    if max_ts > since {
        advance_watermark(pool, "audit_logs", max_ts).await?;
    }
    Ok(n)
}

fn audit_severity(action: &str) -> String {
    let a = action.to_lowercase();
    if a.contains("fail") || a.contains("denied") {
        "high".into()
    } else if a.contains("delete") || a.contains("fence") {
        "medium".into()
    } else {
        "low".into()
    }
}

async fn ingest_platform_events(pool: &DbPool) -> anyhow::Result<usize> {
    let since = watermark(pool, "platform_events").await?;
    let rows: Vec<(
        Uuid,
        String,
        Option<String>,
        Option<Uuid>,
        String,
        Value,
        DateTime<Utc>,
    )> = crate::db::query_as(
        "SELECT id, kind, resource_type,
                CASE WHEN typeof(resource_id) = 'blob' AND length(resource_id) = 16 THEN resource_id END AS resource_id,
                message, payload,
                strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at
             FROM events WHERE strftime('%Y-%m-%dT%H:%M:%SZ', created_at) > ? ORDER BY created_at ASC LIMIT 1000",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;

    let mut n = 0usize;
    let mut max_ts = since;
    for (id, kind, resource_type, resource_id, message, payload, created_at) in rows {
        max_ts = max_ts.max(created_at);
        let severity = if kind.contains("error") || kind.contains("fail") {
            "high"
        } else {
            "low"
        };
        let dedupe = format!("evt:{}", id);
        let ecs = json!({
            "@timestamp": created_at.to_rfc3339(),
            "event.dataset": "machina.platform",
            "event.action": kind,
            "message": message,
        });
        if insert_event(
            pool,
            created_at,
            "platform",
            "operations",
            severity,
            None,
            resource_type.as_deref().and(resource_id),
            None,
            &message,
            ecs,
            json!({ "event_id": id, "payload": payload }),
            Some(&dedupe),
        )
        .await?
        {
            n += 1;
        }
    }
    if max_ts > since {
        advance_watermark(pool, "platform_events", max_ts).await?;
    }
    Ok(n)
}

async fn ingest_bpf_anomalies(pool: &DbPool) -> anyhow::Result<usize> {
    let since = watermark(pool, "machina-bpf").await?;
    let native = crate::engine::bpf::telemetry::anomalies(pool).await;
    let anomalies = native
        .get("anomalies")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut n = 0usize;
    let mut max_ts = since;
    for a in anomalies {
        let ts = a
            .get("ts")
            .and_then(|v| v.as_str())
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        if ts <= since {
            continue;
        }
        max_ts = max_ts.max(ts);
        let summary = a
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("eBPF anomaly");
        let severity = a
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        let host_id = a
            .get("host_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("unknown");
        let anomaly_id = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let dedupe = format!(
            "bpf:{}:{}",
            a.get("host_id").and_then(|v| v.as_str()).unwrap_or(""),
            if anomaly_id.is_empty() {
                kind
            } else {
                anomaly_id
            }
        );
        let ecs = json!({
            "@timestamp": ts.to_rfc3339(),
            "event.dataset": "machina.bpf",
            "event.category": ["intrusion_detection"],
            "event.kind": "alert",
            "event.severity": severity_to_ecs(severity),
            "message": summary,
            "machina.bpf.anomaly_type": kind,
            "machina.bpf.anomaly_id": anomaly_id,
            "machina.vm.name": a.get("vm"),
            "observer.ingress.interface.name": a.get("iface"),
            "source.ip": a.get("local"),
            "destination.ip": a.get("remote"),
        });
        if insert_event(
            pool,
            ts,
            "machina-bpf",
            "intrusion_detection",
            severity,
            host_id,
            None,
            None,
            summary,
            ecs,
            a.clone(),
            Some(&dedupe),
        )
        .await?
        {
            n += 1;
        }
    }
    if max_ts > since {
        advance_watermark(pool, "machina-bpf", max_ts).await?;
    }
    Ok(n)
}

fn severity_to_ecs(severity: &str) -> i32 {
    match severity.to_lowercase().as_str() {
        "critical" => 90,
        "high" => 70,
        "medium" => 50,
        "low" => 30,
        _ => 10,
    }
}
