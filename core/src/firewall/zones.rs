// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Zone-name → CIDR resolution for firewall profile rule sources.
//
// `FirewallProfileRule::sources` carries either literal CIDRs (e.g. `10.0.0.0/8`)
// or symbolic zone names (e.g. `admin-network`, `monitoring`, `bmc-vlan`) defined
// by the operator via `MACHINA_FIREWALL_ZONES`. Backends that enforce source
// restrictions (host iptables, K8s NetworkPolicy) resolve through here rather
// than embedding the symbolic name directly, which previously produced either
// invalid manifests (K8s `ipBlock.cidr: admin-network`) or silently unrestricted
// rules (iptables rules with no `-s` at all).

use std::collections::HashMap;

pub fn is_cidr(s: &str) -> bool {
    let Some((addr, prefix)) = s.split_once('/') else {
        return false;
    };
    if !prefix.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    addr.parse::<std::net::IpAddr>().is_ok()
}

/// Resolves a rule source to a real CIDR: pass through literal CIDRs, look up
/// symbolic zone names in `zones`. Returns `None` when the source is neither —
/// callers must decide how to degrade (skip the restriction, fall back, warn).
pub fn resolve_source_cidr(source: &str, zones: &HashMap<String, String>) -> Option<String> {
    if is_cidr(source) {
        return Some(source.to_string());
    }
    zones.get(source).cloned()
}

/// Parses `MACHINA_FIREWALL_ZONES` — a comma-separated `name=cidr` list, e.g.
/// `admin-network=10.0.0.0/8,monitoring=10.10.0.0/16,bmc-vlan=192.168.100.0/24`.
/// Malformed entries (missing `=`, non-CIDR value) are skipped rather than
/// failing startup, since a typo here shouldn't take down the controller.
pub fn parse_zone_env(raw: &str) -> HashMap<String, String> {
    let mut zones = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((name, cidr)) = entry.split_once('=') else {
            continue;
        };
        let (name, cidr) = (name.trim(), cidr.trim());
        if name.is_empty() || !is_cidr(cidr) {
            continue;
        }
        zones.insert(name.to_string(), cidr.to_string());
    }
    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_cidr_accepts_valid_cidrs() {
        assert!(is_cidr("10.0.0.0/8"));
        assert!(is_cidr("192.168.1.0/24"));
        assert!(is_cidr("::1/128"));
    }

    #[test]
    fn is_cidr_rejects_symbolic_names() {
        assert!(!is_cidr("admin-network"));
        assert!(!is_cidr("monitoring"));
        assert!(!is_cidr(""));
        assert!(!is_cidr("not/a/cidr"));
    }

    #[test]
    fn resolve_source_cidr_passes_through_literal_cidr() {
        let zones = HashMap::new();
        assert_eq!(
            resolve_source_cidr("10.0.0.0/8", &zones),
            Some("10.0.0.0/8".to_string())
        );
    }

    #[test]
    fn resolve_source_cidr_looks_up_zone_name() {
        let mut zones = HashMap::new();
        zones.insert("admin-network".to_string(), "10.0.0.0/8".to_string());
        assert_eq!(
            resolve_source_cidr("admin-network", &zones),
            Some("10.0.0.0/8".to_string())
        );
        assert_eq!(resolve_source_cidr("unknown-zone", &zones), None);
    }

    #[test]
    fn parse_zone_env_parses_valid_entries() {
        let zones = parse_zone_env("admin-network=10.0.0.0/8,monitoring=10.10.0.0/16");
        assert_eq!(zones.get("admin-network"), Some(&"10.0.0.0/8".to_string()));
        assert_eq!(zones.get("monitoring"), Some(&"10.10.0.0/16".to_string()));
        assert_eq!(zones.len(), 2);
    }

    #[test]
    fn parse_zone_env_skips_malformed_entries() {
        let zones = parse_zone_env("bad-entry,admin-network=10.0.0.0/8,no-value=,=novalue");
        assert_eq!(zones.len(), 1);
        assert_eq!(zones.get("admin-network"), Some(&"10.0.0.0/8".to_string()));
    }

    #[test]
    fn parse_zone_env_handles_empty_string() {
        assert!(parse_zone_env("").is_empty());
    }
}
