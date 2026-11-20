// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Chaos faults on VM taps. Latency, jitter and loss are a `netem` root
//! qdisc on the tap (handle `4d43:`), so they act on traffic towards the VM:
//! the tap's ingress belongs to the eBPF programs. Partitions are the
//! `bridge machina_chaos` table, dropping the tap's traffic to and from the
//! listed CIDRs in both directions. Every fault has a lease.

use std::fmt::Write as _;
use std::net::IpAddr;

use crate::api::{VmChaosFault, CHAOS_MAX_SECS};

pub const TABLE: &str = "machina_chaos";
pub const HANDLE: &str = "4d43:";
pub const MAX_DELAY_MS: u32 = 10_000;
pub const MAX_PARTITION: usize = 64;

pub fn ifname_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 15
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn vm_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// `addr/len` with a family-valid length; a bare address is a host route.
pub fn parse_cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (a, l) = match s.split_once('/') {
        Some((a, l)) => (a, Some(l)),
        None => (s, None),
    };
    let ip: IpAddr = a.trim().parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = match l {
        Some(l) => l.trim().parse::<u8>().ok().filter(|l| *l <= max)?,
        None => max,
    };
    Some((ip, len))
}

pub fn has_netem(f: &VmChaosFault) -> bool {
    f.delay_ms > 0 || f.loss_pct > 0.0
}

pub fn validate(f: &VmChaosFault) -> Result<(), String> {
    if !id_ok(&f.id) {
        return Err(format!("invalid fault id {:?}", f.id));
    }
    match (&f.tap, f.vm.is_empty()) {
        (Some(t), _) if !ifname_ok(t) => return Err(format!("invalid interface {t:?}")),
        (None, true) => return Err("a fault needs a vm or a tap".into()),
        (None, false) if !vm_ok(&f.vm) => return Err(format!("invalid vm name {:?}", f.vm)),
        _ => {}
    }
    if f.secs == 0 || f.secs > CHAOS_MAX_SECS {
        return Err(format!("lease must be 1..={CHAOS_MAX_SECS} seconds"));
    }
    if f.delay_ms > MAX_DELAY_MS {
        return Err(format!("delay at most {MAX_DELAY_MS} ms"));
    }
    if f.jitter_ms > f.delay_ms {
        return Err("jitter cannot exceed the delay".into());
    }
    if !(0.0..=100.0).contains(&f.loss_pct) || f.loss_pct.is_nan() {
        return Err("loss must be 0..=100 percent".into());
    }
    if f.partition.len() > MAX_PARTITION {
        return Err(format!("at most {MAX_PARTITION} partition CIDRs"));
    }
    for c in &f.partition {
        if parse_cidr(c).is_none() {
            return Err(format!("invalid partition CIDR {c:?}"));
        }
    }
    if !has_netem(f) && f.partition.is_empty() {
        return Err("the fault does nothing: set a delay, loss or partition".into());
    }
    Ok(())
}

/// `netem` parameters, e.g. `delay 200ms 20ms loss 5%`.
pub fn netem_args(f: &VmChaosFault) -> Vec<String> {
    let mut a = Vec::new();
    if f.delay_ms > 0 {
        a.extend(["delay".into(), format!("{}ms", f.delay_ms)]);
        if f.jitter_ms > 0 {
            a.push(format!("{}ms", f.jitter_ms));
        }
    }
    if f.loss_pct > 0.0 {
        a.extend(["loss".into(), format!("{}%", f.loss_pct)]);
    }
    a
}

/// The bridge table for `(tap, partition CIDRs)`; `None` with no partition.
pub fn render(parts: &[(String, Vec<String>)]) -> Option<String> {
    let mut rules = String::new();
    for (tap, cidrs) in parts {
        if !ifname_ok(tap) {
            continue;
        }
        for c in cidrs {
            let Some((ip, len)) = parse_cidr(c) else {
                continue;
            };
            let fam = if ip.is_ipv4() { "ip" } else { "ip6" };
            let _ = writeln!(rules, "    iifname \"{tap}\" {fam} daddr {ip}/{len} drop");
            let _ = writeln!(rules, "    oifname \"{tap}\" {fam} saddr {ip}/{len} drop");
        }
    }
    if rules.is_empty() {
        return None;
    }
    Some(format!(
        "table bridge {TABLE} {{
  chain chaos_fwd {{
    type filter hook forward priority filter - 10; policy accept;
{rules}  }}
  chain chaos_in {{
    type filter hook input priority filter - 10; policy accept;
{rules}  }}
  chain chaos_out {{
    type filter hook output priority filter - 10; policy accept;
{rules}  }}
}}
"
    ))
}

pub fn transaction(script: Option<&str>) -> String {
    format!(
        "table bridge {TABLE} {{}}\ndelete table bridge {TABLE}\n{}",
        script.unwrap_or("")
    )
}

/// `(kind, handle)` of the root qdisc in `tc -j qdisc show dev X root`.
pub fn root_qdisc(json: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.as_array()?.iter().find_map(|q| {
        (q["root"].as_bool() == Some(true) || q["parent"].is_null()).then_some(())?;
        Some((
            q["kind"].as_str()?.to_string(),
            q["handle"].as_str().unwrap_or("").to_string(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f() -> VmChaosFault {
        VmChaosFault {
            id: "exp1:0".into(),
            vm: "web-1".into(),
            delay_ms: 200,
            jitter_ms: 20,
            loss_pct: 5.0,
            secs: 60,
            ..Default::default()
        }
    }

    #[test]
    fn validates() {
        validate(&f()).unwrap();
        type Mutate = Box<dyn Fn(&mut VmChaosFault)>;
        let bad: Vec<Mutate> = vec![
            Box::new(|x| x.id = "a b".into()),
            Box::new(|x| x.vm.clear()),
            Box::new(|x| x.tap = Some("eth0;rm".into())),
            Box::new(|x| x.secs = 0),
            Box::new(|x| x.secs = CHAOS_MAX_SECS + 1),
            Box::new(|x| x.delay_ms = MAX_DELAY_MS + 1),
            Box::new(|x| x.jitter_ms = 500),
            Box::new(|x| x.loss_pct = 101.0),
            Box::new(|x| x.loss_pct = f64::NAN),
            Box::new(|x| x.partition = vec!["10.0.0.0/33".into()]),
            Box::new(|x| {
                x.delay_ms = 0;
                x.jitter_ms = 0;
                x.loss_pct = 0.0;
            }),
        ];
        for (i, b) in bad.iter().enumerate() {
            let mut x = f();
            b(&mut x);
            assert!(validate(&x).is_err(), "case {i} accepted");
        }
        let mut p = f();
        p.delay_ms = 0;
        p.jitter_ms = 0;
        p.loss_pct = 0.0;
        p.partition = vec!["0.0.0.0/0".into(), "fd00::1".into()];
        validate(&p).unwrap();
    }

    #[test]
    fn netem_and_nft() {
        assert_eq!(netem_args(&f()), ["delay", "200ms", "20ms", "loss", "5%"]);
        let s = render(&[("vnet3".into(), vec!["10.0.0.0/8".into(), "fd00::1".into()])]).unwrap();
        assert!(s.contains("iifname \"vnet3\" ip daddr 10.0.0.0/8 drop"));
        assert!(s.contains("oifname \"vnet3\" ip6 saddr fd00::1/128 drop"));
        assert_eq!(s.matches("type filter hook").count(), 3);
        assert!(render(&[("vnet3".into(), vec![])]).is_none());
        assert!(render(&[("bad name".into(), vec!["10.0.0.0/8".into()])]).is_none());
        assert!(transaction(None).contains("delete table bridge machina_chaos"));
    }

    #[test]
    fn parses_root_qdisc() {
        let j = r#"[{"kind":"netem","handle":"4d43:","root":true,"refcnt":2,"options":{}}]"#;
        assert_eq!(root_qdisc(j), Some(("netem".into(), "4d43:".into())));
        assert_eq!(
            root_qdisc(r#"[{"kind":"fq","handle":"8001:","root":true}]"#),
            Some(("fq".into(), "8001:".into()))
        );
        assert_eq!(root_qdisc("[]"), None);
    }
}
