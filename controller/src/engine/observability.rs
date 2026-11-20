// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// SLO dashboards + API trace inventory (Phase 31).

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SloPolicyRow {
    pub id: Uuid,
    pub name: String,
    pub target: String,
    pub objective_pct: f64,
    pub window_hours: i32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SloStatusItem {
    pub name: String,
    pub target: String,
    pub objective_pct: f64,
    pub current_pct: f64,
    pub burn_rate: f64,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityOverview {
    pub slos: Vec<SloStatusItem>,
    pub trace_count_1h: usize,
    pub p95_latency_ms: i32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TraceSpanRow {
    pub id: Uuid,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i32,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct TraceQuery {
    pub limit: Option<i64>,
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<ObservabilityOverview> {
    let policies = crate::db::query_as(
        "SELECT id, name, target, objective_pct, window_hours, description FROM slo_policies ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let mut slos = Vec::new();
    for p in policies {
        slos.push(evaluate_slo(pool, &p).await?);
    }

    let trace_count_1h: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM api_trace_spans WHERE recorded_at > datetime('now', '-1 hours')",
    )
    .fetch_one(pool)
    .await?;

    let p95_offset = ((trace_count_1h * 5 / 100) - 1).max(0);
    let p95: Option<i32> = crate::db::query_scalar(
        "SELECT duration_ms FROM api_trace_spans
         WHERE recorded_at > datetime('now', '-1 hours')
         ORDER BY duration_ms DESC
         LIMIT 1 OFFSET ?",
    )
    .bind(p95_offset)
    .fetch_optional(pool)
    .await?;

    let worst = slos
        .iter()
        .find(|s| s.status == "breach")
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "all clear".into());

    Ok(ObservabilityOverview {
        summary: format!(
            "{} SLO(s) · {} trace(s)/1h · p95 {}ms · {}",
            slos.len(),
            trace_count_1h,
            p95.unwrap_or(0),
            worst
        ),
        slos,
        trace_count_1h: trace_count_1h as usize,
        p95_latency_ms: p95.unwrap_or(0),
    })
}

async fn evaluate_slo(pool: &DbPool, policy: &SloPolicyRow) -> anyhow::Result<SloStatusItem> {
    let (current_pct, burn_rate) = match policy.name.as_str() {
        "api-availability" => api_availability_slo(pool, policy.window_hours).await?,
        "task-success" => task_success_slo(pool, policy.window_hours).await?,
        "host-availability" => host_availability_slo(pool).await?,
        _ => (99.0, 0.1),
    };

    let status = if current_pct >= policy.objective_pct {
        "ok"
    } else if current_pct >= policy.objective_pct - 1.0 {
        "warn"
    } else {
        "breach"
    };

    Ok(SloStatusItem {
        name: policy.name.clone(),
        target: policy.target.clone(),
        objective_pct: policy.objective_pct,
        current_pct,
        burn_rate,
        status: status.into(),
        description: policy.description.clone(),
    })
}

async fn api_availability_slo(pool: &DbPool, window_hours: i32) -> anyhow::Result<(f64, f64)> {
    let total: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM api_trace_spans WHERE recorded_at > datetime('now', '-' || ? || ' hours')",
    )
    .bind(window_hours)
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok((100.0, 0.0));
    }
    let ok: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM api_trace_spans
         WHERE recorded_at > datetime('now', '-' || ? || ' hours') AND status_code < 500",
    )
    .bind(window_hours)
    .fetch_one(pool)
    .await?;
    let pct = (ok as f64 / total as f64) * 100.0;
    Ok((pct, ((100.0 - pct) / 100.0).max(0.0)))
}

async fn task_success_slo(pool: &DbPool, window_hours: i32) -> anyhow::Result<(f64, f64)> {
    let total: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE created_at > datetime('now', '-' || ? || ' hours') AND status IN ('completed', 'failed')",
    )
    .bind(window_hours)
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok((100.0, 0.0));
    }
    let ok: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE created_at > datetime('now', '-' || ? || ' hours') AND status = 'completed'",
    )
    .bind(window_hours)
    .fetch_one(pool)
    .await?;
    let pct = (ok as f64 / total as f64) * 100.0;
    Ok((pct, ((100.0 - pct) / 100.0).max(0.0)))
}

async fn host_availability_slo(pool: &DbPool) -> anyhow::Result<(f64, f64)> {
    let total: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await?;
    if total == 0 {
        return Ok((100.0, 0.0));
    }
    let online: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
        .fetch_one(pool)
        .await?;
    let pct = (online as f64 / total as f64) * 100.0;
    Ok((pct, ((100.0 - pct) / 100.0).max(0.0)))
}

pub async fn list_traces(pool: &DbPool, limit: i64) -> anyhow::Result<Vec<TraceSpanRow>> {
    crate::db::query_as(
        "SELECT id, method, path, status_code, duration_ms,
                strftime('%Y-%m-%dT%H:%M:%SZ', recorded_at) AS recorded_at
         FROM api_trace_spans ORDER BY recorded_at DESC LIMIT ?",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
    .map_err(|e| e.into())
}

struct PendingSpan {
    method: String,
    path: String,
    status_code: i32,
    duration_ms: i32,
}

static TRACE_TX: OnceLock<mpsc::Sender<PendingSpan>> = OnceLock::new();

/// Starts the background task that batches trace-span writes into one INSERT
/// per second instead of one per request. Call once at controller startup
/// (see main.rs, alongside webhook_worker::spawn/channel_worker::spawn) before
/// any request reaches `record_trace`.
///
/// This replaced a per-request `tokio::spawn` that ran its own INSERT (plus,
/// originally, a prune DELETE) against the shared SQLite pool on every single
/// `/api/v1/*` request — a direct, request-rate-scaling source of contention
/// against SQLite's single writer lock. A previous attempt to fix that same
/// contention by raising `max_connections` and parallelizing an unrelated
/// handler's sub-queries made things far worse (queries observed up to 69s)
/// because both changes *increased* how many writers could pile onto the lock
/// at once. Batching takes the opposite, safer approach: it doesn't touch pool
/// size or per-request concurrency at all, it just reduces how often this
/// path needs the writer lock in the first place, from N times/sec (N =
/// request rate) down to at most once per second.
pub fn spawn_trace_writer(pool: DbPool) {
    let (tx, mut rx) = mpsc::channel::<PendingSpan>(2000);
    if TRACE_TX.set(tx).is_err() {
        return; // already spawned
    }
    tokio::spawn(async move {
        let mut flush_count: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut batch = Vec::new();
            while let Ok(span) = rx.try_recv() {
                batch.push(span);
            }
            if batch.is_empty() {
                continue;
            }
            insert_batch(&pool, &batch).await;
            flush_count += 1;
            // Same rationale as the old counter: a rolling buffer doesn't need
            // to sit at exactly 5000 rows every second, so prune only every
            // 20th flush (~20s) rather than on every one.
            if flush_count.is_multiple_of(20) {
                let _ = crate::db::query(
                    "DELETE FROM api_trace_spans WHERE id NOT IN (
                        SELECT id FROM api_trace_spans ORDER BY recorded_at DESC LIMIT 5000
                     )",
                )
                .execute(&pool)
                .await;
            }
        }
    });
}

async fn insert_batch(pool: &DbPool, batch: &[PendingSpan]) {
    let mut sql = String::from(
        "INSERT INTO api_trace_spans (id, method, path, status_code, duration_ms, recorded_at) VALUES ",
    );
    for i in 0..batch.len() {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str("(?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))");
    }
    let mut query = crate::db::query(&sql);
    for span in batch {
        query = query
            .bind(Uuid::new_v4())
            .bind(&span.method)
            .bind(&span.path)
            .bind(span.status_code)
            .bind(span.duration_ms);
    }
    let _ = query.execute(pool).await;
}

/// Enqueues a trace span for the batched writer above. Synchronous and
/// non-blocking (a bounded-channel `try_send`): if the writer task is
/// somehow falling behind and the queue is full, the span is dropped rather
/// than blocking the request or growing memory without bound — acceptable
/// for a best-effort telemetry feed.
pub fn record_trace(method: &str, path: &str, status_code: i32, duration_ms: i32) {
    // Truncate on a UTF-8 char boundary — `path` is the untrusted request URI and a
    // fixed byte-offset slice would panic if byte 256 splits a multi-byte char.
    let path = if path.len() > 256 {
        let mut end = 256;
        while end > 0 && !path.is_char_boundary(end) {
            end -= 1;
        }
        &path[..end]
    } else {
        path
    };
    if let Some(tx) = TRACE_TX.get() {
        let _ = tx.try_send(PendingSpan {
            method: method.to_string(),
            path: path.to_string(),
            status_code,
            duration_ms,
        });
    }
}

pub async fn prometheus_slo_gauges(pool: &DbPool) -> String {
    let ov = overview(pool).await.ok();
    let Some(ov) = ov else {
        return String::new();
    };
    let mut body = String::new();
    for slo in ov.slos {
        let name = slo.name.replace('"', "'");
        body.push_str(&format!(
            "# HELP machina_slo_current_pct Current SLO attainment percent\n\
             # TYPE machina_slo_current_pct gauge\n\
             machina_slo_current_pct{{slo=\"{name}\"}} {}\n\
             # HELP machina_slo_burn_rate Error budget burn rate stub\n\
             # TYPE machina_slo_burn_rate gauge\n\
             machina_slo_burn_rate{{slo=\"{name}\"}} {}\n",
            slo.current_pct, slo.burn_rate
        ));
    }
    body.push_str(&format!(
        "# HELP machina_api_trace_p95_ms API trace p95 latency (1h)\n\
         # TYPE machina_api_trace_p95_ms gauge\n\
         machina_api_trace_p95_ms {}\n",
        ov.p95_latency_ms
    ));
    body
}
