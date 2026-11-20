// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Wake-on-traffic for sleeping (managed-saved) VMs. A sleeping VM has no
//! tap, so the tap programs never see its traffic; instead the nftables
//! tables `inet machina_wake` (routed, port-forwarded, overlay-DNATed and
//! host-originated packets) and `bridge machina_wake` (ARP from peers on
//! the same bridge) count packets per address, and a counter that moves
//! means someone wants the VM back.

use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::api::VmWake;

pub const TABLE: &str = "machina_wake";

fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn validate(cfg: &VmWake) -> Result<(), String> {
    if cfg.entries.len() > 4096 {
        return Err("at most 4096 sleeping VMs per host".into());
    }
    let mut seen = BTreeMap::new();
    for e in &cfg.entries {
        if !name_ok(&e.vm) {
            return Err(format!("invalid vm name {:?}", e.vm));
        }
        for a in &e.addresses {
            let ip: IpAddr = a
                .parse()
                .map_err(|_| format!("{}: invalid address {a:?}", e.vm))?;
            if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
                return Err(format!("{}: unusable address {a}", e.vm));
            }
            if let Some(other) = seen.insert(ip, e.vm.clone()) {
                if other != e.vm {
                    return Err(format!("{a} is claimed by both {other} and {}", e.vm));
                }
            }
        }
    }
    Ok(())
}

/// `(vm, address)` pairs, deduplicated and sorted.
fn targets(cfg: &VmWake) -> Vec<(String, IpAddr)> {
    let mut out: Vec<(String, IpAddr)> = cfg
        .entries
        .iter()
        .flat_map(|e| {
            e.addresses
                .iter()
                .filter_map(|a| a.parse::<IpAddr>().ok())
                .map(|ip| (e.vm.clone(), ip))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn comment(vm: &str, ip: &IpAddr) -> String {
    format!("w {vm} {ip}")
}

/// Parse a rule comment back into `(vm, address)`.
pub fn parse_comment(c: &str) -> Option<(String, IpAddr)> {
    let mut it = c.split(' ');
    if it.next()? != "w" {
        return None;
    }
    let vm = it.next()?.to_string();
    let ip = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((vm, ip))
}

/// The nftables script for the wake set (`None` when nothing sleeps).
pub fn render(cfg: &VmWake) -> Option<String> {
    let t = targets(cfg);
    if t.is_empty() {
        return None;
    }
    let mut inet = String::new();
    let mut arp = String::new();
    for (vm, ip) in &t {
        let kw = if ip.is_ipv4() { "ip" } else { "ip6" };
        let c = comment(vm, ip);
        inet.push_str(&format!("    {kw} daddr {ip} counter comment \"{c}\"\n"));
        if ip.is_ipv4() {
            arp.push_str(&format!(
                "    arp operation request arp daddr ip {ip} counter comment \"{c}\"\n"
            ));
        }
    }
    let mut s = format!(
        "table inet {TABLE} {{
  chain wake_fwd {{
    type filter hook forward priority filter - 5; policy accept;
{inet}  }}
  chain wake_out {{
    type filter hook output priority filter - 5; policy accept;
{inet}  }}
}}
"
    );
    if !arp.is_empty() {
        s.push_str(&format!(
            "table bridge {TABLE} {{
  chain wake_arp {{
    type filter hook prerouting priority filter - 5; policy accept;
{arp}  }}
}}
"
        ));
    }
    Some(s)
}

/// Drop the old tables (created first so the deletes cannot fail), add the new.
pub fn transaction(script: Option<&str>) -> String {
    format!(
        "table inet {TABLE} {{}}\ndelete table inet {TABLE}\ntable bridge {TABLE} {{}}\ndelete table bridge {TABLE}\n{}",
        script.unwrap_or("")
    )
}

/// Counter key: vm, address and the hook that counted ("host", "forward", "arp").
pub type CounterKey = (String, IpAddr, &'static str);

fn via(chain: &str) -> &'static str {
    match chain {
        "wake_out" => "host",
        "wake_arp" => "arp",
        _ => "forward",
    }
}

/// Packet totals per `(vm, address, hook)` from `nft -j list table ...` output.
pub fn parse_counters(json: &str, into: &mut BTreeMap<CounterKey, u64>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    let Some(items) = v.get("nftables").and_then(|x| x.as_array()) else {
        return;
    };
    for it in items {
        let Some(rule) = it.get("rule") else {
            continue;
        };
        let Some((vm, ip)) = rule
            .get("comment")
            .and_then(|c| c.as_str())
            .and_then(parse_comment)
        else {
            continue;
        };
        let key = (
            vm,
            ip,
            via(rule.get("chain").and_then(|c| c.as_str()).unwrap_or("")),
        );
        let packets: u64 = rule
            .get("expr")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("counter"))
            .filter_map(|c| c.get("packets").and_then(|p| p.as_u64()))
            .sum();
        *into.entry(key).or_insert(0) += packets;
    }
}

/// The counters that rose since `prev`.
pub fn risen(prev: &BTreeMap<CounterKey, u64>, now: &BTreeMap<CounterKey, u64>) -> Vec<CounterKey> {
    now.iter()
        .filter(|(k, n)| **n > prev.get(*k).copied().unwrap_or(0))
        .map(|(k, _)| k.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::VmWakeEntry;

    fn cfg() -> VmWake {
        VmWake {
            entries: vec![
                VmWakeEntry {
                    vm: "web-1".into(),
                    addresses: vec!["10.0.0.5".into(), "fd00::5".into()],
                },
                VmWakeEntry {
                    vm: "db".into(),
                    addresses: vec!["10.0.0.9".into()],
                },
            ],
        }
    }

    #[test]
    fn renders_forward_output_and_arp() {
        let s = render(&cfg()).unwrap();
        assert!(s.contains("table inet machina_wake"));
        assert!(s.contains("hook forward priority filter - 5"));
        assert!(s.contains("hook output priority filter - 5"));
        assert!(s.contains("ip daddr 10.0.0.5 counter comment \"w web-1 10.0.0.5\""));
        assert!(s.contains("ip6 daddr fd00::5 counter comment \"w web-1 fd00::5\""));
        assert!(s.contains("arp operation request arp daddr ip 10.0.0.9"));
        assert!(!s.contains("arp daddr ip fd00"));
        assert!(!s.contains("chain fwd "));
        assert_eq!(s.matches("ip daddr 10.0.0.5 ").count(), 2);
        assert!(render(&VmWake::default()).is_none());
        let tx = transaction(None);
        assert!(tx.contains("delete table inet machina_wake"));
        assert!(tx.contains("delete table bridge machina_wake"));
    }

    #[test]
    fn validation() {
        assert!(validate(&cfg()).is_ok());
        let mut c = cfg();
        c.entries[0].vm = "bad name".into();
        assert!(validate(&c).is_err());
        let mut c = cfg();
        c.entries[1].addresses = vec!["10.0.0.5".into()];
        assert!(validate(&c).unwrap_err().contains("claimed by both"));
        let mut c = cfg();
        c.entries[1].addresses = vec!["127.0.0.1".into()];
        assert!(validate(&c).is_err());
        let mut c = cfg();
        c.entries[1].addresses = vec!["nope".into()];
        assert!(validate(&c).is_err());
    }

    #[test]
    fn counters_sum_and_rise() {
        let j = r#"{"nftables":[{"metainfo":{}},
          {"table":{"family":"inet","name":"machina_wake"}},
          {"rule":{"chain":"wake_fwd","comment":"w web-1 10.0.0.5","expr":[{"match":{}},{"counter":{"packets":2,"bytes":120}}]}},
          {"rule":{"chain":"wake_out","comment":"w web-1 10.0.0.5","expr":[{"counter":{"packets":1,"bytes":60}}]}},
          {"rule":{"chain":"wake_fwd","comment":"w db 10.0.0.9","expr":[{"counter":{"packets":0,"bytes":0}}]}},
          {"rule":{"chain":"x","comment":"other","expr":[{"counter":{"packets":9}}]}}]}"#;
        let mut now = BTreeMap::new();
        parse_counters(j, &mut now);
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        let fwd = ("web-1".to_string(), ip, "forward");
        let host = ("web-1".to_string(), ip, "host");
        assert_eq!(now.get(&fwd), Some(&2));
        assert_eq!(now.get(&host), Some(&1));
        assert_eq!(now.len(), 3);
        let prev = BTreeMap::new();
        assert_eq!(risen(&prev, &now), vec![fwd.clone(), host.clone()]);
        assert!(risen(&now, &now).is_empty());
        assert_eq!(parse_comment("w a b"), None);
        assert_eq!(parse_comment("w a 10.0.0.1 x"), None);
    }
}
