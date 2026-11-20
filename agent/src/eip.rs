// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Elastic IPs: a public address mapped 1:1 to an instance's private address on this host.
//!
//! For each entry the host (1) holds the address as a /32 alias on its uplink, (2) DNATs traffic to it (from outside and from
//! the host itself) to the instance, (3) SNATs the instance's traffic to non-private destinations back to it, and (4) lets
//! the DNATed connections through the FORWARD chain ahead of libvirt's own reject rules. Everything lives in three iptables
//! chains of our own that are rebuilt on every `eip.sync` (the controller pushes every 30 s), so a flush by libvirt or a
//! restart heals by itself. A removed entry loses its alias, its rules disappear with the next rebuild.

use std::net::Ipv4Addr;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

pub const CHAIN_DNAT: &str = "MACHINA_EIP_DNAT";
pub const CHAIN_SNAT: &str = "MACHINA_EIP_SNAT";
pub const CHAIN_FWD: &str = "MACHINA_EIP_FWD";
const ALIAS_LABEL_SUFFIX: &str = ":eip";
/// Destinations an instance reaches with its private address (never rewritten to the elastic address).
const PRIVATE_RANGES: [&str; 4] = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16"];

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub address: String,
    pub private_ip: String,
    pub interface: String,
}

fn valid_iface(s: &str) -> bool {
    (1..=11).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

pub fn validate(entries: &[Entry]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for e in entries {
        let a: Ipv4Addr = e.address.parse().map_err(|_| format!("'{}' is not an IPv4 address", e.address))?;
        let p: Ipv4Addr = e.private_ip.parse().map_err(|_| format!("'{}' is not an IPv4 address", e.private_ip))?;
        if a.is_loopback() || a.is_multicast() || a.is_unspecified() || a.is_broadcast() || p.is_loopback() || p.is_unspecified() {
            return Err(format!("{a} / {p} is not a usable address pair"));
        }
        if !valid_iface(&e.interface) {
            return Err(format!("'{}' is not a usable interface name", e.interface));
        }
        if !seen.insert(a) {
            return Err(format!("{a} appears twice"));
        }
    }
    Ok(())
}

/// One iptables invocation. `check_first`: `-C` the same rule and skip when present (used for the jump rules).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub args: Vec<String>,
    pub check_first: bool,
    pub ignore_error: bool,
}

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

fn step(args: &[&str]) -> Step {
    Step { args: s(args), check_first: false, ignore_error: false }
}

/// The iptables steps that make the host match `entries` (rebuild from scratch).
pub fn plan(entries: &[Entry]) -> Vec<Step> {
    let mut out = Vec::new();
    for (table, chain) in [("nat", CHAIN_DNAT), ("nat", CHAIN_SNAT), ("filter", CHAIN_FWD)] {
        out.push(Step { ignore_error: true, ..step(&["-t", table, "-N", chain]) }); // exists already: fine
        out.push(step(&["-t", table, "-F", chain]));
    }
    for e in entries {
        out.push(step(&["-t", "nat", "-A", CHAIN_DNAT, "-d", &e.address, "-j", "DNAT", "--to-destination", &e.private_ip]));
        for r in PRIVATE_RANGES {
            out.push(step(&["-t", "nat", "-A", CHAIN_SNAT, "-s", &e.private_ip, "-d", r, "-j", "RETURN"]));
        }
        out.push(step(&["-t", "nat", "-A", CHAIN_SNAT, "-s", &e.private_ip, "-j", "SNAT", "--to-source", &e.address]));
        out.push(step(&["-t", "filter", "-A", CHAIN_FWD, "-d", &e.private_ip, "-m", "conntrack", "--ctstate", "DNAT", "-j", "ACCEPT"]));
    }
    // Hook the chains in at the very top, ahead of libvirt's and Cilium's chains.
    for (table, hook, chain) in [("nat", "PREROUTING", CHAIN_DNAT), ("nat", "OUTPUT", CHAIN_DNAT), ("nat", "POSTROUTING", CHAIN_SNAT), ("filter", "FORWARD", CHAIN_FWD)] {
        out.push(Step { check_first: true, ..step(&["-t", table, "-I", hook, "1", "-j", chain]) });
    }
    out
}

fn run(args: &[String]) -> Result<(), String> {
    let out = Command::new("iptables").args(args).output().map_err(|e| format!("iptables: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("iptables {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn exec(step: &Step) -> Result<(), String> {
    if step.check_first {
        let mut check = step.args.clone();
        // `-I <hook> 1 -j chain` → `-C <hook> -j chain`
        if let Some(i) = check.iter().position(|a| a == "-I") {
            check[i] = "-C".into();
            check.remove(i + 2); // the position number
        }
        if run(&check).is_ok() {
            return Ok(());
        }
    }
    match run(&step.args) {
        Err(_) if step.ignore_error => Ok(()),
        other => other,
    }
}

/// Addresses currently held as our alias (`<iface>:eip`) per interface.
fn current_aliases() -> Vec<(String, String)> {
    let out = Command::new("ip").args(["-o", "-4", "addr", "show"]).output();
    let Ok(out) = out else { return Vec::new() };
    parse_aliases(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `ip -o -4 addr show`. In one-line mode the label is the last bare token (`... scope global eno8303:eip`), not
/// preceded by the word `label`, so match on the suffix; looking for the keyword never found an alias.
fn parse_aliases(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if !f.iter().any(|x| x.trim_end_matches('\\').ends_with(ALIAS_LABEL_SUFFIX)) {
                return None;
            }
            let addr = f.iter().position(|x| *x == "inet").and_then(|i| f.get(i + 1))?;
            Some((f.get(1)?.trim_end_matches(':').to_string(), addr.split('/').next()?.to_string()))
        })
        .collect()
}

fn sync_aliases(entries: &[Entry]) -> Result<(), String> {
    let have = current_aliases();
    for e in entries {
        if !have.iter().any(|(_, a)| *a == e.address) {
            let label = format!("{}{ALIAS_LABEL_SUFFIX}", e.interface);
            let out = Command::new("ip").args(["addr", "add", &format!("{}/32", e.address), "dev", &e.interface, "label", &label]).output().map_err(|x| format!("ip: {x}"))?;
            if !out.status.success() {
                return Err(format!("ip addr add {}: {}", e.address, String::from_utf8_lossy(&out.stderr).trim()));
            }
            // Announce it so the upstream switch learns where the address lives (best effort).
            let _ = Command::new("arping").args(["-U", "-c", "1", "-I", &e.interface, &e.address]).output();
        }
    }
    for (iface, addr) in have {
        if !entries.iter().any(|e| e.address == addr) {
            let _ = Command::new("ip").args(["addr", "del", &format!("{addr}/32"), "dev", &iface]).output();
        }
    }
    Ok(())
}

/// Apply an `eip.sync` payload `{entries:[{address, private_ip, interface}]}`.
pub fn sync(payload: &Value) -> Result<usize, String> {
    let entries: Vec<Entry> = serde_json::from_value(payload.get("entries").cloned().unwrap_or(Value::Array(vec![])))
        .map_err(|e| format!("eip.sync payload: {e}"))?;
    validate(&entries)?;
    sync_aliases(&entries)?;
    for st in plan(&entries) {
        exec(&st)?;
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_found_in_one_line_output() {
        let text = "1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever preferred_lft forever\n\
2: eno8303    inet 212.8.248.187/24 brd 212.8.248.255 scope global eno8303\\       valid_lft forever preferred_lft forever\n\
2: eno8303    inet 198.18.0.1/32 scope global eno8303:eip\\       valid_lft forever preferred_lft forever\n";
        assert_eq!(parse_aliases(text), vec![("eno8303".to_string(), "198.18.0.1".to_string())]);
        assert!(parse_aliases("").is_empty());
    }

    fn e(a: &str, p: &str) -> Entry {
        Entry { address: a.into(), private_ip: p.into(), interface: "eno1".into() }
    }

    #[test]
    fn plan_builds_dnat_snat_and_forward_rules_per_entry() {
        let steps = plan(&[e("203.0.113.5", "192.168.122.50")]);
        let has = |needle: &[&str]| steps.iter().any(|s| s.args == needle.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert!(has(&["-t", "nat", "-A", CHAIN_DNAT, "-d", "203.0.113.5", "-j", "DNAT", "--to-destination", "192.168.122.50"]));
        assert!(has(&["-t", "nat", "-A", CHAIN_SNAT, "-s", "192.168.122.50", "-j", "SNAT", "--to-source", "203.0.113.5"]));
        assert!(has(&["-t", "nat", "-A", CHAIN_SNAT, "-s", "192.168.122.50", "-d", "10.0.0.0/8", "-j", "RETURN"]));
        assert!(has(&["-t", "filter", "-A", CHAIN_FWD, "-d", "192.168.122.50", "-m", "conntrack", "--ctstate", "DNAT", "-j", "ACCEPT"]));
    }

    #[test]
    fn private_destinations_are_returned_before_the_snat() {
        let steps = plan(&[e("203.0.113.5", "192.168.122.50")]);
        let snat_pos = steps.iter().position(|s| s.args.contains(&"SNAT".to_string())).unwrap();
        let returns = steps.iter().enumerate().filter(|(_, s)| s.args.contains(&"RETURN".to_string())).map(|(i, _)| i).collect::<Vec<_>>();
        assert_eq!(returns.len(), PRIVATE_RANGES.len());
        assert!(returns.iter().all(|i| *i < snat_pos));
    }

    #[test]
    fn chains_are_flushed_first_and_hooked_in_at_the_top_last() {
        let steps = plan(&[]);
        assert_eq!(steps.iter().filter(|s| s.args.contains(&"-F".to_string())).count(), 3);
        let hooks: Vec<_> = steps.iter().filter(|s| s.check_first).collect();
        assert_eq!(hooks.len(), 4);
        assert!(hooks.iter().all(|s| s.args.contains(&"-I".to_string()) && s.args.contains(&"1".to_string())));
        assert!(steps.iter().take(6).all(|s| !s.check_first), "chains exist before they are hooked");
        assert!(steps.iter().filter(|s| s.args.contains(&"-N".to_string())).all(|s| s.ignore_error));
    }

    #[test]
    fn validation_refuses_bad_pairs() {
        assert!(validate(&[e("203.0.113.5", "192.168.122.50")]).is_ok());
        assert!(validate(&[e("not-an-ip", "192.168.122.50")]).is_err());
        assert!(validate(&[e("203.0.113.5", "192.168.122.50"), e("203.0.113.5", "192.168.122.51")]).is_err(), "duplicate address");
        assert!(validate(&[e("127.0.0.1", "192.168.122.50")]).is_err());
        let mut bad = e("203.0.113.5", "192.168.122.50");
        bad.interface = "eth0; rm".into();
        assert!(validate(&[bad]).is_err());
    }

    #[test]
    fn a_hook_check_is_the_same_rule_without_the_position() {
        let st = Step { check_first: true, ..step(&["-t", "nat", "-I", "PREROUTING", "1", "-j", CHAIN_DNAT]) };
        let mut check = st.args.clone();
        let i = check.iter().position(|a| a == "-I").unwrap();
        check[i] = "-C".into();
        check.remove(i + 2);
        assert_eq!(check, s(&["-t", "nat", "-C", "PREROUTING", "-j", CHAIN_DNAT]));
    }
}
