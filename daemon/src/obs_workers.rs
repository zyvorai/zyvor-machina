// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Manage cancellable OTLP and metrics-history background workers.

use machina_core::MachinaConfig;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::daemon_stats::DaemonStats;
use crate::http_metrics::HttpMetrics;
use crate::metrics_history::MetricsHistoryStore;
use crate::obs_reload::bump_observability_reload;
use crate::{metrics_history, otlp_worker};
use machina_core::LibvirtManager;

pub struct ObservabilityWorkers {
    otlp_cancel: Mutex<Option<CancellationToken>>,
    metrics_cancel: Mutex<Option<CancellationToken>>,
}

impl ObservabilityWorkers {
    pub fn new() -> Self {
        Self {
            otlp_cancel: Mutex::new(None),
            metrics_cancel: Mutex::new(None),
        }
    }

    pub fn start(
        &self,
        manager: &LibvirtManager,
        config: &MachinaConfig,
        store: &MetricsHistoryStore,
        stats: &Arc<DaemonStats>,
        http_metrics: &Arc<HttpMetrics>,
    ) {
        self.restart_otlp(manager.clone(), stats.clone(), http_metrics.clone());
        self.restart_metrics(manager.clone(), store.clone(), config);
    }

    pub fn reload(
        &self,
        manager: &LibvirtManager,
        config: &MachinaConfig,
        store: &MetricsHistoryStore,
        stats: &Arc<DaemonStats>,
        http_metrics: &Arc<HttpMetrics>,
    ) {
        store.set_max_points(config.metrics_history.max_points);
        bump_observability_reload();
        self.restart_otlp(manager.clone(), stats.clone(), http_metrics.clone());
        self.restart_metrics(manager.clone(), store.clone(), config);
    }

    fn restart_otlp(
        &self,
        manager: LibvirtManager,
        stats: Arc<DaemonStats>,
        http_metrics: Arc<HttpMetrics>,
    ) {
        if let Some(old) = self
            .otlp_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            old.cancel();
        }
        let token = CancellationToken::new();
        *self.otlp_cancel.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        otlp_worker::spawn_otlp_worker(manager, stats, http_metrics, token);
    }

    fn restart_metrics(
        &self,
        manager: LibvirtManager,
        store: MetricsHistoryStore,
        config: &MachinaConfig,
    ) {
        if let Some(old) = self
            .metrics_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            old.cancel();
        }
        let token = CancellationToken::new();
        *self
            .metrics_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        if config.metrics_history.persist {
            store.load_from_disk(config.metrics_history.max_points);
        }
        metrics_history::spawn_metrics_history_worker(manager, store, token);
    }
}
