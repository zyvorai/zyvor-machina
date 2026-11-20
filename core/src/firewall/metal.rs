// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::diff::compute_diff;
use super::profiles::profile_by_name;
use super::score::compute_firewall_score;
use super::types::{
    AllowedService, ExposureRisk, FirewallBackend, FirewallInventory, FirewallPlanRequest,
    FirewallPlanResult, FirewallPosture, FirewallRule, OpenPort, StealthLevel,
};
use crate::LibvirtError;

#[derive(Debug, Clone)]
pub struct MetalServerInput {
    pub hostname: String,
    pub bmc_address: String,
    pub bmc_type: String,
    pub firewall_profile: String,
    pub firewall_enabled: bool,
    pub bmc_vlan: String,
    pub pxe_vlan: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetalExposureScan {
    pub bmc_reachable: bool,
    pub ipmi_exposed: bool,
    pub redfish_exposed: bool,
    pub risk: String,
    pub notes: Vec<String>,
    pub scanned_at: String,
}

pub fn gather_metal_inventory(server: &MetalServerInput) -> FirewallInventory {
    let open_ports = synthetic_open_ports(server);
    let services = open_ports
        .iter()
        .map(|p| AllowedService {
            name: p.service_name.clone(),
            port: p.port,
            protocol: p.protocol.clone(),
            allowed_from: p.allowed_from.join(", "),
            status: p.risk,
            recommendation: Some(metal_port_recommendation(p)),
        })
        .collect();

    let rules = profile_rules_as_firewall_rules(&server.firewall_profile);
    let posture = FirewallPosture {
        enabled: server.firewall_enabled,
        backend: FirewallBackend::Policy,
        profile: Some(server.firewall_profile.clone()),
        stealth_level: StealthLevel::Standard,
        default_inbound: Some("deny".into()),
        default_outbound: Some("allow".into()),
        backend_zone: server.bmc_vlan.clone().into(),
        status_line: Some(format!(
            "BMC {} · PXE VLAN {} · policy-only (no live BMC ACL)",
            server.bmc_vlan, server.pxe_vlan
        )),
        drift_detected: false,
        last_changed: None,
    };

    FirewallInventory {
        hostname: server.hostname.clone(),
        posture: posture.clone(),
        rules: rules.clone(),
        open_ports: open_ports.clone(),
        services,
        score: compute_firewall_score(&posture, &rules, &open_ports),
        profiles_available: super::profiles::builtin_profiles()
            .into_iter()
            .filter(|p| {
                p.name.starts_with("BareMetal")
                    || p.name.starts_with("Metal")
                    || p.name == "ProvisioningDenyAll"
            })
            .map(|p| p.name)
            .collect(),
        nftables_summary: None,
        activity: None,
    }
}

pub fn scan_ipmi_exposure(bmc_address: &str, bmc_type: &str) -> MetalExposureScan {
    let mut notes = Vec::new();
    let mut ipmi_exposed = false;
    let mut redfish_exposed = false;
    let bmc_reachable = if bmc_address.trim().is_empty() {
        notes.push("No BMC address configured".into());
        false
    } else {
        let ipmi = probe_tcp(bmc_address, 623);
        let redfish = probe_tcp(bmc_address, 443);
        ipmi_exposed = ipmi;
        redfish_exposed = redfish;
        if ipmi {
            notes.push("IPMI port 623/tcp responded (exposure risk if not VLAN-restricted)".into());
        }
        if redfish {
            notes.push(format!(
                "Redfish HTTPS responded on {} ({bmc_type})",
                bmc_address
            ));
        }
        ipmi || redfish
    };

    let risk = if bmc_address.trim().is_empty() || is_public_bmc(bmc_address) {
        "critical"
    } else if ipmi_exposed {
        "warning"
    } else {
        "low"
    };

    MetalExposureScan {
        bmc_reachable,
        ipmi_exposed,
        redfish_exposed,
        risk: risk.into(),
        notes,
        scanned_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub fn compile_metal_plan(
    server: &MetalServerInput,
    req: &FirewallPlanRequest,
) -> Result<FirewallPlanResult, LibvirtError> {
    let current = gather_metal_inventory(server);
    let enabled = req.enable.unwrap_or(true);

    if !enabled {
        let diff = compute_diff(&current.rules, &[]);
        return Ok(FirewallPlanResult {
            diff,
            operations: vec![
                "zeus-metal:disable".into(),
                format!("zeus-metal:profile:{}", server.firewall_profile),
            ],
        });
    }

    let profile_name = req
        .profile
        .clone()
        .or_else(|| req.preset.clone())
        .unwrap_or_else(|| server.firewall_profile.clone());
    let profile = profile_by_name(&profile_name)
        .ok_or_else(|| LibvirtError::Invalid(format!("Unknown profile: {profile_name}")))?;

    let after_rules = profile_rules_as_firewall_rules(&profile.name);
    let mut operations = vec![
        format!("zeus-metal:apply-profile:{}", profile.name),
        format!("zeus-metal:bmc-vlan:{}", server.bmc_vlan),
        format!("zeus-metal:pxe-vlan:{}", server.pxe_vlan),
    ];
    if let Some(stealth) = req.stealth_level {
        operations.push(format!("zeus-metal:stealth:{stealth:?}"));
    }
    if profile.name == "MetalLockdown" || profile.name == "EmergencyIsolation" {
        operations.push("zeus-metal:lockdown".into());
    }

    let diff = compute_diff(&current.rules, &after_rules);
    Ok(FirewallPlanResult { diff, operations })
}

fn synthetic_open_ports(server: &MetalServerInput) -> Vec<OpenPort> {
    let mut ports = Vec::new();
    if !server.bmc_address.is_empty() {
        let bmc_risk = if is_public_bmc(&server.bmc_address) {
            ExposureRisk::Critical
        } else if server.bmc_vlan.is_empty() {
            ExposureRisk::Warning
        } else {
            ExposureRisk::Safe
        };
        ports.push(OpenPort {
            port: 623,
            protocol: "tcp".into(),
            service_name: "IPMI".into(),
            bind_address: server.bmc_address.clone(),
            process: Some("bmc".into()),
            allowed_from: if server.bmc_vlan.is_empty() {
                vec!["any".into()]
            } else {
                vec![server.bmc_vlan.clone()]
            },
            risk: bmc_risk,
            evidence: vec!["Synthetic BMC exposure model".into()],
        });
        ports.push(OpenPort {
            port: 443,
            protocol: "tcp".into(),
            service_name: "Redfish".into(),
            bind_address: server.bmc_address.clone(),
            process: Some(server.bmc_type.clone()),
            allowed_from: if server.bmc_vlan.is_empty() {
                vec!["any".into()]
            } else {
                vec![server.bmc_vlan.clone()]
            },
            risk: bmc_risk,
            evidence: vec!["Synthetic Redfish HTTPS".into()],
        });
    }

    let pxe_profiles = ["BareMetalPxe", "ProvisioningDenyAll"];
    if pxe_profiles.contains(&server.firewall_profile.as_str()) || !server.pxe_vlan.is_empty() {
        let pxe_risk = if server.pxe_vlan.is_empty() {
            ExposureRisk::Warning
        } else {
            ExposureRisk::Safe
        };
        for (port, name) in [(67, "DHCP"), (69, "TFTP"), (4011, "PXE-HTTP")] {
            ports.push(OpenPort {
                port,
                protocol: if port == 69 { "udp" } else { "tcp" }.into(),
                service_name: name.into(),
                bind_address: "0.0.0.0".into(),
                process: Some("pxe".into()),
                allowed_from: if server.pxe_vlan.is_empty() {
                    vec!["any".into()]
                } else {
                    vec![server.pxe_vlan.clone()]
                },
                risk: pxe_risk,
                evidence: vec!["PXE provisioning segment".into()],
            });
        }
    }
    ports
}

fn profile_rules_as_firewall_rules(profile_name: &str) -> Vec<FirewallRule> {
    profile_by_name(profile_name)
        .map(|p| {
            p.rules
                .iter()
                .enumerate()
                .map(|(i, r)| FirewallRule {
                    id: format!("metal-{i}"),
                    direction: r.direction.clone(),
                    protocol: r.protocol.clone(),
                    ports: r.ports.clone(),
                    sources: r.sources.clone(),
                    targets: vec![],
                    action: r.action.clone(),
                    temporary: false,
                    expires_at: None,
                    description: Some(r.name.clone()),
                    scope: "bare_metal".into(),
                    backend_ref: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_public_bmc(addr: &str) -> bool {
    let a = addr.trim();
    // Empty / listen-all → treat as exposed (unknown or all-interfaces).
    if a.is_empty() || a == "0.0.0.0" {
        return true;
    }
    // Public = a real IPv4 literal NOT in a private / loopback / link-local range.
    // Parse to Ipv4Addr rather than string-prefix matching: the old
    // `!a.contains('.')` clause was dead (every IPv4 has dots, and so do
    // hostnames), so any public BMC outside 203.*/185.* was misclassified as
    // private (fail-open). std's predicates cover 10/8, 172.16/12, 192.168/16,
    // 127/8, 169.254/16 precisely. A hostname / non-IPv4 doesn't parse → not
    // classified public here.
    match a.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => !(ip.is_private() || ip.is_loopback() || ip.is_link_local()),
        Err(_) => false,
    }
}

fn probe_tcp(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    if let Ok(socket) = addr.parse::<SocketAddr>() {
        TcpStream::connect_timeout(&socket, Duration::from_millis(800)).is_ok()
    } else {
        false
    }
}

fn metal_port_recommendation(port: &OpenPort) -> String {
    match port.risk {
        ExposureRisk::Critical => "Restrict BMC to admin VLAN; disable public IPMI".into(),
        ExposureRisk::Warning => "Assign bmc_vlan and apply BareMetalBmc profile".into(),
        ExposureRisk::Safe => "Monitor drift on scheduled scans".into(),
    }
}

pub fn metal_preset_temporary_pxe() -> (i32, i32, String, i32) {
    (67, 69, "udp".into(), 1)
}

pub fn metal_preset_temporary_bmc() -> (i32, i32, String, i32) {
    (623, 443, "tcp".into(), 4)
}

#[cfg(test)]
mod bmc_tests {
    use super::is_public_bmc;

    #[test]
    fn private_ranges_are_not_public() {
        for a in [
            "10.0.0.5",
            "192.168.1.10",
            "172.16.0.1",
            "172.31.255.254",
            "127.0.0.1",
            "169.254.1.1",
        ] {
            assert!(!is_public_bmc(a), "{a} should be private");
        }
    }

    #[test]
    fn public_ipv4_is_public() {
        // The old dead `!contains('.')` clause misclassified all of these as private.
        for a in [
            "8.8.8.8",
            "1.2.3.4",
            "52.10.20.30",
            "203.0.113.5",
            "172.15.0.1",
            "172.32.0.1",
        ] {
            assert!(is_public_bmc(a), "{a} should be public");
        }
    }

    #[test]
    fn empty_and_listen_all_are_exposed() {
        assert!(is_public_bmc(""));
        assert!(is_public_bmc("0.0.0.0"));
    }

    #[test]
    fn hostname_is_not_classified_public() {
        assert!(!is_public_bmc("bmc.internal.example"));
    }
}
