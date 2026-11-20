// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;
use std::process::Command;

use super::detect::find_bin;
use super::types::{ExposureRisk, FirewallRule, OpenPort};
use crate::LibvirtError;

pub fn scan_open_ports() -> Result<Vec<OpenPort>, LibvirtError> {
    if let Ok(ports) = scan_with_ss() {
        if !ports.is_empty() {
            return Ok(ports);
        }
    }
    scan_proc_net()
}

fn scan_with_ss() -> Result<Vec<OpenPort>, LibvirtError> {
    let output = Command::new(find_bin("ss"))
        .args(["-tulnp"])
        .output()
        .map_err(LibvirtError::map_op("ss -tulnp"))?;
    if !output.status.success() {
        return Err(LibvirtError::Operation("ss failed".into()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let services = load_services_map();
    let mut ports = Vec::new();
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let proto = parts[0].to_lowercase();
        let local = parts[4];
        let (bind, port) = parse_local_addr(local);
        let process = line
            .split("users:(")
            .nth(1)
            .map(|s| s.trim_end_matches(')').to_string());
        let service_name = services
            .get(&port)
            .cloned()
            .unwrap_or_else(|| guess_service(port));
        ports.push(OpenPort {
            port,
            protocol: proto,
            service_name,
            bind_address: bind,
            process,
            allowed_from: vec![],
            risk: ExposureRisk::Safe,
            evidence: vec![],
        });
    }
    Ok(ports)
}

fn scan_proc_net() -> Result<Vec<OpenPort>, LibvirtError> {
    let mut ports = Vec::new();
    for (file, proto) in [("/proc/net/tcp", "tcp"), ("/proc/net/udp", "udp")] {
        let content = std::fs::read_to_string(file)
            .map_err(|e| LibvirtError::Operation(format!("read {file}: {e}")))?;
        for line in content.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 2 {
                continue;
            }
            let local = cols[1];
            let (_, port_hex) = local.split_once(':').unwrap_or(("0", "0"));
            let port = u16::from_str_radix(port_hex, 16).unwrap_or(0);
            if port == 0 {
                continue;
            }
            ports.push(OpenPort {
                port,
                protocol: proto.into(),
                service_name: guess_service(port),
                bind_address: "0.0.0.0".into(),
                process: None,
                allowed_from: vec![],
                risk: ExposureRisk::Safe,
                evidence: vec![],
            });
        }
    }
    Ok(ports)
}

fn parse_local_addr(local: &str) -> (String, u16) {
    if let Some((host, port_str)) = local.rsplit_once(':') {
        let port = port_str.parse().unwrap_or(0);
        let bind = if host.starts_with('[') {
            host.trim_matches(&['[', ']'][..]).to_string()
        } else {
            host.to_string()
        };
        (bind, port)
    } else {
        ("*".into(), 0)
    }
}

fn load_services_map() -> HashMap<u16, String> {
    let mut map = HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/etc/services") {
        for line in content.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let name = parts[0].to_string();
            if let Some(port_proto) = parts[1].split('/').next() {
                if let Ok(port) = port_proto.parse::<u16>() {
                    map.entry(port).or_insert(name);
                }
            }
        }
    }
    map
}

fn guess_service(port: u16) -> String {
    match port {
        22 => "ssh".into(),
        80 => "http".into(),
        443 => "https".into(),
        3306 => "mysql".into(),
        5432 => "postgresql".into(),
        6379 => "redis".into(),
        9090 => "metrics".into(),
        10250 => "kubelet".into(),
        _ => format!("port-{port}"),
    }
}

pub fn enrich_port_exposure(ports: &mut [OpenPort], rules: &[FirewallRule]) {
    for port in ports.iter_mut() {
        let public_bind = port.bind_address == "0.0.0.0"
            || port.bind_address == "*"
            || port.bind_address.is_empty();
        let allowed_any = rules.iter().any(|r| {
            r.action == "allow"
                && rule_matches_port(r, port.port, &port.protocol)
                && r.sources
                    .iter()
                    .any(|s| s == "any" || s == "0.0.0.0/0" || s == "Anywhere")
        });
        port.allowed_from = rules
            .iter()
            .filter(|r| r.action == "allow" && rule_matches_port(r, port.port, &port.protocol))
            .flat_map(|r| r.sources.clone())
            .collect();
        port.allowed_from.sort();
        port.allowed_from.dedup();
        port.risk = classify_port_risk(port.port, public_bind, allowed_any);
        if port.risk == ExposureRisk::Critical {
            port.evidence.push(format!(
                "{} port {} is exposed broadly",
                port.service_name, port.port
            ));
        }
    }
}

fn rule_matches_port(rule: &FirewallRule, port: u16, proto: &str) -> bool {
    if rule.ports == "*" || rule.ports.is_empty() {
        return true;
    }
    if rule.ports.contains('/') {
        return rule.ports.starts_with(&port.to_string());
    }
    rule.ports == port.to_string() || rule.ports == proto
}

pub fn classify_port_risk(port: u16, public_bind: bool, allowed_any: bool) -> ExposureRisk {
    let sensitive = matches!(port, 22 | 3306 | 5432 | 6379 | 27017 | 9200 | 10250 | 3389);
    if sensitive && (public_bind || allowed_any) {
        ExposureRisk::Critical
    } else if public_bind && matches!(port, 9090 | 9100 | 8080) {
        ExposureRisk::Warning
    } else {
        ExposureRisk::Safe
    }
}

pub fn ports_to_services(ports: &[OpenPort]) -> Vec<super::types::AllowedService> {
    ports
        .iter()
        .map(|p| {
            let allowed_from = if p.allowed_from.is_empty() {
                if p.bind_address == "0.0.0.0" || p.bind_address == "*" {
                    "Anywhere".into()
                } else {
                    "Internal".into()
                }
            } else {
                let mut uniq: Vec<String> = Vec::new();
                for s in &p.allowed_from {
                    if !uniq.iter().any(|u| u.eq_ignore_ascii_case(s)) {
                        uniq.push(s.clone());
                    }
                }
                let any_only = uniq
                    .iter()
                    .all(|s| matches!(s.as_str(), "any" | "0.0.0.0/0" | "Anywhere" | "*"));
                if any_only {
                    "Anywhere".into()
                } else {
                    uniq.join(", ")
                }
            };
            super::types::AllowedService {
                name: p.service_name.clone(),
                port: p.port,
                protocol: p.protocol.clone(),
                allowed_from,
                status: p.risk,
                recommendation: match p.risk {
                    ExposureRisk::Critical => Some(format!(
                        "Restrict {} to application subnet or admin network only",
                        p.service_name
                    )),
                    ExposureRisk::Warning => {
                        Some("Review whether this port should be public".into())
                    }
                    ExposureRisk::Safe => None,
                },
            }
        })
        .collect()
}
