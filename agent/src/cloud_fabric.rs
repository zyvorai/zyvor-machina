// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host dataplane for Fleet Cloud VPC routes, Internet Gateway forwarding,
//! blackhole routes and EC2-style stateless network ACLs.
//!
//! Cross-host route selection itself is carried by the existing `machina-wg`
//! WireGuard overlay. This module owns the host FORWARD rules and the NACL
//! guard. NACLs are installed as an nftables base chain at a negative priority,
//! so they run before libvirt/iptables compatibility chains and cannot be
//! bypassed by an EIP/NAT rule inserted later.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::net::IpAddr;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FWD_CHAIN: &str = "MACHINA_VPC_FWD";
pub const NFT_TABLE: &str = "machina_cloud_acl";
pub const WG_IFACE: &str = "machina-wg";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PeerRoute {
    /// Local VPC/subnet source CIDR allowed to use this route.
    pub source: String,
    /// Remote VPC/subnet CIDR. The WireGuard overlay owns the kernel route.
    pub destination: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InternetRoute {
    /// Source subnet using an attached Internet Gateway route.
    pub source: String,
    /// Usually 0.0.0.0/0, but preserve the route-table destination.
    pub destination: String,
    /// Optional explicit uplink; otherwise use the host's IPv4 default route.
    #[serde(default)]
    pub uplink: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BlackholeRoute {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AclRule {
    /// Subnet this ACL is associated with.
    pub subnet: String,
    pub egress: bool,
    pub rule_number: u32,
    /// AWS-style protocol: -1/all, tcp, udp, icmp or a protocol number.
    pub protocol: String,
    /// allow | deny
    pub action: String,
    pub cidr: String,
    #[serde(default)]
    pub port_from: Option<u16>,
    #[serde(default)]
    pub port_to: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub peer_routes: Vec<PeerRoute>,
    /// Routed VPC pairs arriving from a peer host. Kept separate from outbound
    /// routes so one-sided route tables do not accidentally create bidirectional
    /// connectivity.
    #[serde(default)]
    pub peer_ingress: Vec<PeerRoute>,
    /// Peered VPCs on this same host: forwarded between their bridges without
    /// a WireGuard hop. One-sided: each direction needs its own route.
    #[serde(default)]
    pub local_routes: Vec<PeerRoute>,
    #[serde(default)]
    pub internet_routes: Vec<InternetRoute>,
    #[serde(default)]
    pub blackholes: Vec<BlackholeRoute>,
    #[serde(default)]
    pub acls: Vec<AclRule>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct SyncResult {
    pub peer_routes: usize,
    pub peer_ingress: usize,
    pub local_routes: usize,
    pub internet_routes: usize,
    pub blackholes: usize,
    pub acl_rules: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: &'static str,
    pub args: Vec<String>,
    pub check_first: bool,
    pub ignore_error: bool,
}

fn step(program: &'static str, args: &[&str]) -> Step {
    Step {
        program,
        args: args.iter().map(|x| (*x).to_string()).collect(),
        check_first: false,
        ignore_error: false,
    }
}

fn cidr(c: &str) -> Result<(IpAddr, u8), String> {
    let (a, b) = c
        .split_once('/')
        .ok_or_else(|| format!("'{c}' is not a CIDR"))?;
    let ip: IpAddr = a.parse().map_err(|_| format!("'{c}' is not a CIDR"))?;
    let bits: u8 = b
        .parse()
        .map_err(|_| format!("'{c}' has an invalid prefix"))?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    if bits > max {
        return Err(format!("'{c}' has an invalid prefix"));
    }
    Ok((ip, bits))
}

fn v4_cidr(c: &str) -> Result<(), String> {
    match cidr(c)?.0 {
        IpAddr::V4(_) => Ok(()),
        IpAddr::V6(_) => Err(format!(
            "'{c}' is IPv6; EC2 VPC forwarding is IPv4 in this release"
        )),
    }
}

fn iface(s: &str) -> bool {
    (1..=15).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

pub fn validate(cfg: &Config) -> Result<(), String> {
    for r in cfg
        .peer_routes
        .iter()
        .chain(&cfg.peer_ingress)
        .chain(&cfg.local_routes)
    {
        v4_cidr(&r.source)?;
        v4_cidr(&r.destination)?;
        if r.destination == "0.0.0.0/0" {
            return Err("a VPC peering route may not own 0.0.0.0/0".into());
        }
    }
    for r in &cfg.internet_routes {
        v4_cidr(&r.source)?;
        v4_cidr(&r.destination)?;
        if let Some(u) = &r.uplink {
            if !iface(u) {
                return Err(format!("'{u}' is not a usable interface name"));
            }
        }
    }
    for r in &cfg.blackholes {
        v4_cidr(&r.source)?;
        v4_cidr(&r.destination)?;
    }
    for a in &cfg.acls {
        v4_cidr(&a.subnet)?;
        v4_cidr(&a.cidr)?;
        if !matches!(a.action.as_str(), "allow" | "deny") {
            return Err(format!(
                "ACL rule {} has action '{}'",
                a.rule_number, a.action
            ));
        }
        let p = a.protocol.to_ascii_lowercase();
        if p != "-1"
            && p != "all"
            && p != "tcp"
            && p != "udp"
            && p != "icmp"
            && p.parse::<u8>().is_err()
        {
            return Err(format!(
                "ACL rule {} has protocol '{}'",
                a.rule_number, a.protocol
            ));
        }
        if matches!(p.as_str(), "tcp" | "udp") {
            let from = a.port_from.unwrap_or(0);
            let to = a.port_to.unwrap_or(65535);
            if from > to {
                return Err(format!(
                    "ACL rule {} has reversed port range",
                    a.rule_number
                ));
            }
        }
    }
    Ok(())
}

pub fn default_uplink() -> Option<String> {
    let out = Command::new("ip")
        .args(["-4", "route", "show", "default"])
        .output()
        .ok()?;
    parse_default_route(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_default_route(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let f: Vec<_> = line.split_whitespace().collect();
        f.iter()
            .position(|v| *v == "dev")
            .and_then(|i| f.get(i + 1))
            .filter(|v| iface(v))
            .map(|v| (*v).to_string())
    })
}

/// iptables forwarding plan. NACLs are deliberately not here; they are an
/// earlier nftables hook rendered by [`render_nacl`].
pub fn plan_forward(cfg: &Config, default_uplink: Option<&str>) -> Result<Vec<Step>, String> {
    let mut out = vec![
        Step {
            ignore_error: true,
            ..step("iptables", &["-t", "filter", "-N", FWD_CHAIN])
        },
        step("iptables", &["-t", "filter", "-F", FWD_CHAIN]),
    ];

    // Explicit blackholes first.
    for r in &cfg.blackholes {
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.source,
                "-d",
                &r.destination,
                "-j",
                "DROP",
            ],
        ));
    }

    // Preserve the existing fleet-address overlay before looking at direct VPC
    // routes: those packets were DNATed by machina_overlay and are already
    // authenticated by WireGuard AllowedIPs.
    out.push(step(
        "iptables",
        &[
            "-t",
            "filter",
            "-A",
            FWD_CHAIN,
            "-i",
            WG_IFACE,
            "-m",
            "conntrack",
            "--ctstate",
            "DNAT",
            "-j",
            "ACCEPT",
        ],
    ));

    for r in &cfg.peer_routes {
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.source,
                "-d",
                &r.destination,
                "-o",
                WG_IFACE,
                "-j",
                "ACCEPT",
            ],
        ));
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.destination,
                "-d",
                &r.source,
                "-i",
                WG_IFACE,
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        ));
        // No other source may use this installed VPC destination route.
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-d",
                &r.destination,
                "-o",
                WG_IFACE,
                "-j",
                "DROP",
            ],
        ));
    }
    for r in &cfg.local_routes {
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.source,
                "-d",
                &r.destination,
                "-j",
                "ACCEPT",
            ],
        ));
    }
    for r in &cfg.peer_ingress {
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.source,
                "-d",
                &r.destination,
                "-i",
                WG_IFACE,
                "-j",
                "ACCEPT",
            ],
        ));
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.destination,
                "-d",
                &r.source,
                "-o",
                WG_IFACE,
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        ));
    }

    for r in &cfg.internet_routes {
        let uplink = r.uplink.as_deref().or(default_uplink).ok_or_else(|| {
            "internet route exists but the host has no IPv4 default-route interface".to_string()
        })?;
        if !iface(uplink) {
            return Err(format!("'{uplink}' is not a usable interface name"));
        }
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-s",
                &r.source,
                "-d",
                &r.destination,
                "-o",
                uplink,
                "-j",
                "ACCEPT",
            ],
        ));
        out.push(step(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                FWD_CHAIN,
                "-d",
                &r.source,
                "-i",
                uplink,
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        ));
    }

    out.push(Step {
        check_first: true,
        ..step(
            "iptables",
            &["-t", "filter", "-I", "FORWARD", "1", "-j", FWD_CHAIN],
        )
    });
    Ok(out)
}

fn proto_expr(p: &str) -> String {
    match p.to_ascii_lowercase().as_str() {
        "-1" | "all" => String::new(),
        "tcp" => " tcp".into(),
        "udp" => " udp".into(),
        "icmp" => " ip protocol icmp".into(),
        n => format!(" meta l4proto {n}"),
    }
}

fn port_expr(a: &AclRule) -> String {
    let p = a.protocol.to_ascii_lowercase();
    if !matches!(p.as_str(), "tcp" | "udp") {
        return String::new();
    }
    match (a.port_from, a.port_to) {
        (None, None) => String::new(),
        (Some(f), None) => format!(" th dport {f}"),
        (None, Some(t)) => format!(" th dport 0-{t}"),
        (Some(f), Some(t)) if f == t => format!(" th dport {f}"),
        (Some(f), Some(t)) => format!(" th dport {f}-{t}"),
    }
}

/// Render a stateless AWS-style NACL as an early nftables FORWARD hook.
/// Lowest rule number wins. Every associated subnet gets an implicit deny if
/// none of its entries matched, matching EC2 NACL semantics.
pub fn render_nacl(cfg: &Config) -> String {
    let mut by_subnet: BTreeMap<String, Vec<&AclRule>> = BTreeMap::new();
    for r in &cfg.acls {
        by_subnet.entry(r.subnet.clone()).or_default().push(r);
    }
    for rules in by_subnet.values_mut() {
        rules.sort_by_key(|r| (r.egress, r.rule_number));
    }

    let mut body = String::new();
    for (subnet, rules) in &by_subnet {
        for direction in [false, true] {
            let mut list: Vec<_> = rules
                .iter()
                .copied()
                .filter(|r| r.egress == direction)
                .collect();
            list.sort_by_key(|r| r.rule_number);
            for r in list {
                let (left, right) = if direction {
                    (format!("ip saddr {subnet}"), format!("ip daddr {}", r.cidr))
                } else {
                    (format!("ip daddr {subnet}"), format!("ip saddr {}", r.cidr))
                };
                let verdict = if r.action == "allow" {
                    "accept"
                } else {
                    "drop"
                };
                body.push_str(&format!(
                    "    {left} {right}{}{} counter {verdict} comment \"machina-nacl-{}\"\n",
                    proto_expr(&r.protocol),
                    port_expr(r),
                    r.rule_number,
                ));
            }
            // Associated NACLs are default deny after their ordered entries.
            let side = if direction { "saddr" } else { "daddr" };
            body.push_str(&format!(
                "    ip {side} {subnet} counter drop comment \"machina-nacl-default-deny\"\n"
            ));
        }
    }

    format!(
        "table inet {NFT_TABLE} {{}}\ndelete table inet {NFT_TABLE}\n\
         table inet {NFT_TABLE} {{\n  chain forward {{\n    type filter hook forward priority -250; policy accept;\n{body}  }}\n}}\n"
    )
}

fn run_step(s: &Step) -> Result<(), String> {
    fn run(program: &str, args: &[String]) -> Result<(), String> {
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("{program}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{program} {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    if s.check_first {
        let mut check = s.args.clone();
        if let Some(i) = check.iter().position(|a| a == "-I") {
            check[i] = "-C".into();
            if check.get(i + 2).is_some_and(|v| v.parse::<u32>().is_ok()) {
                check.remove(i + 2);
            }
        }
        if run(s.program, &check).is_ok() {
            return Ok(());
        }
    }
    match run(s.program, &s.args) {
        Err(_) if s.ignore_error => Ok(()),
        other => other,
    }
}

fn apply_nft(script: &str) -> Result<(), String> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("nft: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("nft stdin")?
        .write_all(script.as_bytes())
        .map_err(|e| format!("nft stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("nft: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "nft: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Apply `cloud.fabric.sync` payload `{peer_routes, internet_routes,
/// blackholes, acls}`. The WireGuard overlay supplies routes for peer
/// destinations and cryptographic source ownership; this call supplies the
/// policy/forwarding layer around it.
pub fn sync(payload: &Value) -> Result<SyncResult, String> {
    let cfg: Config = serde_json::from_value(payload.clone())
        .map_err(|e| format!("cloud.fabric.sync payload: {e}"))?;
    validate(&cfg)?;

    if !cfg.peer_routes.is_empty()
        || !cfg.peer_ingress.is_empty()
        || !cfg.local_routes.is_empty()
        || !cfg.internet_routes.is_empty()
    {
        std::fs::write("/proc/sys/net/ipv4/ip_forward", "1")
            .map_err(|e| format!("enable IPv4 forwarding: {e}"))?;
    }

    apply_nft(&render_nacl(&cfg))?;
    let uplink = default_uplink();
    for s in plan_forward(&cfg, uplink.as_deref())? {
        run_step(&s)?;
    }

    Ok(SyncResult {
        peer_routes: cfg.peer_routes.len(),
        peer_ingress: cfg.peer_ingress.len(),
        local_routes: cfg.local_routes.len(),
        internet_routes: cfg.internet_routes.len(),
        blackholes: cfg.blackholes.len(),
        acl_rules: cfg.acls.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            peer_routes: vec![PeerRoute {
                source: "10.10.0.0/16".into(),
                destination: "10.20.0.0/16".into(),
            }],
            peer_ingress: vec![PeerRoute {
                source: "10.30.0.0/16".into(),
                destination: "10.10.0.0/16".into(),
            }],
            local_routes: Vec::new(),
            internet_routes: vec![InternetRoute {
                source: "10.10.1.0/24".into(),
                destination: "0.0.0.0/0".into(),
                uplink: Some("eno1".into()),
            }],
            blackholes: vec![BlackholeRoute {
                source: "10.10.2.0/24".into(),
                destination: "10.99.0.0/16".into(),
            }],
            acls: vec![
                AclRule {
                    subnet: "10.10.1.0/24".into(),
                    egress: false,
                    rule_number: 100,
                    protocol: "tcp".into(),
                    action: "allow".into(),
                    cidr: "0.0.0.0/0".into(),
                    port_from: Some(443),
                    port_to: Some(443),
                },
                AclRule {
                    subnet: "10.10.1.0/24".into(),
                    egress: true,
                    rule_number: 100,
                    protocol: "-1".into(),
                    action: "allow".into(),
                    cidr: "0.0.0.0/0".into(),
                    port_from: None,
                    port_to: None,
                },
            ],
        }
    }

    #[test]
    fn same_host_peering_is_forwarded_between_bridges_one_sided() {
        let c = Config {
            local_routes: vec![PeerRoute {
                source: "10.211.0.0/16".into(),
                destination: "10.212.0.0/16".into(),
            }],
            ..Config::default()
        };
        validate(&c).unwrap();
        let text = plan_forward(&c, None)
            .unwrap()
            .iter()
            .map(|s| format!("{} {}", s.program, s.args.join(" ")))
            .collect::<Vec<_>>()
            .join("\n");
        // No WireGuard interface in the rule, and only the configured direction.
        assert!(text.contains("-A MACHINA_VPC_FWD -s 10.211.0.0/16 -d 10.212.0.0/16 -j ACCEPT"));
        assert!(!text.contains("-s 10.212.0.0/16 -d 10.211.0.0/16"));
        let line = text
            .lines()
            .find(|l| l.contains("-s 10.211.0.0/16 -d 10.212.0.0/16"))
            .unwrap();
        assert!(!line.contains("machina-wg"));
        let bad = Config {
            local_routes: vec![PeerRoute {
                source: "10.211.0.0/16".into(),
                destination: "0.0.0.0/0".into(),
            }],
            ..Config::default()
        };
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn validates_and_plans_cross_host_and_igw_forwarding() {
        let c = cfg();
        validate(&c).unwrap();
        let p = plan_forward(&c, Some("eno1")).unwrap();
        let text = p
            .iter()
            .map(|s| format!("{} {}", s.program, s.args.join(" ")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("-s 10.10.0.0/16 -d 10.20.0.0/16 -o machina-wg -j ACCEPT"));
        assert!(text.contains("-s 10.30.0.0/16 -d 10.10.0.0/16 -i machina-wg -j ACCEPT"));
        assert!(text.contains("--ctstate DNAT -j ACCEPT"));
        assert!(text.contains("-s 10.10.1.0/24 -d 0.0.0.0/0 -o eno1 -j ACCEPT"));
        assert!(text.contains("-s 10.10.2.0/24 -d 10.99.0.0/16 -j DROP"));
    }

    #[test]
    fn nacl_is_ordered_and_has_implicit_deny() {
        let s = render_nacl(&cfg());
        assert!(s.contains("priority -250"));
        assert!(s.contains("th dport 443"));
        assert!(s.contains("machina-nacl-100"));
        assert!(s.matches("machina-nacl-default-deny").count() >= 2);
    }

    #[test]
    fn bad_inputs_are_refused() {
        let mut c = cfg();
        c.peer_routes[0].destination = "0.0.0.0/0".into();
        assert!(validate(&c).is_err());
        let mut c = cfg();
        c.acls[0].action = "maybe".into();
        assert!(validate(&c).is_err());
        assert_eq!(
            parse_default_route("default via 10.0.0.1 dev eth0 proto dhcp\n").as_deref(),
            Some("eth0")
        );
    }
}
