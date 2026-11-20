// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Periodic OTLP/HTTP export of metrics and audit logs.

use machina_core::config::OtlpExportConfig;
use machina_core::libvirt::extras::get_host_stats;
use machina_core::metrics_history::MetricsHistoryPoint;
use machina_core::otlp::{
    build_logs_export_payload, build_metrics_export_payload, build_traces_export_payload,
    OtlpHttpSpan,
};
use machina_core::{audit_ship, LibvirtManager, MachinaConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::daemon_stats::DaemonStats;
use crate::http_metrics::HttpMetrics;
use crate::obs_reload::observability_reload_generation;

fn normalize_endpoint(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with(path) {
        base.to_string()
    } else {
        format!("{base}{path}")
    }
}

async fn post_otlp(
    client: &reqwest::Client,
    url: &str,
    auth: &str,
    body: serde_json::Value,
) -> Result<(), String> {
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body);
    if !auth.is_empty() {
        req = req.header("Authorization", auth);
    }
    let res = req.send().await.map_err(|e| format!("request: {e}"))?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", res.status()))
    }
}

fn sample_point(manager: &LibvirtManager) -> MetricsHistoryPoint {
    let host = get_host_stats();
    let vms = manager.list_all_vms().unwrap_or_default();
    let vm_count = vms.len() as u32;
    let vms_running = vms
        .iter()
        .filter(|v| v.state.eq_ignore_ascii_case("running"))
        .count() as u32;
    let vm_metrics = manager.merge_all_metrics().unwrap_or_default();
    MetricsHistoryPoint {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        host_cpu_percent: host.cpu_percent,
        host_memory_percent: host.memory_percent,
        host_disk_percent: host.disk_percent,
        load_1: host.load_1,
        vms_running,
        vm_count,
        vm_metrics,
    }
}

pub fn spawn_otlp_worker(
    manager: LibvirtManager,
    stats: Arc<DaemonStats>,
    http_metrics: Arc<HttpMetrics>,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("OTLP export: HTTP client: {e}");
                return;
            }
        };
        let hostname = std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "machina-host".into());

        let mut reload_gen = observability_reload_generation();
        let mut cfg = MachinaConfig::load().observability.otlp;
        let mut interval = worker_interval(&cfg);
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        log_otlp_start(&cfg, interval);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("OTLP export worker stopped");
                    break;
                }
                _ = tick.tick() => {}
            }

            let gen = observability_reload_generation();
            if gen != reload_gen {
                reload_gen = gen;
                cfg = MachinaConfig::load().observability.otlp;
                interval = worker_interval(&cfg);
                tick = tokio::time::interval(interval);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                log_otlp_start(&cfg, interval);
            }

            if !cfg.is_enabled() {
                continue;
            }

            let metrics_url = normalize_endpoint(&cfg.endpoint, "/v1/metrics");
            let logs_url = normalize_endpoint(&cfg.endpoint, "/v1/logs");
            let traces_url = normalize_endpoint(&cfg.endpoint, "/v1/traces");
            let auth = cfg.authorization.clone();
            let export_metrics = cfg.export_metrics;
            let export_logs = cfg.export_logs;
            let export_traces = cfg.export_traces;

            if export_metrics {
                let mgr = manager.clone();
                let hostname3 = hostname.clone();
                let stats2 = stats.clone();
                let body = tokio::task::spawn_blocking(move || {
                    let point = sample_point(&mgr);
                    build_metrics_export_payload(
                        &hostname3,
                        &point,
                        stats2.uptime_seconds(),
                        stats2.auth_failures(),
                    )
                })
                .await;
                match body {
                    Ok(body) => match post_otlp(&client, &metrics_url, &auth, body).await {
                        Ok(()) => tracing::debug!("OTLP metrics export ok"),
                        Err(e) => tracing::warn!("OTLP metrics export failed: {e}"),
                    },
                    Err(e) => tracing::warn!("OTLP metrics sample failed: {e}"),
                }
            }
            if export_logs {
                let events = audit_ship::recent_events_for_otlp(80);
                if !events.is_empty() {
                    let body = build_logs_export_payload(&hostname, &events);
                    match post_otlp(&client, &logs_url, &auth, body).await {
                        Ok(()) => tracing::debug!("OTLP logs export ok ({} events)", events.len()),
                        Err(e) => tracing::warn!("OTLP logs export failed: {e}"),
                    }
                }
            }
            if export_traces {
                let spans: Vec<OtlpHttpSpan> = http_metrics
                    .recent_traces(64)
                    .into_iter()
                    .map(|s| OtlpHttpSpan {
                        trace_id: s.trace_id,
                        span_id: s.span_id,
                        method: s.method,
                        route: s.route,
                        status: s.status,
                        duration_ms: s.duration_ms,
                        timestamp_ms: s.timestamp_ms,
                    })
                    .collect();
                if !spans.is_empty() {
                    let body = build_traces_export_payload(&hostname, &spans);
                    match post_otlp(&client, &traces_url, &auth, body).await {
                        Ok(()) => tracing::debug!("OTLP traces export ok ({} spans)", spans.len()),
                        Err(e) => tracing::warn!("OTLP traces export failed: {e}"),
                    }
                }
            }
        }
    });
}

fn worker_interval(cfg: &OtlpExportConfig) -> Duration {
    Duration::from_secs(cfg.interval_secs.max(30))
}

fn log_otlp_start(cfg: &OtlpExportConfig, interval: Duration) {
    if !cfg.is_enabled() {
        tracing::info!("OTLP export disabled");
        return;
    }
    let endpoint = cfg.endpoint.trim();
    let metrics_url = normalize_endpoint(endpoint, "/v1/metrics");
    let logs_url = normalize_endpoint(endpoint, "/v1/logs");
    let traces_url = normalize_endpoint(endpoint, "/v1/traces");
    tracing::info!(
        "OTLP export every {:?} → metrics={} logs={} traces={}",
        interval,
        metrics_url,
        logs_url,
        traces_url
    );
}
