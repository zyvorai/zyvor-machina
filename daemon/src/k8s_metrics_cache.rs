// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Last `kubectl top` probe result for Prometheus and health checks.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

static LAST_PROBE_UNIX: AtomicI64 = AtomicI64::new(0);
static METRICS_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn record_k8s_metrics_probe(available: bool) {
    METRICS_AVAILABLE.store(available, Ordering::Relaxed);
    LAST_PROBE_UNIX.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
}

pub fn k8s_metrics_available() -> bool {
    METRICS_AVAILABLE.load(Ordering::Relaxed)
}

pub fn k8s_last_probe_unix() -> Option<i64> {
    let ts = LAST_PROBE_UNIX.load(Ordering::Relaxed);
    if ts > 0 {
        Some(ts)
    } else {
        None
    }
}
