// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// NSX-class overlay segments — tier taxonomy, CIDR helpers, east-west defaults.

use serde::{Deserialize, Serialize};

use crate::firewall::FirewallRule;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentTier {
    Tier0,
    Tier1,
}

impl SegmentTier {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tier0" | "t0" => Some(Self::Tier0),
            "tier1" | "t1" => Some(Self::Tier1),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tier0 => "tier0",
            Self::Tier1 => "tier1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EastWestDefault {
    Allow,
    Deny,
}

impl EastWestDefault {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "deny" | "deny_all" | "deny-all" => Self::Deny,
            _ => Self::Allow,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentSpec {
    pub name: String,
    pub tier: SegmentTier,
    pub cidr: String,
    pub east_west_default: EastWestDefault,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_profile: Option<String>,
    pub gitops_namespace: String,
}

pub fn default_segment_presets() -> Vec<SegmentSpec> {
    vec![
        SegmentSpec {
            name: "prod-tier1".into(),
            tier: SegmentTier::Tier1,
            cidr: "10.10.0.0/16".into(),
            east_west_default: EastWestDefault::Allow,
            firewall_profile: Some("ProductionServer".into()),
            gitops_namespace: "prod-segments".into(),
        },
        SegmentSpec {
            name: "dmz-tier0".into(),
            tier: SegmentTier::Tier0,
            cidr: "172.16.0.0/24".into(),
            east_west_default: EastWestDefault::Deny,
            firewall_profile: Some("WebServer".into()),
            gitops_namespace: "dmz-segments".into(),
        },
    ]
}

pub fn validate_cidr(cidr: &str) -> Result<(), String> {
    parse_ipv4_cidr(cidr).map(|_| ()).map_err(|e| e.to_string())
}

pub fn ip_from_cidr_offset(cidr: &str, offset: u32) -> Result<String, String> {
    let (base, prefix) = parse_ipv4_cidr(cidr)?;
    let host_bits = 32 - prefix;
    if host_bits == 0 {
        return Err("CIDR has no host bits".into());
    }
    // Widen to u64 so a /0 CIDR (host_bits == 32) doesn't overflow `1 << 32`.
    let max_hosts = (1u64 << host_bits).saturating_sub(2);
    if offset == 0 || u64::from(offset) > max_hosts {
        return Err(format!(
            "offset {offset} out of range for {cidr} (max {max_hosts})"
        ));
    }
    let ip = base.wrapping_add(offset);
    Ok(format_ipv4(ip))
}

pub fn segment_micro_seg_grade(
    tier: &str,
    east_west: &str,
    firewall_profile: Option<&str>,
    vm_count: usize,
) -> (String, u32) {
    let mut score: i32 = 100;
    if EastWestDefault::parse(east_west) == EastWestDefault::Deny {
        score += 5;
    } else if vm_count >= 5 {
        score -= 15;
    }
    if SegmentTier::parse(tier) == Some(SegmentTier::Tier0)
        && EastWestDefault::parse(east_west) != EastWestDefault::Deny
    {
        score -= 20;
    }
    if firewall_profile.is_none() || firewall_profile == Some("") {
        score -= 10;
    }
    score = score.clamp(0, 100);
    let grade = match score {
        90..=100 => "A",
        75..=89 => "B",
        60..=74 => "C",
        40..=59 => "D",
        _ => "F",
    };
    (grade.into(), score as u32)
}

pub fn compile_micro_segment_rules(
    segment_name: &str,
    cidr: &str,
    east_west: EastWestDefault,
    profile_name: Option<&str>,
) -> Vec<FirewallRule> {
    let mut rules = Vec::new();
    let tag = format!("segment:{segment_name}");

    rules.push(FirewallRule {
        id: format!("{tag}-intra"),
        direction: "inbound".into(),
        protocol: "tcp".into(),
        ports: "*".into(),
        sources: vec![cidr.into()],
        targets: vec![cidr.into()],
        action: if east_west == EastWestDefault::Deny {
            "deny".into()
        } else {
            "allow".into()
        },
        temporary: false,
        expires_at: None,
        description: Some(format!(
            "East-west {} within {segment_name}",
            east_west.as_str()
        )),
        scope: "segment".into(),
        backend_ref: None,
    });

    if let Some(prof) = profile_name {
        rules.push(FirewallRule {
            id: format!("{tag}-profile-{prof}"),
            direction: "inbound".into(),
            protocol: "tcp".into(),
            ports: "443".into(),
            sources: vec!["admin-subnet".into()],
            targets: vec![cidr.into()],
            action: "allow".into(),
            temporary: false,
            expires_at: None,
            description: Some(format!("Zeus profile {prof} admin path (stub)")),
            scope: "segment".into(),
            backend_ref: None,
        });
    }

    if east_west == EastWestDefault::Deny {
        rules.push(FirewallRule {
            id: format!("{tag}-deny-cross"),
            direction: "inbound".into(),
            protocol: "tcp".into(),
            ports: "*".into(),
            sources: vec!["0.0.0.0/0".into()],
            targets: vec![cidr.into()],
            action: "deny".into(),
            temporary: false,
            expires_at: None,
            description: Some("East-west deny-all preset".into()),
            scope: "segment".into(),
            backend_ref: None,
        });
    }

    rules
}

fn parse_ipv4_cidr(cidr: &str) -> Result<(u32, u8), String> {
    let (addr, prefix_s) = cidr
        .split_once('/')
        .ok_or_else(|| format!("invalid CIDR: {cidr}"))?;
    let prefix: u8 = prefix_s
        .parse()
        .map_err(|_| format!("invalid prefix in {cidr}"))?;
    if prefix > 32 {
        return Err(format!("prefix too large in {cidr}"));
    }
    let base = parse_ipv4(addr)?;
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Ok((base & mask, prefix))
}

fn parse_ipv4(s: &str) -> Result<u32, String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return Err(format!("invalid IPv4: {s}"));
    }
    let mut n: u32 = 0;
    for p in parts {
        let oct: u32 = p.parse().map_err(|_| format!("invalid octet in {s}"))?;
        if oct > 255 {
            return Err(format!("octet out of range in {s}"));
        }
        n = (n << 8) | oct;
    }
    Ok(n)
}

fn format_ipv4(n: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (n >> 24) & 0xff,
        (n >> 16) & 0xff,
        (n >> 8) & 0xff,
        n & 0xff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_from_offset_works() {
        let ip = ip_from_cidr_offset("10.10.0.0/24", 10).unwrap();
        assert_eq!(ip, "10.10.0.10");
    }

    #[test]
    fn deny_grade_is_higher() {
        let (g, _) = segment_micro_seg_grade("tier0", "deny", Some("WebServer"), 3);
        assert_eq!(g, "A");
    }
}
