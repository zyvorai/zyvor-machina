// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Process-wide observability counters (incremented from core; read by the daemon for Prometheus).

use std::sync::atomic::{AtomicU64, Ordering};

/// Libvirt reconnect attempts after a dead connection.
pub static LIBVIRT_RECONNECTS: AtomicU64 = AtomicU64::new(0);
/// Successful API token validations (`Bearer mach_…`).
pub static API_TOKEN_AUTH_OK: AtomicU64 = AtomicU64::new(0);
/// Rejected API token attempts.
pub static API_TOKEN_AUTH_FAIL: AtomicU64 = AtomicU64::new(0);
/// Last Prometheus scrape wall time in milliseconds (for duration gauge).
pub static PROMETHEUS_LAST_SCRAPE_MS: AtomicU64 = AtomicU64::new(0);
/// Duration of last Prometheus scrape in milliseconds.
pub static PROMETHEUS_LAST_SCRAPE_DURATION_MS: AtomicU64 = AtomicU64::new(0);

pub fn inc_libvirt_reconnect() {
    LIBVIRT_RECONNECTS.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_api_token_ok() {
    API_TOKEN_AUTH_OK.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_api_token_fail() {
    API_TOKEN_AUTH_FAIL.fetch_add(1, Ordering::Relaxed);
}

pub fn record_prometheus_scrape(duration_ms: u64) {
    PROMETHEUS_LAST_SCRAPE_DURATION_MS.store(duration_ms, Ordering::Relaxed);
    PROMETHEUS_LAST_SCRAPE_MS.store(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        Ordering::Relaxed,
    );
}
