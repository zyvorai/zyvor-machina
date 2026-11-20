// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Daemon process counters for Prometheus and health dashboards.

use machina_core::obs_counters::LIBVIRT_RECONNECTS;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct DaemonStats {
    started_at: Instant,
    auth_failures: Arc<AtomicU64>,
    auth_attempts: Arc<Mutex<BTreeMap<String, BTreeMap<String, u64>>>>,
    prometheus_scrapes: Arc<AtomicU64>,
    session_count_fn: Arc<dyn Fn() -> usize + Send + Sync>,
}

impl DaemonStats {
    pub fn new(session_count_fn: impl Fn() -> usize + Send + Sync + 'static) -> Self {
        Self {
            started_at: Instant::now(),
            auth_failures: Arc::new(AtomicU64::new(0)),
            auth_attempts: Arc::new(Mutex::new(BTreeMap::new())),
            prometheus_scrapes: Arc::new(AtomicU64::new(0)),
            session_count_fn: Arc::new(session_count_fn),
        }
    }

    pub fn inc_auth_attempt(&self, method: &str, result: &str) {
        let method = method.to_ascii_lowercase();
        let result = result.to_ascii_lowercase();
        if result == "failure" {
            self.auth_failures.fetch_add(1, Ordering::Relaxed);
        }
        let mut guard = self.auth_attempts.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entry(method)
            .or_default()
            .entry(result)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    pub fn inc_prometheus_scrape(&self) {
        self.prometheus_scrapes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn auth_failures(&self) -> u64 {
        self.auth_failures.load(Ordering::Relaxed)
    }

    pub fn prometheus_scrapes(&self) -> u64 {
        self.prometheus_scrapes.load(Ordering::Relaxed)
    }

    pub fn active_sessions(&self) -> usize {
        (self.session_count_fn)()
    }

    pub fn libvirt_reconnects(&self) -> u64 {
        LIBVIRT_RECONNECTS.load(Ordering::Relaxed)
    }

    pub fn render_auth_prometheus(&self) -> String {
        let guard = self.auth_attempts.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "# HELP machina_daemon_auth_attempts_total Login attempts by method and result\n\
             # TYPE machina_daemon_auth_attempts_total counter\n",
        );
        for (method, results) in guard.iter() {
            for (result, count) in results {
                let _ = writeln!(
                    out,
                    "machina_daemon_auth_attempts_total{{method=\"{method}\",result=\"{result}\"}} {count}"
                );
            }
        }
        out
    }
}
