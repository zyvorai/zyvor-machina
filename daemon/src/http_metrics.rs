// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! In-process HTTP request latency histograms for Prometheus.

use axum::body::Body;
use axum::extract::{Extension, Request};
use axum::middleware::Next;
use axum::response::Response;
use http::header::{HeaderName, HeaderValue};
use machina_core::{format_traceparent, trace_context_from_headers};
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Recent HTTP request for OTLP trace export.
#[derive(Clone, Debug, serde::Serialize)]
pub struct HttpTraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub duration_ms: u64,
    pub timestamp_ms: i64,
}

/// Prometheus histogram bucket upper bounds in seconds.
pub const HISTOGRAM_BUCKETS_SEC: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Clone, Hash, Eq, PartialEq)]
struct RouteKey {
    method: String,
    route: String,
}

struct RouteStats {
    buckets: Vec<u64>,
    sum_sec: f64,
    count: u64,
    status_counts: HashMap<u16, u64>,
}

impl Default for RouteStats {
    fn default() -> Self {
        Self {
            buckets: vec![0; HISTOGRAM_BUCKETS_SEC.len()],
            sum_sec: 0.0,
            count: 0,
            status_counts: HashMap::new(),
        }
    }
}

impl RouteStats {
    fn observe(&mut self, duration: Duration, status: u16) {
        let sec = duration.as_secs_f64();
        for (i, bound) in HISTOGRAM_BUCKETS_SEC.iter().enumerate() {
            if sec <= *bound {
                self.buckets[i] += 1;
            }
        }
        self.sum_sec += sec;
        self.count += 1;
        *self.status_counts.entry(status).or_insert(0) += 1;
    }
}

const MAX_TRACE_SPANS: usize = 256;

/// Cap on distinct (method, route) label combinations. `record_request` runs on every
/// request — including unauthenticated ones hitting the SPA fallback or 404s — and
/// `normalize_path` only collapses segments it recognizes as dynamic. Without a cap, an
/// attacker sweeping many distinct nonexistent paths could grow `inner` without bound
/// (unbounded memory growth / DoS) before auth is ever checked. Once the cap is hit,
/// unseen route keys are folded into a shared "other" bucket instead of being dropped,
/// so the Prometheus counters still account for the traffic.
const MAX_ROUTE_KEYS: usize = 2000;

#[derive(Clone)]
pub struct HttpMetrics {
    inner: Arc<Mutex<HashMap<RouteKey, RouteStats>>>,
    traces: Arc<Mutex<VecDeque<HttpTraceSpan>>>,
}

impl HttpMetrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            traces: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn record(
        &self,
        method: &str,
        route: &str,
        status: u16,
        duration: Duration,
        trace_id: &str,
        span_id: &str,
    ) {
        let mut key = RouteKey {
            method: method.to_ascii_uppercase(),
            route: route.to_string(),
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.contains_key(&key) && guard.len() >= MAX_ROUTE_KEYS {
            key = RouteKey {
                method: key.method,
                route: "other".to_string(),
            };
        }
        guard
            .entry(key.clone())
            .or_default()
            .observe(duration, status);
        let mut traces = self.traces.lock().unwrap_or_else(|e| e.into_inner());
        traces.push_back(HttpTraceSpan {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            method: key.method,
            route: key.route,
            status,
            duration_ms: duration.as_millis() as u64,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
        while traces.len() > MAX_TRACE_SPANS {
            traces.pop_front();
        }
    }

    pub fn recent_traces(&self, limit: usize) -> Vec<HttpTraceSpan> {
        let traces = self.traces.lock().unwrap_or_else(|e| e.into_inner());
        let lim = limit.min(traces.len());
        traces.iter().rev().take(lim).cloned().collect()
    }

    pub fn render_prometheus(&self) -> String {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut keys: Vec<_> = guard.keys().collect();
        keys.sort_by(|a, b| (&a.method, &a.route).cmp(&(&b.method, &b.route)));

        let mut out = String::new();
        out.push_str("# HELP machina_http_request_duration_seconds API request latency\n");
        out.push_str("# TYPE machina_http_request_duration_seconds histogram\n");
        for key in &keys {
            let stats = &guard[key];
            let labels = format!(
                "method=\"{}\",route=\"{}\"",
                escape_label(&key.method),
                escape_label(&key.route)
            );
            for (i, bound) in HISTOGRAM_BUCKETS_SEC.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "machina_http_request_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {}",
                    stats.buckets[i]
                );
            }
            let _ = writeln!(
                out,
                "machina_http_request_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {}",
                stats.count
            );
            let _ = writeln!(
                out,
                "machina_http_request_duration_seconds_sum{{{labels}}} {:.6}",
                stats.sum_sec
            );
            let _ = writeln!(
                out,
                "machina_http_request_duration_seconds_count{{{labels}}} {}",
                stats.count
            );
        }

        out.push_str(
            "# HELP machina_http_requests_total API requests by method, route, and status\n",
        );
        out.push_str("# TYPE machina_http_requests_total counter\n");
        for key in keys {
            let stats = &guard[key];
            let mut statuses: Vec<_> = stats.status_counts.iter().collect();
            statuses.sort_by_key(|(code, _)| **code);
            for (status, count) in statuses {
                let labels = format!(
                    "method=\"{}\",route=\"{}\",status=\"{status}\"",
                    escape_label(&key.method),
                    escape_label(&key.route)
                );
                let _ = writeln!(out, "machina_http_requests_total{{{labels}}} {count}");
            }
        }

        out
    }
}

fn escape_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Collapse dynamic path segments to keep Prometheus cardinality bounded.
pub fn normalize_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return "/".to_string();
    }

    let mut out = String::new();
    let mut i = 0;
    while i < parts.len() {
        let prev = if i > 0 { Some(parts[i - 1]) } else { None };
        let seg = parts[i];
        let normalized = if is_dynamic_segment(prev, seg) {
            dynamic_placeholder(prev)
        } else {
            seg
        };
        out.push('/');
        out.push_str(normalized);
        i += 1;
    }
    out
}

fn dynamic_placeholder(parent: Option<&str>) -> &'static str {
    match parent {
        Some("vms") | Some("templates") | Some("snapshots") => ":name",
        Some("jobs") | Some("events") | Some("ws-token") => ":id",
        Some("clusters") | Some("namespaces") | Some("nodes") | Some("pods") => ":name",
        _ => ":id",
    }
}

fn is_dynamic_segment(parent: Option<&str>, seg: &str) -> bool {
    if is_uuid_like(seg) {
        return true;
    }
    if seg.chars().all(|c| c.is_ascii_digit()) && seg.len() >= 4 {
        return true;
    }
    if seg.len() > 80 {
        return true;
    }
    matches!(
        parent,
        Some(
            "vms"
                | "templates"
                | "snapshots"
                | "jobs"
                | "events"
                | "clusters"
                | "namespaces"
                | "nodes"
                | "pods",
        )
    )
}

fn is_uuid_like(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    s.chars().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => c == '-',
        _ => c.is_ascii_hexdigit(),
    })
}

pub async fn record_request(
    Extension(metrics): Extension<Arc<HttpMetrics>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().as_str().to_string();
    let route = normalize_path(req.uri().path());
    let traceparent_in = req
        .headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok());
    let trace_ctx = trace_context_from_headers(traceparent_in);
    let start = Instant::now();
    let response = next.run(req).await;
    metrics.record(
        &method,
        &route,
        response.status().as_u16(),
        start.elapsed(),
        &trace_ctx.trace_id,
        &trace_ctx.span_id,
    );
    let mut response = response;
    if let Ok(val) = HeaderValue::from_str(&format_traceparent(&trace_ctx)) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("traceparent"), val);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_vm_name() {
        assert_eq!(
            normalize_path("/api/v1/vms/my-vm/metrics"),
            "/api/v1/vms/:name/metrics"
        );
    }

    #[test]
    fn normalize_uuid() {
        assert_eq!(
            normalize_path("/api/v1/jobs/550e8400-e29b-41d4-a716-446655440000"),
            "/api/v1/jobs/:id"
        );
    }
}
