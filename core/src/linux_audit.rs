// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Linux auditd event probes (`/var/log/audit/audit.log` or `ausearch`).

use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

use std::sync::OnceLock;

use crate::config::LinuxAuditConfig;
use crate::LibvirtError;

static PROBE_CFG: OnceLock<LinuxAuditConfig> = OnceLock::new();

pub fn configure_linux_audit(cfg: LinuxAuditConfig) {
    let _ = PROBE_CFG.set(cfg);
}

fn probe_cfg() -> LinuxAuditConfig {
    PROBE_CFG.get().cloned().unwrap_or_default()
}

pub fn health_avc_threshold() -> u32 {
    probe_cfg().health_avc_threshold
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxAuditEvent {
    pub timestamp: String,
    pub event_type: String,
    pub summary: String,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxAuditReport {
    pub available: bool,
    pub source: String,
    pub events: Vec<LinuxAuditEvent>,
    pub avc_count: u32,
}

#[cfg(target_os = "linux")]
fn parse_audit_log_line(line: &str) -> Option<LinuxAuditEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let event_type = if line.contains("type=AVC") || line.contains("type=USER_AVC") {
        "AVC"
    } else if line.contains("type=USER_LOGIN") {
        "USER_LOGIN"
    } else if line.contains("type=USER_AUTH") {
        "USER_AUTH"
    } else if line.contains("type=SYSCALL") {
        "SYSCALL"
    } else if let Some(t) = line
        .split("type=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
    {
        t.trim_end_matches(':')
    } else {
        "UNKNOWN"
    };
    let ts = line
        .split_whitespace()
        .find(|p| p.contains(':'))
        .unwrap_or("")
        .trim_end_matches(':')
        .to_string();
    let summary = line.chars().take(280).collect();
    Some(LinuxAuditEvent {
        timestamp: ts,
        event_type: event_type.to_string(),
        summary,
        raw: line.chars().take(512).collect(),
    })
}

#[cfg(target_os = "linux")]
fn tail_audit_log(path: &Path, max_events: usize) -> Option<Vec<LinuxAuditEvent>> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut events: Vec<LinuxAuditEvent> = content
        .lines()
        .rev()
        .filter_map(parse_audit_log_line)
        .take(max_events)
        .collect();
    events.reverse();
    Some(events)
}

#[cfg(target_os = "linux")]
fn probe_ausearch(max_events: usize) -> Option<Vec<LinuxAuditEvent>> {
    let out = Command::new("ausearch")
        .args([
            "--start",
            "recent",
            "-m",
            "AVC,USER_AVC,USER_LOGIN,USER_AUTH",
            "-i",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut events = Vec::new();
    for block in text.split("----") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let event_type = if block.contains("type=AVC") || block.contains("USER_AVC") {
            "AVC"
        } else if block.contains("USER_LOGIN") {
            "USER_LOGIN"
        } else if block.contains("USER_AUTH") {
            "USER_AUTH"
        } else {
            "AUDIT"
        };
        let ts = block
            .lines()
            .find(|l| l.contains("time->"))
            .and_then(|l| l.split("time->").nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let summary = block
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(280)
            .collect();
        events.push(LinuxAuditEvent {
            timestamp: ts,
            event_type: event_type.to_string(),
            summary,
            raw: block.chars().take(512).collect(),
        });
        if events.len() >= max_events {
            break;
        }
    }
    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

pub fn gather_linux_audit(max_events: usize) -> Result<LinuxAuditReport, LibvirtError> {
    gather_linux_audit_with_cfg(max_events, &probe_cfg())
}

pub fn gather_linux_audit_configured() -> Result<LinuxAuditReport, LibvirtError> {
    let cfg = probe_cfg();
    if !cfg.enabled {
        return Ok(LinuxAuditReport::default());
    }
    gather_linux_audit_with_cfg(cfg.max_events, &cfg)
}

fn gather_linux_audit_with_cfg(
    max_events: usize,
    _cfg: &LinuxAuditConfig,
) -> Result<LinuxAuditReport, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = max_events;
        return Ok(LinuxAuditReport::default());
    }
    #[cfg(target_os = "linux")]
    {
        let lim = max_events.clamp(10, 2000);
        let paths = [
            Path::new("/var/log/audit/audit.log"),
            Path::new("/var/log/audit.log"),
        ];
        for path in paths {
            if let Some(events) = tail_audit_log(path, lim) {
                let avc_count = events.iter().filter(|e| e.event_type == "AVC").count() as u32;
                return Ok(LinuxAuditReport {
                    available: true,
                    source: path.display().to_string(),
                    events,
                    avc_count,
                });
            }
        }
        if let Some(events) = probe_ausearch(lim) {
            let avc_count = events.iter().filter(|e| e.event_type == "AVC").count() as u32;
            return Ok(LinuxAuditReport {
                available: true,
                source: "ausearch".into(),
                events,
                avc_count,
            });
        }
        Ok(LinuxAuditReport::default())
    }
}
