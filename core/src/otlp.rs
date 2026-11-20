// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Build OTLP/HTTP JSON payloads (metrics + logs) for OpenTelemetry collectors.

use serde_json::{json, Value};

use crate::metrics_history::MetricsHistoryPoint;
use crate::state::AuditEvent;

fn now_unix_nano() -> String {
    chrono::Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or(0)
        .to_string()
}

fn attr_str(key: &str, v: &str) -> Value {
    json!({ "key": key, "value": { "stringValue": v } })
}

fn resource_attrs(service: &str, host: &str) -> Vec<Value> {
    vec![
        attr_str("service.name", service),
        attr_str("host.name", host),
    ]
}

fn gauge_metric(name: &str, value: f64, attrs: Vec<Value>) -> Value {
    json!({
        "name": name,
        "gauge": {
            "dataPoints": [{
                "attributes": attrs,
                "timeUnixNano": now_unix_nano(),
                "asDouble": value
            }]
        }
    })
}

fn sum_metric(name: &str, value: f64, attrs: Vec<Value>, cumulative: bool) -> Value {
    json!({
        "name": name,
        "sum": {
            "aggregationTemporality": if cumulative { 2 } else { 1 },
            "isMonotonic": true,
            "dataPoints": [{
                "attributes": attrs,
                "timeUnixNano": now_unix_nano(),
                "asDouble": value
            }]
        }
    })
}

/// OTLP metrics JSON body from a metrics-history sample.
pub fn build_metrics_export_payload(
    hostname: &str,
    point: &MetricsHistoryPoint,
    daemon_uptime_secs: u64,
    auth_failures: u64,
) -> Value {
    let mut metrics: Vec<Value> = vec![
        gauge_metric("machina.host.cpu_percent", point.host_cpu_percent, vec![]),
        gauge_metric(
            "machina.host.memory_percent",
            point.host_memory_percent,
            vec![],
        ),
        gauge_metric("machina.host.disk_percent", point.host_disk_percent, vec![]),
        gauge_metric("machina.host.load_1", point.load_1, vec![]),
        gauge_metric("machina.vms.running", point.vms_running as f64, vec![]),
        gauge_metric("machina.vms.defined", point.vm_count as f64, vec![]),
        gauge_metric(
            "machina.daemon.uptime_seconds",
            daemon_uptime_secs as f64,
            vec![],
        ),
        sum_metric(
            "machina.daemon.auth_failures_total",
            auth_failures as f64,
            vec![],
            true,
        ),
    ];

    for vm in &point.vm_metrics {
        let mut attrs = vec![attr_str("vm.name", &vm.name)];
        if let Some(c) = &vm.libvirt_connection {
            attrs.push(attr_str("libvirt.connection", c));
        }
        metrics.push(gauge_metric(
            "machina.vm.memory_percent",
            vm.memory_pct,
            attrs.clone(),
        ));
        metrics.push(gauge_metric(
            "machina.vm.memory_used_mb",
            vm.memory_used_mb as f64,
            attrs.clone(),
        ));
        metrics.push(sum_metric(
            "machina.vm.disk_read_bytes_total",
            vm.disk_rd_bytes as f64,
            attrs.clone(),
            true,
        ));
        metrics.push(sum_metric(
            "machina.vm.disk_write_bytes_total",
            vm.disk_wr_bytes as f64,
            attrs.clone(),
            true,
        ));
        metrics.push(sum_metric(
            "machina.vm.net_rx_bytes_total",
            vm.net_rx_bytes as f64,
            attrs.clone(),
            true,
        ));
        metrics.push(sum_metric(
            "machina.vm.net_tx_bytes_total",
            vm.net_tx_bytes as f64,
            attrs,
            true,
        ));
    }

    json!({
        "resourceMetrics": [{
            "resource": { "attributes": resource_attrs("machina-daemon", hostname) },
            "scopeMetrics": [{
                "scope": { "name": "machina" },
                "metrics": metrics
            }]
        }]
    })
}

/// OTLP logs JSON body for Machina audit events.
pub fn build_logs_export_payload(hostname: &str, events: &[AuditEvent]) -> Value {
    let records: Vec<Value> = events
        .iter()
        .map(|e| {
            let body = if e.actor.is_empty() {
                format!("{} {} {} {}", e.timestamp, e.action, e.target, e.result)
            } else {
                format!(
                    "{} {} {} {} actor={}",
                    e.timestamp, e.action, e.target, e.result, e.actor
                )
            };
            json!({
                "timeUnixNano": now_unix_nano(),
                "severityNumber": if e.result.eq_ignore_ascii_case("ok") || e.result.eq_ignore_ascii_case("success") { 9 } else { 13 },
                "severityText": e.result,
                "body": { "stringValue": body },
                "attributes": [
                    attr_str("machina.audit.action", &e.action),
                    attr_str("machina.audit.target", &e.target),
                    attr_str("machina.audit.result", &e.result),
                    attr_str("machina.audit.actor", &e.actor),
                ]
            })
        })
        .collect();

    json!({
        "resourceLogs": [{
            "resource": { "attributes": resource_attrs("machina-daemon", hostname) },
            "scopeLogs": [{
                "scope": { "name": "machina.audit" },
                "logRecords": records
            }]
        }]
    })
}

/// One HTTP request span for OTLP trace export.
#[derive(Debug, Clone)]
pub struct OtlpHttpSpan {
    pub trace_id: String,
    pub span_id: String,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub duration_ms: u64,
    pub timestamp_ms: i64,
}

/// OTLP traces JSON (HTTP server spans).
pub fn build_traces_export_payload(hostname: &str, spans: &[OtlpHttpSpan]) -> Value {
    let spans_json: Vec<Value> = spans
        .iter()
        .map(|s| {
            let name = format!("{} {}", s.method, s.route);
            let start_ns = (s.timestamp_ms as i128 - s.duration_ms as i128)
                .max(0)
                .saturating_mul(1_000_000);
            json!({
                "traceId": s.trace_id,
                "spanId": s.span_id,
                "name": name,
                "kind": 2,
                "startTimeUnixNano": start_ns.to_string(),
                "endTimeUnixNano": (s.timestamp_ms as i128 * 1_000_000).to_string(),
                "attributes": [
                    attr_str("http.method", &s.method),
                    attr_str("http.route", &s.route),
                    attr_str("http.status_code", &s.status.to_string()),
                ],
                "status": { "code": if s.status < 400 { 1 } else { 2 } }
            })
        })
        .collect();

    json!({
        "resourceSpans": [{
            "resource": { "attributes": resource_attrs("machina-daemon", hostname) },
            "scopeSpans": [{
                "scope": { "name": "machina.http" },
                "spans": spans_json
            }]
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_payload_includes_host_gauges() {
        let point = MetricsHistoryPoint {
            timestamp_ms: 1,
            host_cpu_percent: 10.0,
            host_memory_percent: 20.0,
            host_disk_percent: 30.0,
            load_1: 0.5,
            vms_running: 2,
            vm_count: 4,
            vm_metrics: vec![],
        };
        let body = build_metrics_export_payload("host1", &point, 100, 3);
        let names: Vec<String> = body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
            .map(str::to_string)
            .collect();
        assert!(names.iter().any(|n| n == "machina.host.cpu_percent"));
        assert!(names.iter().any(|n| n == "machina.vms.running"));
    }

    #[test]
    fn traces_payload_has_http_span() {
        let spans = vec![OtlpHttpSpan {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            method: "GET".into(),
            route: "/api/v1/vms".into(),
            status: 200,
            duration_ms: 12,
            timestamp_ms: 1_700_000_000_000,
        }];
        let body = build_traces_export_payload("host1", &spans);
        let out = body["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["name"], "GET /api/v1/vms");
    }
}
