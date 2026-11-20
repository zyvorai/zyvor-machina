// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional remote shipping for Machina audit events (syslog + HTTP webhook).

use std::sync::{OnceLock, RwLock};

use crate::config::AuditLogConfig;
use crate::state::AuditEvent;

static SHIP_CFG: OnceLock<RwLock<AuditLogConfig>> = OnceLock::new();

fn ship_lock() -> &'static RwLock<AuditLogConfig> {
    SHIP_CFG.get_or_init(|| RwLock::new(AuditLogConfig::default()))
}

pub fn configure_ship(cfg: AuditLogConfig) {
    if let Some(lock) = SHIP_CFG.get() {
        *lock.write().unwrap_or_else(|e| e.into_inner()) = cfg;
    } else {
        let _ = SHIP_CFG.set(RwLock::new(cfg));
    }
}

fn ship_cfg() -> AuditLogConfig {
    ship_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[cfg(target_os = "linux")]
fn syslog_line(line: &str) {
    use std::ffi::CString;
    let msg = CString::new(line.chars().take(900).collect::<String>()).unwrap_or_default();
    unsafe {
        libc::openlog(c"machina".as_ptr(), libc::LOG_PID, libc::LOG_AUTHPRIV);
        libc::syslog(libc::LOG_INFO, c"%s".as_ptr(), msg.as_ptr());
        libc::closelog();
    }
}

#[cfg(not(target_os = "linux"))]
fn syslog_line(_line: &str) {}

fn post_webhook(url: &str, auth: &str, event: &AuditEvent) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("audit webhook client: {e}");
            return;
        }
    };
    let mut req = client.post(url).json(event);
    if !auth.is_empty() {
        req = req.header("Authorization", auth);
    }
    match req.send() {
        Ok(res) if res.status().is_success() => {}
        Ok(res) => tracing::warn!("audit webhook HTTP {}", res.status()),
        Err(e) => tracing::warn!("audit webhook failed: {e}"),
    }
}

/// Ship a single audit event per `[audit]` syslog / webhook settings.
pub fn ship_audit_event(event: &AuditEvent) {
    let cfg = ship_cfg();
    let line = if event.actor.is_empty() {
        format!(
            "machina-audit {} action={} target={} result={}",
            event.timestamp, event.action, event.target, event.result
        )
    } else {
        format!(
            "machina-audit {} action={} target={} result={} actor={}",
            event.timestamp, event.action, event.target, event.result, event.actor
        )
    };

    if cfg.syslog_enabled {
        syslog_line(&line);
    }

    let url = cfg.http_webhook_url.trim();
    if !url.is_empty() {
        let event = event.clone();
        let auth = cfg.webhook_authorization.clone();
        let url = url.to_string();
        std::thread::spawn(move || post_webhook(&url, &auth, &event));
    }
}

/// Drain recent audit lines for OTLP log export.
pub fn recent_events_for_otlp(limit: usize) -> Vec<AuditEvent> {
    crate::audit::load_audit_events(limit)
}
