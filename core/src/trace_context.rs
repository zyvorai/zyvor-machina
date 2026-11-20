// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! W3C Trace Context helpers for HTTP request correlation.

/// Parsed or generated trace identifiers for one HTTP request.
#[derive(Debug, Clone)]
pub struct HttpTraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
}

/// Parse `traceparent` (W3C) or generate a new root context.
pub fn trace_context_from_headers(traceparent: Option<&str>) -> HttpTraceContext {
    if let Some(tp) = traceparent {
        if let Some(ctx) = parse_traceparent(tp) {
            return ctx;
        }
    }
    HttpTraceContext {
        trace_id: random_trace_id(),
        span_id: random_span_id(),
        parent_span_id: None,
    }
}

/// Build a `traceparent` response header value for this span.
pub fn format_traceparent(ctx: &HttpTraceContext) -> String {
    format!("00-{}-{}-01", ctx.trace_id, ctx.span_id)
}

fn parse_traceparent(value: &str) -> Option<HttpTraceContext> {
    let parts: Vec<&str> = value.trim().split('-').collect();
    if parts.len() != 4 || parts[0] != "00" {
        return None;
    }
    let trace_id = parts[1];
    let parent_span_id = parts[2];
    if trace_id.len() != 32
        || parent_span_id.len() != 16
        || !trace_id.chars().all(|c| c.is_ascii_hexdigit())
        || !parent_span_id.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    Some(HttpTraceContext {
        trace_id: trace_id.to_ascii_lowercase(),
        span_id: random_span_id(),
        parent_span_id: Some(parent_span_id.to_ascii_lowercase()),
    })
}

fn random_trace_id() -> String {
    random_hex(16)
}

fn random_span_id() -> String {
    random_hex(8)
}

fn random_hex(bytes: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = String::with_capacity(bytes * 2);
    let mut x = seed ^ (bytes as u128).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for _ in 0..bytes {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push_str(&format!("{:02x}", (x & 0xFF) as u8));
        x >>= 8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_traceparent() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = parse_traceparent(tp).unwrap();
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.parent_span_id.as_deref(), Some("00f067aa0ba902b7"));
        assert_eq!(ctx.span_id.len(), 16);
    }

    #[test]
    fn format_roundtrip_prefix() {
        let ctx = trace_context_from_headers(None);
        let tp = format_traceparent(&ctx);
        assert!(tp.starts_with("00-"));
        assert!(tp.ends_with("-01"));
    }
}
