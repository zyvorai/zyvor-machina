// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::Serialize;

use crate::config::AuditLogConfig;
use crate::state::AuditEvent;

static ROTATION: OnceLock<RwLock<AuditLogConfig>> = OnceLock::new();

fn rotation_lock() -> &'static RwLock<AuditLogConfig> {
    ROTATION.get_or_init(|| RwLock::new(AuditLogConfig::default()))
}

pub fn configure_rotation(cfg: AuditLogConfig) {
    if let Some(lock) = ROTATION.get() {
        *lock.write().unwrap_or_else(|e| e.into_inner()) = cfg;
    } else {
        let _ = ROTATION.set(RwLock::new(cfg));
    }
}

fn rotation_cfg() -> AuditLogConfig {
    rotation_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn maybe_sign_audit_line(line: &str, sign: bool) -> String {
    if !sign {
        return line.to_string();
    }
    let hash = audit_line_sha256_hex(line);
    format!("sha256:{hash}\t{line}")
}

fn audit_line_sha256_hex(line: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(line.as_bytes()))
}

/// Returns true when `line` is `sha256:<hex>\\t<payload>` and the hash matches `payload`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AuditVerifyReport {
    pub total_lines: usize,
    pub signed_valid: usize,
    pub signed_invalid: usize,
    pub unsigned: usize,
    pub invalid_samples: Vec<String>,
}

/// Verify signed lines in the audit log (up to `max_lines` from the end of the file).
pub fn verify_audit_log(max_lines: usize) -> AuditVerifyReport {
    let path = audit_log_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return AuditVerifyReport::default(),
    };
    let max_lines = max_lines.clamp(1, 500_000);
    let lines: Vec<&str> = content.lines().rev().take(max_lines).collect();
    let mut report = AuditVerifyReport {
        total_lines: lines.len(),
        ..Default::default()
    };
    for line in lines.into_iter().rev() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("sha256:") {
            if verify_signed_audit_line(line) {
                report.signed_valid += 1;
            } else {
                report.signed_invalid += 1;
                if report.invalid_samples.len() < 20 {
                    report
                        .invalid_samples
                        .push(line.chars().take(200).collect());
                }
            }
        } else {
            report.unsigned += 1;
        }
    }
    report
}

pub fn verify_signed_audit_line(line: &str) -> bool {
    let Some(hex_and_rest) = line.strip_prefix("sha256:") else {
        return false;
    };
    let Some((hex, payload)) = hex_and_rest.split_once('\t') else {
        return false;
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    audit_line_sha256_hex(payload).eq_ignore_ascii_case(hex)
}

pub fn audit_log_path() -> PathBuf {
    PathBuf::from("/var/lib/machina/audit.log")
}

fn maybe_rotate_audit_log(cfg: &AuditLogConfig) {
    if cfg.max_file_mb == 0 {
        return;
    }
    let path = audit_log_path();
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    let max_bytes = cfg.max_file_mb.saturating_mul(1024 * 1024);
    if meta.len() < max_bytes {
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    let keep = cfg.rotate_keep.max(1) as usize;
    if keep > 1 {
        let oldest = parent.join(format!("audit.log.{keep}"));
        let _ = fs::remove_file(oldest);
    }
    for i in (1..=keep).rev() {
        let from = if i == 1 {
            path.clone()
        } else {
            parent.join(format!("audit.log.{}", i - 1))
        };
        let to = parent.join(format!("audit.log.{i}"));
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    tracing::info!(
        "rotated audit log (limit {} MiB, keep {})",
        cfg.max_file_mb,
        keep
    );
}

/// Neutralize the tab-delimited log format's own field/line separators so a
/// value that happens to contain a tab or newline (e.g. free-form error text,
/// or a filesystem path — both can legally contain either byte on Linux)
/// can't split into extra fields or forge an additional, unrelated-looking
/// audit line when the file is read back with `parse_audit_line`.
fn sanitize_audit_field(s: &str) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace('\t', " ").replace(['\n', '\r'], " ")
    } else {
        s.to_string()
    }
}

pub fn write_audit_event(event: &AuditEvent) {
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let timestamp = sanitize_audit_field(&event.timestamp);
    let action = sanitize_audit_field(&event.action);
    let target = sanitize_audit_field(&event.target);
    let result = sanitize_audit_field(&event.result);
    let actor = sanitize_audit_field(&event.actor);

    let line = if actor.is_empty() {
        format!("{timestamp}\t{action}\t{target}\t{result}\n")
    } else {
        format!("{timestamp}\t{action}\t{target}\t{result}\t{actor}\n")
    };
    let line = maybe_sign_audit_line(&line, rotation_cfg().sign_lines);

    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(line.as_bytes()) {
                tracing::error!(
                    "audit log write failed for {}: {e} (security event NOT persisted: action={} target={} result={})",
                    path.display(),
                    event.action,
                    event.target,
                    event.result
                );
            }
        }
        Err(e) => {
            tracing::error!(
                "failed to open audit log {}: {e} (security event NOT persisted: action={} target={} result={})",
                path.display(),
                event.action,
                event.target,
                event.result
            );
        }
    }
    maybe_rotate_audit_log(&rotation_cfg());
    crate::audit_ship::ship_audit_event(event);
}

/// Parse one stored audit line into an event. Strips the optional
/// `sha256:<hex>\t` signature prefix first — without this, a signed line's
/// leading hash column shifted every field by one (timestamp="sha256:…",
/// actor=result, …), corrupting every read/export precisely when tamper-evident
/// signing was enabled. Mirrors verify_signed_audit_line's prefix handling.
fn parse_audit_line(line: &str) -> Option<AuditEvent> {
    let line = line
        .strip_prefix("sha256:")
        .and_then(|rest| rest.split_once('\t'))
        .map(|(_hash, payload)| payload)
        .unwrap_or(line);
    let parts: Vec<&str> = line.split('\t').collect();
    match parts.len() {
        4 => Some(AuditEvent {
            timestamp: parts[0].to_string(),
            action: parts[1].to_string(),
            target: parts[2].to_string(),
            result: parts[3].to_string(),
            actor: String::new(),
        }),
        n if n >= 5 => Some(AuditEvent {
            timestamp: parts[0].to_string(),
            action: parts[1].to_string(),
            target: parts[2].to_string(),
            result: parts[3].to_string(),
            actor: parts[4].to_string(),
        }),
        _ => {
            tracing::debug!("Skipping malformed audit line: {}", line);
            None
        }
    }
}

pub fn load_audit_events(max: usize) -> Vec<AuditEvent> {
    let path = audit_log_path();
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .rev()
        .take(max)
        .filter_map(parse_audit_line)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Full audit log as newline-delimited JSON (SIEM / archival export).
pub fn export_audit_ndjson(max: usize) -> String {
    let events = load_audit_events(max);
    events
        .into_iter()
        .filter_map(|e| serde_json::to_string(&e).ok())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_audit_line_roundtrip() {
        let raw = "2026-05-26T12:00:00Z\tvm.start\tmy-vm\tok\n";
        let signed = maybe_sign_audit_line(raw, true);
        assert!(signed.starts_with("sha256:"));
        assert!(verify_signed_audit_line(&signed));
        assert!(!verify_signed_audit_line(raw));
    }

    #[test]
    fn tampered_signed_line_fails_verify() {
        let raw = "2026-05-26T12:00:00Z\tvm.stop\tmy-vm\tok\n";
        let mut signed = maybe_sign_audit_line(raw, true);
        signed = signed.replace("vm.stop", "vm.start");
        assert!(!verify_signed_audit_line(&signed));
    }

    #[test]
    fn parse_audit_line_unsigned_and_signed_agree() {
        // Unsigned, no actor.
        let e = parse_audit_line("2026-05-26T12:00:00Z\tvm.start\tmy-vm\tok").unwrap();
        assert_eq!(e.timestamp, "2026-05-26T12:00:00Z");
        assert_eq!(e.action, "vm.start");
        assert_eq!(e.target, "my-vm");
        assert_eq!(e.result, "ok");
        assert_eq!(e.actor, "");

        // With actor.
        let e = parse_audit_line("2026-05-26T12:00:00Z\tvm.start\tmy-vm\tok\talice").unwrap();
        assert_eq!(e.actor, "alice");

        // Signed line must parse to the SAME fields (prefix stripped), not shifted.
        let signed = maybe_sign_audit_line("2026-05-26T12:00:00Z\tvm.start\tmy-vm\tok\n", true);
        let e = parse_audit_line(signed.trim_end()).unwrap();
        assert_eq!(e.timestamp, "2026-05-26T12:00:00Z");
        assert_eq!(e.action, "vm.start");
        assert_eq!(e.target, "my-vm");
        assert_eq!(e.result, "ok");
    }
}
