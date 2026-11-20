// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::FirewallAdapter;
use crate::firewall::detect::run_cmd;
use crate::firewall::types::{FirewallBackend, FirewallPosture, FirewallRule, StealthLevel};
use crate::LibvirtError;

pub struct UfwAdapter;

impl FirewallAdapter for UfwAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError> {
        let status = run_cmd("ufw", &["status", "verbose"]).unwrap_or_else(|_| "inactive".into());
        let enabled = status.contains("Status: active");
        let default_inbound = status
            .lines()
            .find(|l| l.contains("Default:"))
            .and_then(|l| l.split("incoming:").nth(1))
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
        let default_outbound = status
            .lines()
            .find(|l| l.contains("Default:"))
            .and_then(|l| l.split("outgoing:").nth(1))
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
        let profile = if default_inbound.as_deref() == Some("deny") {
            Some("ProductionServer".into())
        } else if enabled {
            Some("Public".into())
        } else {
            None
        };
        Ok(FirewallPosture {
            enabled,
            backend: FirewallBackend::Ufw,
            profile,
            stealth_level: StealthLevel::Off,
            default_inbound,
            default_outbound,
            backend_zone: None,
            status_line: Some(status.lines().next().unwrap_or("ufw unknown").to_string()),
            drift_detected: false,
            last_changed: None,
        })
    }

    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError> {
        let numbered = run_cmd("ufw", &["status", "numbered"]).unwrap_or_default();
        let mut rules = Vec::new();
        for line in numbered.lines() {
            if !line.contains(']') {
                continue;
            }
            let num = line
                .split(']')
                .next()
                .and_then(|s| s.trim_start_matches('[').parse::<usize>().ok())
                .unwrap_or(0);
            let action = if line.contains("ALLOW") {
                "allow"
            } else if line.contains("DENY") {
                "deny"
            } else if line.contains("REJECT") {
                "reject"
            } else if line.contains("LIMIT") {
                "limit"
            } else {
                continue;
            };
            let proto = if line.contains("tcp") {
                "tcp"
            } else if line.contains("udp") {
                "udp"
            } else {
                "all"
            };
            let port = line
                .split_whitespace()
                .find(|t| t.contains('/'))
                .map(|t| t.split('/').next().unwrap_or("*").to_string())
                .unwrap_or_else(|| "*".into());
            rules.push(FirewallRule {
                id: format!("ufw-{num}"),
                direction: if line.contains("OUT") {
                    "outbound".into()
                } else {
                    "inbound".into()
                },
                protocol: proto.into(),
                ports: port,
                sources: vec![extract_ufw_source(line)],
                targets: vec![],
                action: action.into(),
                temporary: false,
                expires_at: None,
                description: Some(line.to_string()),
                scope: "host".into(),
                backend_ref: Some(format!("ufw rule {num}")),
            });
        }
        Ok(rules)
    }

    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError> {
        let status = run_cmd("ufw", &["status", "numbered"]).unwrap_or_default();
        Ok(serde_json::json!({ "numbered": status }))
    }
}

fn extract_ufw_source(line: &str) -> String {
    if line.contains("Anywhere") {
        return "0.0.0.0/0".into();
    }
    line.split_whitespace()
        .find(|t| t.contains('.') || t.contains(':'))
        .unwrap_or("any")
        .to_string()
}

pub fn apply_profile_ops(profile: &str, enable: bool) -> Result<Vec<String>, LibvirtError> {
    let mut ops = Vec::new();
    if enable {
        ops.push("ufw --force enable".into());
    }
    match profile {
        "WebServer" => {
            ops.push("ufw default deny incoming".into());
            ops.push("ufw default allow outgoing".into());
            ops.push("ufw allow 80/tcp".into());
            ops.push("ufw allow 443/tcp".into());
        }
        "DatabaseServer" => {
            ops.push("ufw default deny incoming".into());
            ops.push("ufw allow from 10.0.0.0/8 to any port 5432 proto tcp".into());
        }
        "LockedDown" | "EmergencyIsolation" => {
            ops.push("ufw default deny incoming".into());
            ops.push("ufw default deny outgoing".into());
        }
        "Public" => {
            ops.push("ufw default deny incoming".into());
            ops.push("ufw default allow outgoing".into());
        }
        _ => {
            ops.push("ufw default deny incoming".into());
            ops.push("ufw default allow outgoing".into());
        }
    }
    Ok(ops)
}
