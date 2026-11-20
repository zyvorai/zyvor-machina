// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-project egress addresses: VM traffic leaving the host is source-NATed
//! to a project's egress IP by the nftables tables `ip machina_egress`
//! (IPv4) and `ip6 machina_egress` (IPv6), replaced atomically on every
//! change.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::api::{VmEgressSnat, VmEgressSnatRule};

pub const TABLE: &str = "machina_egress";

/// Destinations that stay un-NATed unless the config lists its own.
pub const DEFAULT_EXCLUDE: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "169.254.0.0/16",
    "127.0.0.0/8",
    "224.0.0.0/4",
];
/// IPv6 counterpart: ULA, link-local, loopback, multicast.
pub const DEFAULT_EXCLUDE6: &[&str] = &["fc00::/7", "fe80::/10", "::1/128", "ff00::/8"];

pub const MAX_RULES: usize = 256;
pub const MAX_SOURCES: usize = 4096;

fn cidr(s: &str) -> Option<(bool, String)> {
    let (a, p) = s.split_once('/').map_or((s, None), |(a, p)| (a, Some(p)));
    let a: IpAddr = a.trim().parse().ok()?;
    let max = if a.is_ipv4() { 32 } else { 128 };
    let p: u8 = match p {
        Some(p) => p.trim().parse().ok().filter(|p| *p <= max)?,
        None => max,
    };
    Some((a.is_ipv6(), format!("{a}/{p}")))
}

fn addr(s: &str) -> Option<IpAddr> {
    s.split('/').next()?.trim().parse().ok()
}

pub fn excludes(cfg: &VmEgressSnat) -> Vec<String> {
    match &cfg.exclude {
        Some(x) => x.clone(),
        None => DEFAULT_EXCLUDE
            .iter()
            .chain(DEFAULT_EXCLUDE6)
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Reject configs that cannot be rendered at all.
pub fn validate(cfg: &VmEgressSnat) -> Result<(), String> {
    if cfg.rules.len() > MAX_RULES {
        return Err(format!("at most {MAX_RULES} egress rules"));
    }
    let sources: usize = cfg.rules.iter().map(|r| r.sources.len()).sum();
    if sources > MAX_SOURCES {
        return Err(format!("at most {MAX_SOURCES} source addresses"));
    }
    for r in &cfg.rules {
        if r.egress_ip.parse::<IpAddr>().is_err() {
            return Err(format!("egress IP `{}` is not an IP address", r.egress_ip));
        }
    }
    for x in excludes(cfg) {
        if cidr(&x).is_none() {
            return Err(format!("exclude `{x}` is not a CIDR"));
        }
    }
    Ok(())
}

fn table(family: &str, excl: &[String], rules: &[(IpAddr, Vec<IpAddr>)]) -> String {
    let (ty, kw) = if family == "ip6" {
        ("ipv6_addr", "ip6")
    } else {
        ("ipv4_addr", "ip")
    };
    let mut out = format!("table {family} {TABLE} {{\n");
    if !excl.is_empty() {
        out.push_str(&format!(
            "  set excluded {{\n    type {ty}\n    flags interval\n    elements = {{ {} }}\n  }}\n",
            excl.join(", ")
        ));
    }
    out.push_str("  chain postrouting {\n    type nat hook postrouting priority srcnat - 1; policy accept;\n");
    if !excl.is_empty() {
        out.push_str(&format!("    {kw} daddr @excluded return\n"));
    }
    for (ip, srcs) in rules {
        let list: Vec<String> = srcs.iter().map(|a| a.to_string()).collect();
        out.push_str(&format!(
            "    {kw} saddr {{ {} }} snat to {ip}\n",
            list.join(", ")
        ));
    }
    out.push_str("  }\n}\n");
    out
}

/// The nftables script for `cfg` (`None` = no table), plus what was left out:
/// egress IPs not configured on this host, sources of the other address
/// family, and sources already claimed by an earlier rule.
pub fn render(cfg: &VmEgressSnat, local: &[IpAddr]) -> (Option<String>, Vec<String>) {
    let mut skipped = Vec::new();
    let mut claimed = BTreeSet::new();
    let mut rules: Vec<(IpAddr, Vec<IpAddr>)> = Vec::new();
    for r in &cfg.rules {
        let label = if r.project.is_empty() {
            r.egress_ip.clone()
        } else {
            format!("project {}", r.project)
        };
        let Ok(ip) = r.egress_ip.parse::<IpAddr>() else {
            skipped.push(format!(
                "{label}: egress IP `{}` is not an IP address",
                r.egress_ip
            ));
            continue;
        };
        if !local.contains(&ip) {
            skipped.push(format!(
                "{label}: {ip} is not configured on this host and could not be added"
            ));
            continue;
        }
        let mut srcs = Vec::new();
        for s in &r.sources {
            match addr(s) {
                Some(a) if a.is_ipv4() != ip.is_ipv4() => skipped.push(format!(
                    "{label}: source `{s}` is not {} like {ip}",
                    if ip.is_ipv4() { "IPv4" } else { "IPv6" }
                )),
                Some(a) if claimed.insert(a) => srcs.push(a),
                Some(a) => skipped.push(format!("{label}: {a} already uses another egress IP")),
                None => skipped.push(format!("{label}: source `{s}` is not an IP address")),
            }
        }
        if !srcs.is_empty() {
            rules.push((ip, srcs));
        }
    }
    if rules.is_empty() {
        return (None, skipped);
    }
    let (mut ex4, mut ex6) = (Vec::new(), Vec::new());
    for (v6, c) in excludes(cfg).iter().filter_map(|x| cidr(x)) {
        if v6 {
            ex6.push(c);
        } else {
            ex4.push(c);
        }
    }
    let (r4, r6): (Vec<_>, Vec<_>) = rules.into_iter().partition(|(ip, _)| ip.is_ipv4());
    let mut out = String::new();
    if !r4.is_empty() {
        out.push_str(&table("ip", &ex4, &r4));
    }
    if !r6.is_empty() {
        out.push_str(&table("ip6", &ex6, &r6));
    }
    (Some(out), skipped)
}

/// The whole transaction: drop the old tables (creating them first so the
/// deletes cannot fail), then add the new ones.
pub fn transaction(script: Option<&str>) -> String {
    format!(
        "table ip {TABLE} {{}}\ndelete table ip {TABLE}\ntable ip6 {TABLE} {{}}\ndelete table ip6 {TABLE}\n{}",
        script.unwrap_or("")
    )
}

/// IPv4 and IPv6 addresses from `ip -o addr show`.
pub fn local_addrs(ip_output: &str) -> Vec<IpAddr> {
    ip_output
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            it.find(|w| *w == "inet" || *w == "inet6")?;
            addr(it.next()?)
        })
        .collect()
}

/// The egress IPs bpfd should hold (rules with an address only).
pub fn wanted_addrs(cfg: &VmEgressSnat) -> BTreeSet<IpAddr> {
    if cfg.manage_addresses == Some(false) {
        return BTreeSet::new();
    }
    cfg.rules
        .iter()
        .filter(|r| !r.sources.is_empty())
        .filter_map(|r| r.egress_ip.parse().ok())
        .collect()
}

/// `IP/32` or `IP/128`: a managed address never claims a subnet route.
pub fn host_cidr(ip: IpAddr) -> String {
    format!("{ip}/{}", if ip.is_ipv4() { 32 } else { 128 })
}

/// The interface of the first default route in `ip -o route show default`.
pub fn default_dev(route_output: &str) -> Option<String> {
    route_output.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        (it.next()? == "default").then_some(())?;
        it.skip_while(|w| *w != "dev").nth(1).map(str::to_string)
    })
}

/// Gratuitous ARP (a request for our own address), the 28-byte ARP payload.
pub fn garp(mac: [u8; 6], ip: Ipv4Addr) -> [u8; 28] {
    let mut b = [0u8; 28];
    b[0..2].copy_from_slice(&1u16.to_be_bytes());
    b[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
    b[4] = 6;
    b[5] = 4;
    b[6..8].copy_from_slice(&1u16.to_be_bytes());
    b[8..14].copy_from_slice(&mac);
    b[14..18].copy_from_slice(&ip.octets());
    b[24..28].copy_from_slice(&ip.octets());
    b
}

/// Unsolicited neighbour advertisement (override flag, target link-layer
/// option); the kernel fills the ICMPv6 checksum.
pub fn unsolicited_na(mac: [u8; 6], ip: Ipv6Addr) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[0] = 136;
    b[4] = 0x20;
    b[8..24].copy_from_slice(&ip.octets());
    b[24] = 2;
    b[25] = 1;
    b[26..32].copy_from_slice(&mac);
    b
}

/// One project's rule from its VMs' addresses (the egress IP's family
/// only, deduplicated).
pub fn rules_for(
    project: &str,
    egress_ip: &str,
    addresses: impl IntoIterator<Item = String>,
) -> VmEgressSnatRule {
    let v4 = egress_ip.parse::<IpAddr>().map_or(true, |a| a.is_ipv4());
    let mut sources: Vec<String> = addresses
        .into_iter()
        .filter(|a| addr(a).is_some_and(|x| x.is_ipv4() == v4))
        .map(|a| a.split('/').next().unwrap_or(&a).trim().to_string())
        .collect();
    sources.sort();
    sources.dedup();
    VmEgressSnatRule {
        project: project.to_string(),
        egress_ip: egress_ip.to_string(),
        sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(p: &str, ip: &str, src: &[&str]) -> VmEgressSnatRule {
        VmEgressSnatRule {
            project: p.into(),
            egress_ip: ip.into(),
            sources: src.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn renders_one_table_with_exclusions_first() {
        let cfg = VmEgressSnat {
            rules: vec![
                rule(
                    "shop",
                    "203.0.113.10",
                    &["192.168.122.5", "192.168.122.6/32"],
                ),
                rule(
                    "lab",
                    "203.0.113.11",
                    &["192.168.122.5", "fd00::5", "192.168.122.9"],
                ),
                rule("ghost", "198.51.100.1", &["192.168.122.7"]),
            ],
            exclude: None,
            ..Default::default()
        };
        let local = [
            "203.0.113.10".parse().unwrap(),
            "203.0.113.11".parse().unwrap(),
        ];
        let (s, skipped) = render(&cfg, &local);
        let s = s.unwrap();
        assert!(s.contains("elements = { 10.0.0.0/8, 172.16.0.0/12"), "{s}");
        let ret = s.find("@excluded return").unwrap();
        let first = s
            .find("ip saddr { 192.168.122.5, 192.168.122.6 } snat to 203.0.113.10")
            .unwrap();
        assert!(ret < first, "{s}");
        assert!(
            s.contains("ip saddr { 192.168.122.9 } snat to 203.0.113.11"),
            "{s}"
        );
        assert!(s.contains("priority srcnat - 1"));
        assert_eq!(skipped.len(), 3, "{skipped:?}");
        assert!(skipped
            .iter()
            .any(|x| x.contains("already uses another egress IP")));
        assert!(skipped
            .iter()
            .any(|x| x.contains("not configured on this host")));
        assert!(skipped.iter().any(|x| x.contains("fd00::5")));
    }

    #[test]
    fn empty_or_unusable_rules_mean_no_table() {
        let (s, _) = render(&VmEgressSnat::default(), &[]);
        assert!(s.is_none());
        let t = transaction(None);
        assert_eq!(
            t,
            "table ip machina_egress {}\ndelete table ip machina_egress\ntable ip6 machina_egress {}\ndelete table ip6 machina_egress\n"
        );
        let cfg = VmEgressSnat {
            rules: vec![rule("x", "203.0.113.10", &["192.168.122.5"])],
            exclude: Some(vec![]),
            ..Default::default()
        };
        let (s, skipped) = render(&cfg, &[]);
        assert!(s.is_none() && skipped.len() == 1);
        let (s, _) = render(&cfg, &["203.0.113.10".parse().unwrap()]);
        assert!(!s.unwrap().contains("excluded"));
    }

    #[test]
    fn validation_and_ip_parsing() {
        let bad = VmEgressSnat {
            rules: vec![rule("x", "not-an-ip", &[])],
            exclude: None,
            ..Default::default()
        };
        assert!(validate(&bad).is_err());
        let bad = VmEgressSnat {
            rules: vec![],
            exclude: Some(vec!["10.0.0.0/33".into()]),
            ..Default::default()
        };
        assert!(validate(&bad).is_err());
        let out = "2: eth0    inet 203.0.113.10/24 brd 203.0.113.255 scope global eth0\\       valid_lft forever\n3: virbr0    inet 192.168.122.1/24 scope global virbr0\n2: eth0    inet6 2001:db8::10/64 scope global\n";
        assert_eq!(
            local_addrs(out),
            vec![
                "203.0.113.10".parse::<IpAddr>().unwrap(),
                "192.168.122.1".parse().unwrap(),
                "2001:db8::10".parse().unwrap(),
            ]
        );
        let r = rules_for(
            "p",
            "1.2.3.4",
            ["10.0.0.2".into(), "fd00::1".into(), "10.0.0.2".into()],
        );
        assert_eq!(r.sources, ["10.0.0.2"]);
    }

    #[test]
    fn managed_addresses_and_announcements() {
        let mut cfg = VmEgressSnat {
            rules: vec![
                rule("a", "203.0.113.10", &["192.168.122.5"]),
                rule("b", "2001:db8::10", &["fd00::5"]),
                rule("idle", "203.0.113.99", &[]),
            ],
            ..Default::default()
        };
        let w: Vec<String> = wanted_addrs(&cfg).iter().map(|a| host_cidr(*a)).collect();
        assert_eq!(w, ["203.0.113.10/32", "2001:db8::10/128"]);
        cfg.manage_addresses = Some(false);
        assert!(wanted_addrs(&cfg).is_empty());
        let r = "default via 80.79.5.1 dev eno8303 proto static\n10.0.0.0/8 dev x\n";
        assert_eq!(default_dev(r).as_deref(), Some("eno8303"));
        assert_eq!(default_dev("10.0.0.0/8 dev x"), None);
        let mac = [0x52, 0x54, 0, 1, 2, 3];
        let a = garp(mac, "203.0.113.10".parse().unwrap());
        assert_eq!(&a[0..8], &[0, 1, 8, 0, 6, 4, 0, 1]);
        assert_eq!(&a[8..14], &mac);
        assert_eq!(&a[14..18], &a[24..28]);
        assert_eq!(&a[18..24], &[0; 6]);
        let n = unsolicited_na(mac, "2001:db8::10".parse().unwrap());
        assert_eq!((n[0], n[4], n[24], n[25]), (136, 0x20, 2, 1));
        assert_eq!(&n[26..32], &mac);
    }

    #[test]
    fn ipv6_egress_gets_its_own_table() {
        let cfg = VmEgressSnat {
            rules: vec![
                rule("shop", "203.0.113.10", &["192.168.122.5", "fd00::5"]),
                rule("shop", "2001:db8::10", &["fd00::5", "192.168.122.5"]),
            ],
            exclude: None,
            ..Default::default()
        };
        let local = [
            "203.0.113.10".parse().unwrap(),
            "2001:db8::10".parse().unwrap(),
        ];
        let (s, skipped) = render(&cfg, &local);
        let s = s.unwrap();
        let v4 = s.find("table ip machina_egress").unwrap();
        let v6 = s.find("table ip6 machina_egress").unwrap();
        assert!(v4 < v6, "{s}");
        assert!(
            s.contains("ip saddr { 192.168.122.5 } snat to 203.0.113.10"),
            "{s}"
        );
        assert!(
            s.contains("ip6 saddr { fd00::5 } snat to 2001:db8::10"),
            "{s}"
        );
        assert!(s.contains("ip6 daddr @excluded return"), "{s}");
        assert!(s[v6..].contains("type ipv6_addr") && s[v6..].contains("fc00::/7"));
        assert!(!s[..v6].contains("fc00::/7"), "{s}");
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        let r = rules_for(
            "p",
            "2001:db8::10",
            ["10.0.0.2".into(), "fd00::1/64".into()],
        );
        assert_eq!(r.sources, ["fd00::1"]);
        let only6 = VmEgressSnat {
            rules: vec![rule("x", "2001:db8::10", &["fd00::5"])],
            exclude: None,
            ..Default::default()
        };
        let (s, _) = render(&only6, &local);
        assert!(!s.unwrap().contains("table ip machina_egress"));
    }
}
