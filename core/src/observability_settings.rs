// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! API-facing observability settings (subset of [`MachinaConfig`]).

use serde::{Deserialize, Serialize};

use crate::config::MachinaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilitySettingsView {
    pub otlp: OtlpSettingsView,
    pub metrics_history: MetricsHistoryRemoteView,
    pub audit: AuditObservabilityView,
    /// True when settings were loaded from a writable config file path.
    pub config_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSettingsView {
    pub enabled: bool,
    pub endpoint: String,
    pub interval_secs: u64,
    pub export_metrics: bool,
    pub export_logs: bool,
    pub export_traces: bool,
    /// Masked when set (`Bearer abcd…wxyz`).
    pub authorization: String,
    pub authorization_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsHistoryRemoteView {
    pub remote_write_url: String,
    pub remote_write_authorization: String,
    pub remote_write_authorization_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditObservabilityView {
    pub sign_lines: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObservabilitySettingsPatch {
    #[serde(default)]
    pub otlp_enabled: Option<bool>,
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    #[serde(default)]
    pub otlp_interval_secs: Option<u64>,
    #[serde(default)]
    pub otlp_export_metrics: Option<bool>,
    #[serde(default)]
    pub otlp_export_logs: Option<bool>,
    #[serde(default)]
    pub otlp_export_traces: Option<bool>,
    /// Set to empty string to clear; omit to leave unchanged.
    #[serde(default)]
    pub otlp_authorization: Option<String>,
    #[serde(default)]
    pub metrics_history_remote_write_url: Option<String>,
    #[serde(default)]
    pub metrics_history_remote_write_authorization: Option<String>,
    #[serde(default)]
    pub audit_sign_lines: Option<bool>,
}

fn mask_secret(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    if v.len() <= 12 {
        return "***".into();
    }
    // Char-based head/tail so a multi-byte secret can't panic on a byte-offset slice.
    let head: String = v.chars().take(6).collect();
    let tail: String = {
        let n = v.chars().count();
        v.chars().skip(n.saturating_sub(4)).collect()
    };
    format!("{head}…{tail}")
}

pub fn settings_view_from_config(cfg: &MachinaConfig) -> ObservabilitySettingsView {
    let path = if MachinaConfig::system_config_path().exists() {
        MachinaConfig::system_config_path()
    } else {
        MachinaConfig::user_config_path()
    };
    let otlp = &cfg.observability.otlp;
    let mh = &cfg.metrics_history;
    ObservabilitySettingsView {
        otlp: OtlpSettingsView {
            enabled: otlp.enabled,
            endpoint: otlp.endpoint.clone(),
            interval_secs: otlp.interval_secs,
            export_metrics: otlp.export_metrics,
            export_logs: otlp.export_logs,
            export_traces: otlp.export_traces,
            authorization_set: !otlp.authorization.trim().is_empty(),
            authorization: mask_secret(&otlp.authorization),
        },
        metrics_history: MetricsHistoryRemoteView {
            remote_write_url: mh.remote_write_url.clone(),
            remote_write_authorization_set: !mh.remote_write_authorization.trim().is_empty(),
            remote_write_authorization: mask_secret(&mh.remote_write_authorization),
        },
        audit: AuditObservabilityView {
            sign_lines: cfg.audit.sign_lines,
        },
        config_path: path.display().to_string(),
    }
}

pub fn apply_observability_patch(cfg: &mut MachinaConfig, patch: &ObservabilitySettingsPatch) {
    if let Some(v) = patch.otlp_enabled {
        cfg.observability.otlp.enabled = v;
    }
    if let Some(v) = &patch.otlp_endpoint {
        cfg.observability.otlp.endpoint = v.clone();
    }
    if let Some(v) = patch.otlp_interval_secs {
        cfg.observability.otlp.interval_secs = v.max(30);
    }
    if let Some(v) = patch.otlp_export_metrics {
        cfg.observability.otlp.export_metrics = v;
    }
    if let Some(v) = patch.otlp_export_logs {
        cfg.observability.otlp.export_logs = v;
    }
    if let Some(v) = patch.otlp_export_traces {
        cfg.observability.otlp.export_traces = v;
    }
    if let Some(v) = &patch.otlp_authorization {
        cfg.observability.otlp.authorization = v.clone();
    }
    if let Some(v) = &patch.metrics_history_remote_write_url {
        cfg.metrics_history.remote_write_url = v.clone();
    }
    if let Some(v) = &patch.metrics_history_remote_write_authorization {
        cfg.metrics_history.remote_write_authorization = v.clone();
    }
    if let Some(v) = patch.audit_sign_lines {
        cfg.audit.sign_lines = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_updates_otlp_and_audit() {
        let mut cfg = MachinaConfig::default();
        apply_observability_patch(
            &mut cfg,
            &ObservabilitySettingsPatch {
                otlp_enabled: Some(true),
                otlp_endpoint: Some("http://127.0.0.1:4318".into()),
                audit_sign_lines: Some(true),
                ..Default::default()
            },
        );
        assert!(cfg.observability.otlp.enabled);
        assert_eq!(cfg.observability.otlp.endpoint, "http://127.0.0.1:4318");
        assert!(cfg.audit.sign_lines);
    }

    #[test]
    fn patch_remote_write_and_masks_secrets() {
        let mut cfg = MachinaConfig::default();
        apply_observability_patch(
            &mut cfg,
            &ObservabilitySettingsPatch {
                metrics_history_remote_write_url: Some("http://127.0.0.1:9/ingest".into()),
                metrics_history_remote_write_authorization: Some("Bearer secret-token-xyz".into()),
                ..Default::default()
            },
        );
        let view = settings_view_from_config(&cfg);
        assert_eq!(
            view.metrics_history.remote_write_url,
            "http://127.0.0.1:9/ingest"
        );
        assert!(view.metrics_history.remote_write_authorization_set);
        assert!(view
            .metrics_history
            .remote_write_authorization
            .contains('…'));
    }
}
