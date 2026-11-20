// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// FinOps × Security — exposure cost estimation stubs (Phase 22).

use super::types::{ExposureRisk, OpenPort};

/// Estimated monthly USD per idle listening port (audit + monitoring overhead).
pub const IDLE_PORT_MONTHLY_USD: f64 = 2.50;
/// Public critical exposure (e.g. database, IPMI) monthly risk cost.
pub const PUBLIC_CRITICAL_PORT_USD: f64 = 45.0;
/// Public warning exposure monthly risk cost.
pub const PUBLIC_WARNING_PORT_USD: f64 = 12.0;
/// Per cloud security-group rule attribution stub.
pub const CLOUD_SG_RULE_MONTHLY_USD: f64 = 0.85;
/// GPU node exposure multiplier vs baseline host.
pub const GPU_EXPOSURE_MULTIPLIER: f64 = 1.8;
/// Storage node exposure multiplier vs baseline host.
pub const STORAGE_EXPOSURE_MULTIPLIER: f64 = 1.4;
/// Mission stack isolated network segment per node (monthly).
pub const MISSION_STACK_NETWORK_USD_PER_NODE: f64 = 18.0;

pub fn is_public_bind(bind: &str) -> bool {
    bind == "0.0.0.0"
        || bind == "::"
        || bind.starts_with("0.0.0.0")
        || bind.starts_with("[::]")
        || bind == "*"
}

pub fn port_monthly_cost(port: &OpenPort, idle: bool) -> f64 {
    let base = match port.risk {
        ExposureRisk::Critical => PUBLIC_CRITICAL_PORT_USD,
        ExposureRisk::Warning => PUBLIC_WARNING_PORT_USD,
        ExposureRisk::Safe if is_public_bind(&port.bind_address) => IDLE_PORT_MONTHLY_USD * 2.0,
        ExposureRisk::Safe => IDLE_PORT_MONTHLY_USD,
    };
    if idle {
        base + IDLE_PORT_MONTHLY_USD
    } else {
        base
    }
}

pub fn idle_open_port_cost(ports: &[OpenPort]) -> f64 {
    ports
        .iter()
        .filter(|p| p.risk == ExposureRisk::Safe && is_public_bind(&p.bind_address))
        .map(|p| port_monthly_cost(p, true))
        .sum()
}

pub fn exposure_chargeback_tag(team: &str, monthly_usd: f64) -> String {
    format!("chargeback:exposure:{team}:${monthly_usd:.2}")
}

pub fn public_port_finops_alert(ports: &[OpenPort]) -> Option<String> {
    let critical = ports
        .iter()
        .filter(|p| p.risk == ExposureRisk::Critical)
        .count();
    let public_warn = ports
        .iter()
        .filter(|p| p.risk == ExposureRisk::Warning && is_public_bind(&p.bind_address))
        .count();
    if critical == 0 && public_warn == 0 {
        return None;
    }
    Some(format!(
        "{critical} critical + {public_warn} public warning port(s) — est ${:.0}/mo exposure waste",
        ports
            .iter()
            .map(|p| port_monthly_cost(p, false))
            .sum::<f64>()
    ))
}

pub fn fleet_exposure_monthly(ports: &[OpenPort], profile_multiplier: f64) -> f64 {
    let base: f64 = ports.iter().map(|p| port_monthly_cost(p, false)).sum();
    base * profile_multiplier
}

pub fn cloud_sg_monthly_cost(rule_count: usize, public_rule_count: usize) -> f64 {
    let base = rule_count as f64 * CLOUD_SG_RULE_MONTHLY_USD;
    let public_premium = public_rule_count as f64 * PUBLIC_WARNING_PORT_USD * 0.15;
    base + public_premium
}

pub fn gpu_profile_exposure_cost(base: f64) -> f64 {
    base * GPU_EXPOSURE_MULTIPLIER
}

pub fn storage_profile_exposure_cost(base: f64) -> f64 {
    base * STORAGE_EXPOSURE_MULTIPLIER
}

pub fn mission_stack_network_cost(node_count: i32) -> f64 {
    node_count.max(1) as f64 * MISSION_STACK_NETWORK_USD_PER_NODE
}

pub fn profile_exposure_multiplier(profile: Option<&str>, hostname: &str) -> f64 {
    let hl = hostname.to_lowercase();
    if hl.contains("gpu") || profile.is_some_and(|p| p.contains("Gpu") || p.contains("GPU")) {
        return GPU_EXPOSURE_MULTIPLIER;
    }
    if hl.contains("storage")
        || hl.contains("ceph")
        || hl.contains("nfs")
        || profile.is_some_and(|p| p.contains("Storage"))
    {
        return STORAGE_EXPOSURE_MULTIPLIER;
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(risk: ExposureRisk, bind: &str) -> OpenPort {
        OpenPort {
            port: 22,
            protocol: "tcp".into(),
            service_name: "ssh".into(),
            bind_address: bind.into(),
            process: None,
            allowed_from: vec![],
            risk,
            evidence: vec![],
        }
    }

    #[test]
    fn critical_costs_more_than_idle() {
        let crit = port(ExposureRisk::Critical, "0.0.0.0");
        let idle = port(ExposureRisk::Safe, "0.0.0.0");
        assert!(port_monthly_cost(&crit, false) > port_monthly_cost(&idle, true));
    }

    #[test]
    fn chargeback_tag_format() {
        assert!(exposure_chargeback_tag("platform", 120.5).contains("platform"));
    }
}
