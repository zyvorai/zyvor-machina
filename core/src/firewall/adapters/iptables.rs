// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::process::Command;

use super::FirewallAdapter;
use crate::firewall::detect::find_bin;
use crate::firewall::types::{FirewallBackend, FirewallPosture, FirewallRule, StealthLevel};
use crate::LibvirtError;

pub struct IptablesAdapter;

impl FirewallAdapter for IptablesAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError> {
        let filter = Command::new(find_bin("iptables"))
            .args(["-L", "INPUT", "-n"])
            .output()
            .map_err(LibvirtError::map_op("iptables INPUT"))?;
        let enabled = filter.status.success();
        let policy = String::from_utf8_lossy(&filter.stdout)
            .lines()
            .find(|l| l.starts_with("Chain INPUT"))
            .and_then(|l| {
                l.split('(')
                    .nth(1)
                    .and_then(|s| s.split(')').next())
                    .map(|s| s.to_string())
            });
        Ok(FirewallPosture {
            enabled,
            backend: FirewallBackend::Iptables,
            profile: None,
            stealth_level: StealthLevel::Off,
            default_inbound: policy.clone(),
            default_outbound: None,
            backend_zone: None,
            status_line: policy.map(|p| format!("iptables INPUT policy {p}")),
            drift_detected: false,
            last_changed: None,
        })
    }

    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError> {
        let mut rules = Vec::new();
        for chain in ["INPUT", "FORWARD", "OUTPUT"] {
            let output = Command::new(find_bin("iptables"))
                .args(["-L", chain, "-n", "--line-numbers", "-v"])
                .output()
                .map_err(LibvirtError::map_op("iptables list"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("Chain") || line.starts_with("num") || line.is_empty() {
                    continue;
                }
                let action = if line.contains("ACCEPT") {
                    "allow"
                } else if line.contains("DROP") || line.contains("REJECT") {
                    "deny"
                } else {
                    continue;
                };
                let proto = if line.contains(" tcp ") {
                    "tcp"
                } else if line.contains(" udp ") {
                    "udp"
                } else if line.contains(" icmp ") {
                    "icmp"
                } else {
                    "all"
                };
                let port = extract_dpt(line)
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "*".into());
                let scope = if chain == "FORWARD" && line.contains("vs-fw:") {
                    "hypervisor_forward"
                } else {
                    "host"
                };
                let desc = extract_comment(line, "vs-fw:");
                rules.push(FirewallRule {
                    id: format!("ipt-{chain}-{port}-{action}"),
                    direction: match chain {
                        "OUTPUT" => "outbound",
                        _ => "inbound",
                    }
                    .into(),
                    protocol: proto.into(),
                    ports: port,
                    sources: vec![extract_source(line)],
                    targets: vec![],
                    action: action.into(),
                    temporary: false,
                    expires_at: None,
                    description: desc,
                    scope: scope.into(),
                    backend_ref: Some(line.trim().to_string()),
                });
            }
        }
        Ok(rules)
    }

    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError> {
        let save = Command::new(find_bin("iptables-save"))
            .output()
            .map_err(LibvirtError::map_op("iptables-save"))?;
        Ok(serde_json::json!({
            "save": String::from_utf8_lossy(&save.stdout)
        }))
    }
}

fn extract_dpt(line: &str) -> Option<u16> {
    line.split_whitespace()
        .find(|t| t.starts_with("dpt:"))
        .and_then(|t| t.strip_prefix("dpt:"))
        .and_then(|p| p.parse().ok())
}

fn extract_comment(line: &str, prefix: &str) -> Option<String> {
    line.find(prefix)
        .map(|i| line[i + prefix.len()..].trim().to_string())
}

fn extract_source(line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() > 8 {
        parts[8].to_string()
    } else {
        "any".into()
    }
}
