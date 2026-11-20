// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Source addresses each VM's tap sent from: identities for addresses the
//! inventory can't see (static IPv6, extra NICs, no guest agent). Bounded
//! per VM, so a VM sending from random sources can't grow it; the
//! controller decides which ones it trusts.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Reported while seen within this window.
pub(crate) const KEEP: Duration = Duration::from_secs(15 * 60);
pub(crate) const MAX_PER_VM: usize = 16;

#[derive(Default)]
pub(crate) struct Learned {
    by_vm: HashMap<String, Vec<(IpAddr, Instant)>>,
}

fn usable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_link_local()
                || a.is_multicast()
                || a.is_broadcast())
        }
        IpAddr::V6(a) => {
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                || (a.segments()[0] & 0xffc0) == 0xfe80
                || a.to_ipv4_mapped().is_some())
        }
    }
}

impl Learned {
    pub(crate) fn note(&mut self, vm: &str, ip: IpAddr, now: Instant) {
        if !usable(ip) {
            return;
        }
        let v = self.by_vm.entry(vm.to_string()).or_default();
        if let Some(e) = v.iter_mut().find(|e| e.0 == ip) {
            e.1 = now;
            return;
        }
        v.retain(|e| now.duration_since(e.1) < KEEP);
        if v.len() >= MAX_PER_VM {
            if let Some(i) = (0..v.len()).min_by_key(|i| v[*i].1) {
                v.swap_remove(i);
            }
        }
        v.push((ip, now));
    }

    /// VM → addresses seen within `KEEP`, sorted; drops the rest.
    pub(crate) fn report(&mut self, now: Instant) -> BTreeMap<String, Vec<String>> {
        self.by_vm.retain(|_, v| {
            v.retain(|e| now.duration_since(e.1) < KEEP);
            !v.is_empty()
        });
        self.by_vm
            .iter()
            .map(|(vm, v)| {
                let mut a: Vec<String> = v.iter().map(|e| e.0.to_string()).collect();
                a.sort();
                (vm.clone(), a)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_bounded_and_expires() {
        let mut l = Learned::default();
        let t0 = Instant::now();
        l.note("a", "192.168.122.5".parse().unwrap(), t0);
        l.note("a", "fd00::5".parse().unwrap(), t0);
        l.note("a", "fe80::1".parse().unwrap(), t0);
        l.note("a", "0.0.0.0".parse().unwrap(), t0);
        l.note("a", "::ffff:10.0.0.1".parse().unwrap(), t0);
        l.note("a", "192.168.122.5".parse().unwrap(), t0);
        assert_eq!(
            l.report(t0)["a"],
            ["192.168.122.5".to_string(), "fd00::5".to_string()]
        );
        for i in 0..40u8 {
            l.note(
                "b",
                IpAddr::from([10, 0, 0, i]),
                t0 + Duration::from_secs(i as u64),
            );
        }
        let b = &l.report(t0 + Duration::from_secs(40))["b"];
        assert_eq!(b.len(), MAX_PER_VM);
        assert!(b.contains(&"10.0.0.39".to_string()) && !b.contains(&"10.0.0.0".to_string()));
        assert!(l.report(t0 + KEEP + Duration::from_secs(41)).is_empty());
    }
}
