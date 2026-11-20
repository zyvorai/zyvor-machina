// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::types::{FirewallInventory, FirewallRule, OpenPort};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityCell {
    pub source: String,
    pub destination: String,
    pub port: u16,
    pub protocol: String,
    pub verdict: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityMatrix {
    pub allows: Vec<ConnectivityCell>,
    pub blocks: Vec<ConnectivityCell>,
    pub warnings: Vec<String>,
    pub summary: String,
}

pub fn simulate_connectivity(
    current: &FirewallInventory,
    after_rules: &[FirewallRule],
) -> ConnectivityMatrix {
    let probes = default_probes(&current.open_ports);
    let mut allows = Vec::new();
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();

    for probe in &probes {
        let verdict = evaluate_probe(probe, after_rules, &current.open_ports);
        let cell = ConnectivityCell {
            source: probe.source.clone(),
            destination: probe.destination.clone(),
            port: probe.port,
            protocol: probe.protocol.clone(),
            verdict: verdict.0.clone(),
            reason: verdict.1.clone(),
        };
        if verdict.0 == "allow" {
            allows.push(cell);
        } else {
            blocks.push(cell);
        }
        if probe.port == 5432 && verdict.0 == "allow" && probe.source == "internet" {
            warnings.push("Database reachable from internet after plan".into());
        }
    }

    let summary = format!(
        "{} allowed paths · {} blocked · {} warnings",
        allows.len(),
        blocks.len(),
        warnings.len()
    );
    ConnectivityMatrix {
        allows,
        blocks,
        warnings,
        summary,
    }
}

struct Probe {
    source: String,
    destination: String,
    port: u16,
    protocol: String,
}

fn default_probes(open_ports: &[OpenPort]) -> Vec<Probe> {
    let mut probes = vec![
        Probe {
            source: "admin-subnet".into(),
            destination: "host".into(),
            port: 22,
            protocol: "tcp".into(),
        },
        Probe {
            source: "internet".into(),
            destination: "host".into(),
            port: 22,
            protocol: "tcp".into(),
        },
        Probe {
            source: "app-tier".into(),
            destination: "host".into(),
            port: 5432,
            protocol: "tcp".into(),
        },
        Probe {
            source: "internet".into(),
            destination: "host".into(),
            port: 5432,
            protocol: "tcp".into(),
        },
        Probe {
            source: "internet".into(),
            destination: "host".into(),
            port: 443,
            protocol: "tcp".into(),
        },
    ];
    for p in open_ports.iter().take(5) {
        if !probes.iter().any(|pr| pr.port == p.port) {
            probes.push(Probe {
                source: "internet".into(),
                destination: "host".into(),
                port: p.port,
                protocol: p.protocol.clone(),
            });
        }
    }
    probes
}

fn evaluate_probe(
    probe: &Probe,
    rules: &[FirewallRule],
    open_ports: &[OpenPort],
) -> (String, String) {
    for rule in rules {
        if rule.action != "allow" || rule.direction != "inbound" {
            continue;
        }
        if !port_matches(&rule.ports, probe.port) {
            continue;
        }
        if source_matches(&rule.sources, &probe.source) {
            return (
                "allow".into(),
                format!("Rule {} permits {}:{}", rule.id, probe.protocol, probe.port),
            );
        }
    }
    if open_ports
        .iter()
        .any(|p| p.port == probe.port && p.risk == super::types::ExposureRisk::Critical)
    {
        return (
            "block".into(),
            format!("Port {} exposed but no matching allow rule", probe.port),
        );
    }
    (
        "block".into(),
        "Default deny — no matching inbound rule".into(),
    )
}

fn port_matches(spec: &str, port: u16) -> bool {
    if spec == "*" {
        return true;
    }
    if let Ok(p) = spec.parse::<u16>() {
        return p == port;
    }
    false
}

fn source_matches(sources: &[String], probe_source: &str) -> bool {
    if sources.is_empty() {
        return true;
    }
    sources.iter().any(|s| {
        s == "any"
            || s == "0.0.0.0/0"
            || s.contains(probe_source)
            || (probe_source == "admin-subnet" && s.contains("10."))
            || (probe_source == "app-tier" && s.contains("10.0.1"))
    })
}
