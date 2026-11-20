// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Metrics API and ingest. Remote-write / Prometheus-text ingest only map
//! `machina_host_{cpu,memory,disk}_percent` into the in-memory history ring — not a full TSDB.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_core::libvirt::extras::get_host_stats;
use machina_core::libvirt::metrics;
use machina_core::metrics_history::MetricsHistoryPoint;
use machina_core::{
    decode_remote_write_body, host_percents_from_remote_write, host_percents_from_samples,
    parse_prometheus_text, LibvirtError, LibvirtManager, VmMetrics,
};

const REMOTE_WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;

use crate::auth::{require_write, RequestActor};
use crate::conn_query::{apply_impersonation_session_default, spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;
use crate::http_metrics::HttpMetrics;
use crate::metrics_history::MetricsHistoryStore;

async fn get_all_metrics(
    State(manager): State<LibvirtManager>,
) -> Result<Json<Vec<VmMetrics>>, AppError> {
    let result = tokio::task::spawn_blocking(move || manager.merge_all_metrics())
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;
    Ok(Json(result?))
}

async fn get_vm_metrics(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(q): Query<ConnQuery>,
) -> Result<Json<VmMetrics>, AppError> {
    if q.is_fluxvm() {
        return Ok(Json(super::fluxvm::client()?.metrics(&name).await?));
    }
    let dual = manager.dual_enabled();
    let conn_q = apply_impersonation_session_default(&actor, q);
    let target = manager.resolve_query(conn_q.connection.as_deref());
    let label = crate::conn_query::connection_label(dual, target);
    let name2 = name.clone();
    let mut m = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        metrics::get_vm_metrics(conn, &name2)
    })
    .await?;
    m.libvirt_connection = label;
    Ok(Json(m))
}

#[derive(serde::Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

async fn get_metrics_history(
    Extension(store): Extension<MetricsHistoryStore>,
    Query(q): Query<HistoryQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(60).min(500);
    let points = store.snapshot(limit);
    Json(serde_json::json!({
        "points": points,
        "persist_path": machina_core::metrics_history::metrics_history_jsonl_path().to_string_lossy(),
    }))
}

#[derive(serde::Deserialize)]
struct TracesQuery {
    limit: Option<usize>,
}

async fn get_metrics_traces(
    Extension(http_metrics): Extension<std::sync::Arc<HttpMetrics>>,
    Query(q): Query<TracesQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(64).min(256);
    let traces = http_metrics.recent_traces(limit);
    Json(serde_json::json!({ "traces": traces, "count": traces.len() }))
}

#[derive(serde::Deserialize)]
struct BatchIngestRequest {
    points: Vec<MetricsHistoryPoint>,
}

async fn post_metrics_ingest_batch(
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<MetricsHistoryStore>,
    Json(req): Json<BatchIngestRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "metrics:write")?;
    if req.points.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "points array must not be empty".into(),
        )));
    }
    if req.points.len() > 500 {
        return Err(AppError::from(LibvirtError::Invalid(
            "at most 500 points per batch".into(),
        )));
    }
    for p in &req.points {
        store.push(p.clone());
    }
    Ok(Json(serde_json::json!({
        "status": "ok",
        "ingested": req.points.len(),
    })))
}

async fn post_metrics_ingest_remote_write(
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<MetricsHistoryStore>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "metrics:write")?;
    if body.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "empty remote_write body".into(),
        )));
    }
    let content_type = headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.is_empty() && !content_type.contains("application/x-protobuf") {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "Content-Type must be application/x-protobuf (got {content_type})"
        ))));
    }
    let decoded = decode_remote_write_body(&body, Some(content_type))
        .map_err(|e| LibvirtError::Invalid(format!("remote_write decode: {e}")))?;
    let mut history_points_added = 0usize;
    if let Some((cpu, mem, disk)) = host_percents_from_remote_write(&decoded) {
        let host = get_host_stats();
        store.push(MetricsHistoryPoint {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            host_cpu_percent: cpu,
            host_memory_percent: mem,
            host_disk_percent: disk,
            load_1: host.load_1,
            vms_running: 0,
            vm_count: 0,
            vm_metrics: Vec::new(),
        });
        history_points_added = 1;
    }
    Ok(Json(serde_json::json!({
        "status": "ok",
        "protocol": decoded.protocol,
        "compression": "snappy",
        "timeseries_count": decoded.timeseries_count,
        "sample_count": decoded.sample_count,
        "unique_metric_names": decoded.samples.len(),
        "history_points_added": history_points_added,
        "recognized_host_metrics": history_points_added > 0,
    })))
}

async fn post_metrics_ingest_prometheus(
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<MetricsHistoryStore>,
    body: String,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "metrics:write")?;
    let samples = parse_prometheus_text(&body);
    if samples.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "no Prometheus samples parsed from body".into(),
        )));
    }
    let mut ingested = 0usize;
    if let Some((cpu, mem, disk)) = host_percents_from_samples(&samples) {
        let host = get_host_stats();
        store.push(MetricsHistoryPoint {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            host_cpu_percent: cpu,
            host_memory_percent: mem,
            host_disk_percent: disk,
            load_1: host.load_1,
            vms_running: 0,
            vm_count: 0,
            vm_metrics: Vec::new(),
        });
        ingested = 1;
    }
    Ok(Json(serde_json::json!({
        "status": "ok",
        "samples_parsed": samples.len(),
        "history_points_added": ingested,
        "recognized_host_metrics": ingested > 0,
    })))
}

pub fn metrics_routes() -> Router<LibvirtManager> {
    let remote_write = Router::new()
        .route(
            "/metrics/ingest/remote-write",
            post(post_metrics_ingest_remote_write),
        )
        .layer(DefaultBodyLimit::max(REMOTE_WRITE_MAX_BYTES));

    Router::new()
        .merge(remote_write)
        .route("/metrics", get(get_all_metrics))
        .route("/metrics/history", get(get_metrics_history))
        .route("/metrics/traces", get(get_metrics_traces))
        .route("/metrics/ingest/batch", post(post_metrics_ingest_batch))
        .route(
            "/metrics/ingest/prometheus",
            post(post_metrics_ingest_prometheus),
        )
        .route("/metrics/{name}", get(get_vm_metrics))
}
