// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! WireGuard overlay between hosts. Each host owns a fleet prefix (a `/24`
//! of the IPv4 base, a `/64` of the IPv6 base); its VMs get fleet addresses
//! from it, mapped 1:1 to their local addresses by the nftables tables
//! `ip machina_overlay` / `ip6 machina_overlay`. Traffic to another host's
//! prefix is routed into `machina-wg`, leaves with the sending VM's fleet
//! address and is encrypted; WireGuard only accepts a peer's own prefixes
//! as sources, so a host cannot claim another host's VMs.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use base64::Engine as _;

use crate::api::{VmOverlay, VmOverlayPeerStatus};

pub const IFACE: &str = "machina-wg";
pub const TABLE: &str = "machina_overlay";
pub const DEFAULT_PREFIX4: &str = "100.96.0.0/12";
pub const DEFAULT_PREFIX6: &str = "fd6d:6163:6869::/48";
pub const DEFAULT_PORT: u16 = 51871;
pub const MAX_MAPPINGS: usize = 8192;
pub const MAX_PEERS: usize = 1024;

fn net4(base: &str) -> Option<(u32, u8)> {
    let (a, l) = base.split_once('/')?;
    let a: Ipv4Addr = a.parse().ok()?;
    let l: u8 = l.parse().ok().filter(|l| *l <= 24)?;
    let mask = if l == 0 { 0 } else { u32::MAX << (32 - l) };
    Some((u32::from(a) & mask, l))
}

fn net6(base: &str) -> Option<(u128, u8)> {
    let (a, l) = base.split_once('/')?;
    let a: Ipv6Addr = a.parse().ok()?;
    let l: u8 = l.parse().ok().filter(|l| *l <= 64)?;
    let mask = if l == 0 { 0 } else { u128::MAX << (128 - l) };
    Some((u128::from(a) & mask, l))
}

/// Host prefixes a base holds: (IPv4 `/24`s, IPv6 `/64`s); index 0 is unused.
pub fn host_capacity(base4: &str, base6: &str) -> (u32, u64) {
    let c4 = net4(base4).map_or(0, |(_, l)| (1u32 << (24 - l)) - 1);
    let c6 = net6(base6).map_or(0, |(_, l)| {
        ((1u128 << (64 - l)) - 1).min(u64::MAX as u128) as u64
    });
    (c4, c6)
}

/// Host `idx`'s IPv4 prefix (`/24`).
pub fn host_prefix4(base: &str, idx: u32) -> Option<String> {
    let (n, l) = net4(base)?;
    (idx > 0 && idx < (1u32 << (24 - l))).then(|| format!("{}/24", Ipv4Addr::from(n + (idx << 8))))
}

/// Host `idx`'s IPv6 prefix (`/64`).
pub fn host_prefix6(base: &str, idx: u32) -> Option<String> {
    let (n, l) = net6(base)?;
    ((idx as u128) > 0 && (idx as u128) < (1u128 << (64 - l)))
        .then(|| format!("{}/64", Ipv6Addr::from(n + ((idx as u128) << 64))))
}

/// Address `n` in a host prefix (1 = the host, 2.. = VMs).
pub fn addr_in(prefix: &str, n: u32) -> Option<IpAddr> {
    let (a, l) = prefix.split_once('/')?;
    match a.parse::<IpAddr>().ok()? {
        IpAddr::V4(a) if l == "24" && (1..=254).contains(&n) => {
            Some(IpAddr::V4(Ipv4Addr::from(u32::from(a) + n)))
        }
        IpAddr::V6(a) if l == "64" && n >= 1 => {
            Some(IpAddr::V6(Ipv6Addr::from(u128::from(a) + n as u128)))
        }
        _ => None,
    }
}

/// VM slots in a host prefix (IPv4: .2–.254).
pub const VM_SLOTS4: u32 = 253;

fn cidr_ok(s: &str) -> bool {
    let Some((a, l)) = s.split_once('/') else {
        return false;
    };
    match a.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => l.parse::<u8>().is_ok_and(|l| l <= 32),
        Ok(IpAddr::V6(_)) => l.parse::<u8>().is_ok_and(|l| l <= 128),
        Err(_) => false,
    }
}

pub fn validate(cfg: &VmOverlay) -> Result<(), String> {
    if !cfg.enabled {
        return Ok(());
    }
    if cfg.listen_port == 0 {
        return Err("listen_port is required".into());
    }
    if cfg.mappings.len() > MAX_MAPPINGS {
        return Err(format!("at most {MAX_MAPPINGS} mappings"));
    }
    if cfg.peers.len() > MAX_PEERS {
        return Err(format!("at most {MAX_PEERS} peers"));
    }
    for p in cfg.prefixes.iter().chain(&cfg.fleet_prefixes) {
        if !cidr_ok(p) {
            return Err(format!("prefix `{p}` is not a CIDR"));
        }
    }
    for m in &cfg.mappings {
        let (Ok(l), Ok(f)) = (m.local.parse::<IpAddr>(), m.fleet.parse::<IpAddr>()) else {
            return Err(format!(
                "mapping {} → {} is not two addresses",
                m.local, m.fleet
            ));
        };
        if l.is_ipv4() != f.is_ipv4() {
            return Err(format!(
                "mapping {} → {} mixes IPv4 and IPv6",
                m.local, m.fleet
            ));
        }
    }
    for p in &cfg.peers {
        let key = base64::engine::general_purpose::STANDARD
            .decode(p.public_key.trim())
            .map_err(|_| format!("peer {}: public key is not base64", p.host))?;
        if key.len() != 32 {
            return Err(format!("peer {}: public key is not 32 bytes", p.host));
        }
        if p.endpoint.parse::<std::net::SocketAddr>().is_err() {
            return Err(format!(
                "peer {}: endpoint `{}` is not address:port",
                p.host, p.endpoint
            ));
        }
        if p.prefixes.is_empty() || !p.prefixes.iter().all(|x| cidr_ok(x)) {
            return Err(format!("peer {}: prefixes must be CIDRs", p.host));
        }
        if !p.routes.iter().all(|x| cidr_ok(x)) {
            return Err(format!("peer {}: routes must be CIDRs", p.host));
        }
    }
    for r in &cfg.direct_routes {
        if !cidr_ok(&r.source) || !cidr_ok(&r.destination) {
            return Err(format!("direct route {} -> {} must use CIDRs", r.source, r.destination));
        }
        let sf = r.source.split('/').next().and_then(|s| s.parse::<IpAddr>().ok());
        let df = r.destination.split('/').next().and_then(|s| s.parse::<IpAddr>().ok());
        if sf.zip(df).is_none_or(|(s, d)| s.is_ipv4() != d.is_ipv4()) {
            return Err(format!("direct route {} -> {} mixes address families", r.source, r.destination));
        }
    }
    Ok(())
}

fn table(family: &str, maps: &[(IpAddr, IpAddr)]) -> String {
    let (ty, kw) = if family == "ip6" {
        ("ipv6_addr", "ip6")
    } else {
        ("ipv4_addr", "ip")
    };
    let el = |f: &dyn Fn(&(IpAddr, IpAddr)) -> String| {
        if maps.is_empty() {
            return String::new();
        }
        let e: Vec<String> = maps.iter().map(f).collect();
        format!("\n    elements = {{ {} }}", e.join(", "))
    };
    format!(
        "table {family} {TABLE} {{
  map to_vm {{
    type {ty} : {ty}{dn}
  }}
  map to_fleet {{
    type {ty} : {ty}{sn}
  }}
  chain pre {{
    type nat hook prerouting priority dstnat - 1; policy accept;
    iifname \"{IFACE}\" dnat to {kw} daddr map @to_vm
  }}
  chain post {{
    type nat hook postrouting priority srcnat - 2; policy accept;
    oifname \"{IFACE}\" snat to {kw} saddr map @to_fleet
  }}
  chain guard {{
    type filter hook forward priority mangle; policy accept;
    iifname \"{IFACE}\" ct state new ct status & dnat == 0 drop
    oifname \"{IFACE}\" tcp flags syn / syn,rst tcp option maxseg size set rt mtu
    iifname \"{IFACE}\" tcp flags syn / syn,rst tcp option maxseg size set rt mtu
  }}
}}
",
        dn = el(&|(l, f)| format!("{f} : {l}")),
        sn = el(&|(l, f)| format!("{l} : {f}")),
    )
}

/// The nftables script (`None` when off). Both families are always
/// rendered, so new connections from peers are dropped unless mapped.
pub fn render(cfg: &VmOverlay) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let mut m4 = Vec::new();
    let mut m6 = Vec::new();
    for m in &cfg.mappings {
        if let (Ok(l), Ok(f)) = (m.local.parse::<IpAddr>(), m.fleet.parse::<IpAddr>()) {
            if l.is_ipv4() && f.is_ipv4() {
                m4.push((l, f));
            } else if l.is_ipv6() && f.is_ipv6() {
                m6.push((l, f));
            }
        }
    }
    let mut v4 = table("ip", &m4);
    let mut v6 = table("ip6", &m6);
    // The normal guard accepts only DNATed fleet-address traffic. Direct VPC
    // routing is an explicit exception, scoped to configured source/destination
    // CIDRs and still subject to NACL/host FORWARD policy.
    let marker = format!("    iifname \"{IFACE}\" ct state new ct status & dnat == 0 drop\n");
    let mut a4 = String::new();
    let mut a6 = String::new();
    for r in &cfg.direct_routes {
        let ipv6 = r.source.contains(':');
        let kw = if ipv6 { "ip6" } else { "ip" };
        let line = format!(
            "    iifname \"{IFACE}\" {kw} saddr {} {kw} daddr {} accept\n",
            r.source, r.destination
        );
        if ipv6 { a6.push_str(&line); } else { a4.push_str(&line); }
    }
    if !a4.is_empty() { v4 = v4.replacen(&marker, &(a4 + &marker), 1); }
    if !a6.is_empty() { v6 = v6.replacen(&marker, &(a6 + &marker), 1); }
    Some(v4 + &v6)
}

/// Drop the old tables (created first so the deletes cannot fail), add the new.
pub fn transaction(script: Option<&str>) -> String {
    format!(
        "table ip {TABLE} {{}}\ndelete table ip {TABLE}\ntable ip6 {TABLE} {{}}\ndelete table ip6 {TABLE}\n{}",
        script.unwrap_or("")
    )
}

/// A WireGuard private key: 32 random bytes, clamped, base64.
pub fn private_key(mut k: [u8; 32]) -> String {
    k[0] &= 248;
    k[31] = (k[31] & 127) | 64;
    base64::engine::general_purpose::STANDARD.encode(k)
}

/// Peers from `wg show IFACE dump` (the first line, which holds the
/// private key, is skipped).
pub fn parse_dump(out: &str) -> Vec<VmOverlayPeerStatus> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 8).then(|| VmOverlayPeerStatus {
                public_key: f[0].to_string(),
                host: String::new(),
                endpoint: if f[2] == "(none)" {
                    String::new()
                } else {
                    f[2].to_string()
                },
                allowed_ips: f[3]
                    .split(',')
                    .filter(|x| !x.is_empty() && *x != "(none)")
                    .map(str::to_string)
                    .collect(),
                latest_handshake: f[4].parse().unwrap_or(0),
                rx_bytes: f[5].parse().unwrap_or(0),
                tx_bytes: f[6].parse().unwrap_or(0),
            })
        })
        .collect()
}

/// Destinations of `ip route show dev IFACE`.
pub fn parse_routes(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{VmOverlayMap, VmOverlayPeer};

    fn cfg() -> VmOverlay {
        VmOverlay {
            enabled: true,
            listen_port: DEFAULT_PORT,
            prefixes: vec!["100.96.1.0/24".into(), "fd6d:6163:6869:1::/64".into()],
            fleet_prefixes: vec![DEFAULT_PREFIX4.into(), DEFAULT_PREFIX6.into()],
            mappings: vec![
                VmOverlayMap {
                    local: "192.168.122.5".into(),
                    fleet: "100.96.1.2".into(),
                    vm: "a".into(),
                },
                VmOverlayMap {
                    local: "fd00::5".into(),
                    fleet: "fd6d:6163:6869:1::2".into(),
                    vm: "a".into(),
                },
            ],
            peers: vec![VmOverlayPeer {
                host: "h2".into(),
                public_key: private_key([7; 32]),
                endpoint: "10.0.0.2:51871".into(),
                prefixes: vec!["100.96.2.0/24".into()],
                routes: vec!["100.96.2.0/24".into()],
            }],
            direct_routes: Vec::new(),
        }
    }

    #[test]
    fn allocation() {
        assert_eq!(host_capacity(DEFAULT_PREFIX4, DEFAULT_PREFIX6).0, 4095);
        assert_eq!(
            host_prefix4(DEFAULT_PREFIX4, 1).as_deref(),
            Some("100.96.1.0/24")
        );
        assert_eq!(
            host_prefix4(DEFAULT_PREFIX4, 256).as_deref(),
            Some("100.97.0.0/24")
        );
        assert_eq!(host_prefix4(DEFAULT_PREFIX4, 0), None);
        assert_eq!(host_prefix4(DEFAULT_PREFIX4, 4096), None);
        assert_eq!(
            host_prefix6(DEFAULT_PREFIX6, 3).as_deref(),
            Some("fd6d:6163:6869:3::/64")
        );
        assert_eq!(
            addr_in("100.96.1.0/24", 1).unwrap().to_string(),
            "100.96.1.1"
        );
        assert_eq!(addr_in("100.96.1.0/24", 255), None);
        assert_eq!(
            addr_in("fd6d:6163:6869:3::/64", 9).unwrap().to_string(),
            "fd6d:6163:6869:3::9"
        );
    }

    #[test]
    fn renders_both_families_and_validates() {
        let c = cfg();
        assert!(validate(&c).is_ok());
        let s = render(&c).unwrap();
        assert!(s.contains("table ip machina_overlay"), "{s}");
        assert!(
            s.contains("elements = { 100.96.1.2 : 192.168.122.5 }"),
            "{s}"
        );
        assert!(
            s.contains("elements = { 192.168.122.5 : 100.96.1.2 }"),
            "{s}"
        );
        assert!(s.contains("iifname \"machina-wg\" dnat to ip daddr map @to_vm"));
        assert!(
            s.contains("table ip6 machina_overlay") && s.contains("dnat to ip6 daddr map @to_vm")
        );
        assert!(s.contains("priority srcnat - 2"));
        assert!(s.contains("ct status & dnat == 0 drop"));
        assert!(render(&VmOverlay::default()).is_none());
        let empty = VmOverlay {
            enabled: true,
            listen_port: 1,
            ..Default::default()
        };
        let e = render(&empty).unwrap();
        assert!(
            !e.contains("elements") && e.contains("table ip6 machina_overlay"),
            "{e}"
        );
        let mut bad = cfg();
        bad.peers[0].public_key = "short".into();
        assert!(validate(&bad).is_err());
        let mut bad = cfg();
        bad.mappings[0].fleet = "fd6d::2".into();
        assert!(validate(&bad).is_err());
        let mut bad = cfg();
        bad.peers[0].endpoint = "10.0.0.2".into();
        assert!(validate(&bad).is_err());
        assert!(transaction(None).ends_with("delete table ip6 machina_overlay\n"));
    }

    #[test]
    fn keys_and_wg_dump() {
        let k = base64::engine::general_purpose::STANDARD
            .decode(private_key([0xff; 32]))
            .unwrap();
        assert_eq!((k[0], k[31]), (0xf8, 0x7f));
        let dump = "PRIV\tPUB\t51871\toff\nPEERKEY\t(none)\t10.0.0.2:51871\t100.96.2.0/24,fd6d:6163:6869:2::/64\t1700000000\t1200\t3400\t25\n";
        let p = parse_dump(dump);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].public_key, "PEERKEY");
        assert_eq!(p[0].allowed_ips.len(), 2);
        assert_eq!(
            (p[0].latest_handshake, p[0].rx_bytes, p[0].tx_bytes),
            (1700000000, 1200, 3400)
        );
        assert!(!format!("{p:?}").contains("PRIV"));
        assert_eq!(
            parse_routes("100.96.2.0/24 scope link\nfd6d::/64 metric 1024\n"),
            ["100.96.2.0/24", "fd6d::/64"]
        );
    }
}
