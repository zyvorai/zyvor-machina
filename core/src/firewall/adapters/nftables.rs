// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::FirewallAdapter;
use crate::firewall::detect::run_cmd;
use crate::firewall::types::{FirewallBackend, FirewallPosture, FirewallRule, StealthLevel};
use crate::LibvirtError;

pub struct NftablesAdapter;

impl FirewallAdapter for NftablesAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError> {
        let ruleset = run_cmd("nft", &["list", "ruleset"]).unwrap_or_default();
        let enabled = !ruleset.trim().is_empty();
        Ok(FirewallPosture {
            enabled,
            backend: FirewallBackend::Nftables,
            profile: None,
            stealth_level: StealthLevel::Off,
            default_inbound: None,
            default_outbound: None,
            backend_zone: None,
            status_line: Some(if enabled {
                "nftables active".into()
            } else {
                "nftables empty".into()
            }),
            drift_detected: false,
            last_changed: None,
        })
    }

    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError> {
        let ruleset = run_cmd("nft", &["list", "ruleset"]).unwrap_or_default();
        let mut rules = Vec::new();
        for (i, line) in ruleset.lines().enumerate() {
            if !line.contains("accept") && !line.contains("drop") && !line.contains("reject") {
                continue;
            }
            let action = if line.contains("drop") || line.contains("reject") {
                "deny"
            } else {
                "allow"
            };
            rules.push(FirewallRule {
                id: format!("nft-{i}"),
                direction: "inbound".into(),
                protocol: "all".into(),
                ports: "*".into(),
                sources: vec!["any".into()],
                targets: vec![],
                action: action.into(),
                temporary: false,
                expires_at: None,
                description: Some(line.trim().to_string()),
                scope: "host".into(),
                backend_ref: Some(line.trim().to_string()),
            });
        }
        Ok(rules)
    }

    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError> {
        let ruleset = run_cmd("nft", &["-j", "list", "ruleset"]).unwrap_or_else(|_| "{}".into());
        let parsed: serde_json::Value = serde_json::from_str(&ruleset)
            .unwrap_or_else(|_| serde_json::json!({ "raw": ruleset }));
        Ok(parsed)
    }
}

pub fn ruleset_summary() -> Option<String> {
    let ruleset = run_cmd("nft", &["list", "ruleset"]).ok()?;
    let tables = ruleset.lines().filter(|l| l.starts_with("table ")).count();
    let chains = ruleset.lines().filter(|l| l.contains("chain ")).count();
    Some(format!("{tables} tables, {chains} chains"))
}
