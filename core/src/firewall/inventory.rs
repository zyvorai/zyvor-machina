// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::adapters::{adapter_for, nftables};
use super::detect::detect_backend;
use super::profiles::builtin_profiles;
use super::scan::{enrich_port_exposure, ports_to_services, scan_open_ports};
use super::score::compute_firewall_score;
use super::types::FirewallInventory;
use crate::LibvirtError;

pub fn gather_firewall_inventory(hostname: &str) -> Result<FirewallInventory, LibvirtError> {
    let backend = detect_backend();
    let adapter = adapter_for(backend);
    let mut posture = adapter.read_posture()?;
    let rules = adapter.read_rules()?;
    let mut open_ports = scan_open_ports().unwrap_or_default();
    enrich_port_exposure(&mut open_ports, &rules);
    let services = ports_to_services(&open_ports);
    let score = compute_firewall_score(&posture, &rules, &open_ports);
    let profiles_available: Vec<String> = builtin_profiles().into_iter().map(|p| p.name).collect();
    let nftables_summary = if backend == crate::firewall::types::FirewallBackend::Nftables {
        nftables::ruleset_summary()
    } else {
        None
    };
    if posture.profile.is_none() {
        posture.profile = score.score.checked_sub(0).map(|_| {
            if score.score >= 80 {
                "ProductionServer".into()
            } else if score.score >= 60 {
                "Public".into()
            } else {
                "DevelopmentVm".into()
            }
        });
    }
    Ok(FirewallInventory {
        hostname: hostname.to_string(),
        posture,
        rules,
        open_ports,
        services,
        score,
        profiles_available,
        nftables_summary,
        activity: None,
    })
}
