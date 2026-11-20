// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! NAT gateway for private cloud subnets (`nat.sync`): instances in a subnet reach the outside through the host, whose uplink
//! address they are masqueraded behind. Three pieces per subnet: a MASQUERADE rule for traffic leaving the subnet, a FORWARD
//! accept for it and for the replies, ahead of libvirt's reject rules. Our own chains (`MACHINA_NAT`, `MACHINA_NAT_FWD`) are
//! rebuilt on every push, like the elastic IP chains.

use std::net::Ipv4Addr;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

pub const CHAIN_NAT: &str = "MACHINA_NAT";
pub const CHAIN_FWD: &str = "MACHINA_NAT_FWD";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub cidr: String,
}

fn valid_cidr(c: &str) -> bool {
    let Some((a, bits)) = c.split_once('/') else { return false };
    a.parse::<Ipv4Addr>().is_ok() && bits.parse::<u8>().is_ok_and(|b| (8..=30).contains(&b))
}

pub fn validate(entries: &[Entry]) -> Result<(), String> {
    for e in entries {
        if !valid_cidr(&e.cidr) {
            return Err(format!("'{}' is not an IPv4 subnet (/8 to /30)", e.cidr));
        }
    }
    Ok(())
}

pub use crate::eip::Step;

fn st(args: &[&str]) -> Step {
    Step { args: args.iter().map(|a| a.to_string()).collect(), check_first: false, ignore_error: false }
}

/// The iptables steps for `entries`, masquerading out of `uplink`.
pub fn plan(entries: &[Entry], uplink: &str) -> Vec<Step> {
    let mut out = vec![
        Step { ignore_error: true, ..st(&["-t", "nat", "-N", CHAIN_NAT]) },
        st(&["-t", "nat", "-F", CHAIN_NAT]),
        Step { ignore_error: true, ..st(&["-t", "filter", "-N", CHAIN_FWD]) },
        st(&["-t", "filter", "-F", CHAIN_FWD]),
    ];
    for e in entries {
        out.push(st(&["-t", "nat", "-A", CHAIN_NAT, "-s", &e.cidr, "!", "-d", &e.cidr, "-o", uplink, "-j", "MASQUERADE"]));
        out.push(st(&["-t", "filter", "-A", CHAIN_FWD, "-s", &e.cidr, "-j", "ACCEPT"]));
        out.push(st(&["-t", "filter", "-A", CHAIN_FWD, "-d", &e.cidr, "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED", "-j", "ACCEPT"]));
    }
    out.push(Step { check_first: true, ..st(&["-t", "nat", "-I", "POSTROUTING", "1", "-j", CHAIN_NAT]) });
    out.push(Step { check_first: true, ..st(&["-t", "filter", "-I", "FORWARD", "1", "-j", CHAIN_FWD]) });
    out
}

/// The interface of the default route (`default via … dev X`).
pub fn default_uplink() -> Option<String> {
    let out = Command::new("ip").args(["-4", "route", "show", "default"]).output().ok()?;
    parse_default_route(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_default_route(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.iter().position(|x| *x == "dev").and_then(|i| f.get(i + 1)).map(|d| d.to_string())
    })
}

/// Apply a `nat.sync` payload `{entries:[{cidr}], uplink?}`.
pub fn sync(payload: &Value) -> Result<usize, String> {
    let entries: Vec<Entry> = serde_json::from_value(payload.get("entries").cloned().unwrap_or(Value::Array(vec![]))).map_err(|e| format!("nat.sync payload: {e}"))?;
    validate(&entries)?;
    let uplink = payload.get("uplink").and_then(|v| v.as_str()).map(String::from).or_else(default_uplink).ok_or("no default route: cannot pick an uplink")?;
    if !uplink.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) || uplink.len() > 15 {
        return Err(format!("'{uplink}' is not a usable interface name"));
    }
    if !entries.is_empty() {
        // Routing is the point of a NAT gateway; libvirt's own NAT networks turn this on too.
        let _ = std::fs::write("/proc/sys/net/ipv4/ip_forward", "1");
    }
    for s in plan(&entries, &uplink) {
        crate::eip::exec(&s)?;
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(c: &str) -> Entry {
        Entry { cidr: c.into() }
    }

    #[test]
    fn plan_masquerades_and_forwards_each_subnet() {
        let steps = plan(&[e("10.250.77.0/24")], "eno1");
        let has = |needle: &[&str]| steps.iter().any(|s| s.args == needle.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert!(has(&["-t", "nat", "-A", CHAIN_NAT, "-s", "10.250.77.0/24", "!", "-d", "10.250.77.0/24", "-o", "eno1", "-j", "MASQUERADE"]));
        assert!(has(&["-t", "filter", "-A", CHAIN_FWD, "-s", "10.250.77.0/24", "-j", "ACCEPT"]));
        assert!(has(&["-t", "filter", "-A", CHAIN_FWD, "-d", "10.250.77.0/24", "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED", "-j", "ACCEPT"]));
    }

    #[test]
    fn an_empty_list_still_flushes_and_hooks() {
        let steps = plan(&[], "eno1");
        assert_eq!(steps.iter().filter(|s| s.args.contains(&"-F".to_string())).count(), 2);
        assert_eq!(steps.iter().filter(|s| s.check_first).count(), 2);
        assert!(!steps.iter().any(|s| s.args.contains(&"MASQUERADE".to_string())));
    }

    #[test]
    fn validation_and_default_route_parsing() {
        assert!(validate(&[e("10.250.77.0/24")]).is_ok());
        assert!(validate(&[e("10.250.77.0/31")]).is_err());
        assert!(validate(&[e("0.0.0.0/0")]).is_err(), "never masquerade everything");
        assert!(validate(&[e("not a cidr")]).is_err());
        assert_eq!(parse_default_route("default via 212.8.248.1 dev eno8303 proto static\n").as_deref(), Some("eno8303"));
        assert_eq!(parse_default_route(""), None);
    }
}
