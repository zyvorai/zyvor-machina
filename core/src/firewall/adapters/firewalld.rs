// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::FirewallAdapter;
use crate::firewall::detect::run_cmd;
use crate::firewall::types::{FirewallBackend, FirewallPosture, FirewallRule, StealthLevel};
use crate::LibvirtError;

pub struct FirewalldAdapter;

impl FirewallAdapter for FirewalldAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError> {
        let state = run_cmd("firewall-cmd", &["--state"]).unwrap_or_else(|_| "not running".into());
        let enabled = state.trim().eq_ignore_ascii_case("running");
        let zone = run_cmd("firewall-cmd", &["--get-default-zone"]).ok();
        let default_inbound = run_cmd("firewall-cmd", &["--get-default-zone"])
            .ok()
            .map(|z| format!("zone:{z}"));
        Ok(FirewallPosture {
            enabled,
            backend: FirewallBackend::Firewalld,
            profile: zone.clone().map(|z| map_zone_to_profile(&z)),
            stealth_level: StealthLevel::Off,
            default_inbound,
            default_outbound: Some("allow".into()),
            backend_zone: zone,
            status_line: Some(format!("firewalld {state}")),
            drift_detected: false,
            last_changed: None,
        })
    }

    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError> {
        let mut rules = Vec::new();
        let list = run_cmd("firewall-cmd", &["--list-all"]).unwrap_or_default();
        let zone =
            run_cmd("firewall-cmd", &["--get-default-zone"]).unwrap_or_else(|_| "public".into());
        for line in list.lines() {
            let line = line.trim();
            if line.starts_with("services:") {
                for svc in line.trim_start_matches("services:").split_whitespace() {
                    rules.push(FirewallRule {
                        id: format!("fwld-svc-{svc}"),
                        direction: "inbound".into(),
                        protocol: "tcp".into(),
                        ports: svc.into(),
                        sources: vec!["any".into()],
                        targets: vec![],
                        action: "allow".into(),
                        temporary: false,
                        expires_at: None,
                        description: Some(format!("firewalld service {svc}")),
                        scope: "host".into(),
                        backend_ref: Some(format!("zone={zone} service={svc}")),
                    });
                }
            }
            if line.starts_with("ports:") {
                for port in line.trim_start_matches("ports:").split_whitespace() {
                    rules.push(FirewallRule {
                        id: format!("fwld-port-{port}"),
                        direction: "inbound".into(),
                        protocol: "tcp".into(),
                        ports: port.into(),
                        sources: vec!["any".into()],
                        targets: vec![],
                        action: "allow".into(),
                        temporary: false,
                        expires_at: None,
                        description: Some(format!("firewalld port {port}")),
                        scope: "host".into(),
                        backend_ref: Some(format!("zone={zone} port={port}")),
                    });
                }
            }
        }
        let rich = run_cmd("firewall-cmd", &["--list-rich-rules"]).unwrap_or_default();
        for (i, line) in rich.lines().filter(|l| !l.is_empty()).enumerate() {
            rules.push(FirewallRule {
                id: format!("fwld-rich-{i}"),
                direction: "inbound".into(),
                protocol: "all".into(),
                ports: "*".into(),
                sources: vec!["any".into()],
                targets: vec![],
                action: if line.contains("reject") || line.contains("drop") {
                    "deny".into()
                } else {
                    "allow".into()
                },
                temporary: false,
                expires_at: None,
                description: Some(line.to_string()),
                scope: "host".into(),
                backend_ref: Some(line.to_string()),
            });
        }
        Ok(rules)
    }

    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError> {
        let all = run_cmd("firewall-cmd", &["--list-all"]).unwrap_or_default();
        Ok(serde_json::json!({ "list_all": all }))
    }
}

fn map_zone_to_profile(zone: &str) -> String {
    match zone {
        "public" => "Public".into(),
        "trusted" | "home" => "Private".into(),
        "block" => "LockedDown".into(),
        _ => zone.to_string(),
    }
}

pub fn apply_profile_ops(profile: &str, enable: bool) -> Result<Vec<String>, LibvirtError> {
    let mut ops = Vec::new();
    if enable {
        ops.push("firewall-cmd --state".into());
    }
    let zone = match profile {
        "Public" => "public",
        "Private" => "home",
        "LockedDown" | "EmergencyIsolation" => "drop",
        "WebServer" => "public",
        "DatabaseServer" => "internal",
        _ => "public",
    };
    ops.push(format!("firewall-cmd --set-default-zone={zone}"));
    if profile == "WebServer" {
        ops.push("firewall-cmd --permanent --add-service=http".into());
        ops.push("firewall-cmd --permanent --add-service=https".into());
        ops.push("firewall-cmd --reload".into());
    }
    if profile == "DatabaseServer" {
        ops.push("firewall-cmd --permanent --remove-service=ssh".into());
        ops.push("firewall-cmd --reload".into());
    }
    Ok(ops)
}
