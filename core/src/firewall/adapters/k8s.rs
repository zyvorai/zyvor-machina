// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{default_posture, FirewallAdapter};
use crate::firewall::detect::run_cmd;
use crate::firewall::types::{FirewallBackend, FirewallPosture, FirewallRule};
use crate::LibvirtError;

pub struct K8sNetworkPolicyAdapter;

impl FirewallAdapter for K8sNetworkPolicyAdapter {
    fn read_posture(&self) -> Result<FirewallPosture, LibvirtError> {
        let mut posture = default_posture(FirewallBackend::K8sNetworkPolicy);
        posture.status_line = Some("Kubernetes NetworkPolicy".into());
        posture.enabled = run_cmd("kubectl", &["version", "--client"]).is_ok();
        Ok(posture)
    }

    fn read_rules(&self) -> Result<Vec<FirewallRule>, LibvirtError> {
        let out = run_cmd(
            "kubectl",
            &[
                "get",
                "networkpolicy",
                "-A",
                "-o",
                "jsonpath={range .items[*]}{.metadata.namespace}/{.metadata.name}{\"\\n\"}{end}",
            ],
        )
        .unwrap_or_default();
        let mut rules = Vec::new();
        for (i, line) in out.lines().filter(|l| !l.is_empty()).enumerate() {
            rules.push(FirewallRule {
                id: format!("np-{i}"),
                direction: "inbound".into(),
                protocol: "all".into(),
                ports: "*".into(),
                sources: vec!["namespace selector".into()],
                targets: vec![line.to_string()],
                action: "allow".into(),
                temporary: false,
                expires_at: None,
                description: Some(format!("NetworkPolicy {line}")),
                scope: "kubernetes".into(),
                backend_ref: Some(line.to_string()),
            });
        }
        Ok(rules)
    }

    fn snapshot_state(&self) -> Result<serde_json::Value, LibvirtError> {
        let out = run_cmd("kubectl", &["get", "networkpolicy", "-A", "-o", "json"])
            .unwrap_or_else(|_| "{}".into());
        let parsed: serde_json::Value =
            serde_json::from_str(&out).unwrap_or_else(|_| serde_json::json!({}));
        Ok(parsed)
    }
}
