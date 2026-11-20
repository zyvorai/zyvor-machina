// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM quarantine: a monotonic deadline in the taps' VM_EDGE entries plus
//! allowlist entries (direction | POLICY_QUARANTINE) in VM_POLICY. The
//! datapath drops everything else whatever the enforcement mode, and stops
//! at the deadline on its own, so a quarantine never outlives its lease.
//! VMs outside the synced edge state get their taps programmed for it.

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};

use super::*;

const NSEC: u64 = 1_000_000_000;

pub(in crate::server) struct Held {
    pub info: VmQuarantine,
    pub until_mono: u64,
}

fn dir_of(s: &str) -> Result<u8> {
    match s.to_ascii_lowercase().as_str() {
        "ingress" | "in" => Ok(POLICY_INGRESS),
        "egress" | "out" => Ok(POLICY_EGRESS),
        o => Err(anyhow!("allow direction `{o}`: ingress or egress")),
    }
}

fn proto_of(s: &str) -> Result<u8> {
    match s.to_ascii_lowercase().as_str() {
        "" | "any" => Ok(0),
        "tcp" => Ok(6),
        "udp" => Ok(17),
        "sctp" => Ok(132),
        "icmp" => Ok(1),
        "icmpv6" => Ok(58),
        o => Err(anyhow!("allow protocol `{o}`")),
    }
}

/// Global-scope addresses of this host (not loopback).
fn host_addresses() -> Vec<[u8; ADDR_LEN]> {
    let Ok(out) = std::process::Command::new("ip")
        .args(["-j", "addr", "show"])
        .output()
    else {
        return Vec::new();
    };
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let mut addrs = Vec::new();
    for link in v.as_array().into_iter().flatten() {
        if link["ifname"].as_str() == Some("lo") {
            continue;
        }
        for a in link["addr_info"].as_array().into_iter().flatten() {
            if a["scope"].as_str() != Some("global") {
                continue;
            }
            if let Some(ip) = a["local"].as_str() {
                if let Ok(k) = addr16(ip) {
                    addrs.push(k);
                }
            }
        }
    }
    addrs
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl VmEdgeRuntime {
    pub(super) fn subject_of(&self, vm: &str) -> u32 {
        self.state
            .vms
            .iter()
            .find(|v| v.name == vm)
            .map_or_else(|| crate::netpol::vm_identity(vm), vm_ident)
    }

    fn peer_of(&self, peer: &str) -> Option<u32> {
        match peer {
            "host" => Some(IDENTITY_HOST),
            "world" => Some(IDENTITY_WORLD),
            "any" | "" => Some(0),
            p => self.state.vms.iter().find(|v| v.name == p).map(vm_ident),
        }
    }

    /// Allowlist entries and host addresses for `vm_edge_apply`.
    pub(super) fn quarantine_entries(
        &self,
        ips: &mut HashMap<[u8; ADDR_LEN], u32>,
        policy: &mut HashMap<PolicyKey, u32>,
        index: &mut FlowIndex,
    ) {
        let wants_host = self
            .quarantines
            .values()
            .any(|q| q.info.allow.iter().any(|a| a.peer == "host"));
        if wants_host {
            for a in host_addresses() {
                ips.entry(a).or_insert(IDENTITY_HOST);
            }
        }
        for q in self.quarantines.values() {
            let subject = self.subject_of(&q.info.vm);
            index
                .names
                .entry(subject)
                .or_insert_with(|| (q.info.vm.clone(), BTreeMap::new()));
            for a in &q.info.allow {
                let (Some(peer), Ok(dir), Ok(proto)) = (
                    self.peer_of(&a.peer),
                    dir_of(&a.direction),
                    proto_of(&a.proto),
                ) else {
                    tracing::info!(
                        "quarantine {}: allow peer `{}` not known yet",
                        q.info.vm,
                        a.peer
                    );
                    continue;
                };
                // ICMP keys carry the type + 1; the allowlist takes any type.
                let port = if proto == 1 || proto == 58 { 0 } else { a.port };
                policy.insert(
                    PolicyKey {
                        subject_identity: subject,
                        peer_identity: peer,
                        direction: dir | POLICY_QUARANTINE,
                        proto,
                        port: port.to_be_bytes(),
                    },
                    VM_POLICY_ALLOW,
                );
            }
        }
    }

    /// Deadline for a tap of `vm` (0 = not quarantined).
    pub(super) fn quarantine_until(&self, vm: &str) -> u64 {
        self.quarantines.get(vm).map_or(0, |q| q.until_mono)
    }
}

impl Engine {
    pub(in crate::server) fn vm_quarantine(
        &mut self,
        vm: &str,
        secs: u64,
        allow: Vec<VmQuarantineAllow>,
        reason: String,
        by: String,
    ) -> Result<VmQuarantine> {
        if vm.is_empty() {
            return Err(anyhow!("quarantine: VM name required"));
        }
        if secs == 0 || secs > QUARANTINE_MAX_SECS {
            return Err(anyhow!(
                "quarantine: duration must be 1..{QUARANTINE_MAX_SECS} seconds"
            ));
        }
        for a in &allow {
            dir_of(&a.direction)?;
            proto_of(&a.proto)?;
            if a.peer.is_empty() {
                return Err(anyhow!("quarantine: allow entry without a peer"));
            }
        }
        let now = Utc::now();
        let info = VmQuarantine {
            vm: vm.to_string(),
            since: rfc3339(now),
            until: rfc3339(now + ChronoDuration::seconds(secs as i64)),
            allow,
            reason,
            by,
            ..Default::default()
        };
        self.quarantine_set(info, loader::monotonic_ns() + secs * NSEC)?;
        Ok(self
            .vm_quarantines()
            .into_iter()
            .find(|q| q.vm == vm)
            .unwrap_or_default())
    }

    /// Hold a persisted quarantine again for the time it has left.
    pub(in crate::server) fn quarantine_restore(&mut self, info: VmQuarantine) -> Result<()> {
        let until = DateTime::parse_from_rfc3339(&info.until)?.with_timezone(&Utc);
        let left = (until - Utc::now()).num_seconds();
        if left <= 0 {
            return Ok(());
        }
        self.quarantine_set(info, loader::monotonic_ns() + left as u64 * NSEC)
    }

    fn quarantine_set(&mut self, info: VmQuarantine, until_mono: u64) -> Result<()> {
        let vm = info.vm.clone();
        self.vm_edge
            .quarantines
            .insert(vm.clone(), Held { info, until_mono });
        self.vm_edge_apply()?;
        self.quarantine_push(&vm);
        Ok(())
    }

    pub(in crate::server) fn vm_quarantine_release(&mut self, vm: &str) -> Result<bool> {
        if self.vm_edge.quarantines.remove(vm).is_none() {
            return Ok(false);
        }
        self.quarantine_push(vm);
        self.vm_edge_apply()?;
        Ok(true)
    }

    /// Forget quarantines past their deadline (the datapath already
    /// stopped applying them).
    pub(in crate::server) fn quarantine_expire(&mut self) {
        let now = loader::monotonic_ns();
        let gone: Vec<String> = self
            .vm_edge
            .quarantines
            .iter()
            .filter(|(_, q)| q.until_mono <= now)
            .map(|(v, _)| v.clone())
            .collect();
        for vm in gone {
            tracing::info!("quarantine of {vm} expired");
            if let Err(e) = self.vm_quarantine_release(&vm) {
                tracing::warn!("quarantine expiry {vm}: {e:#}");
            }
        }
    }

    /// Write the deadline into programmed taps of `vm`, then program or
    /// drop taps that exist only for a quarantine.
    fn quarantine_push(&mut self, vm: &str) {
        let until = self.vm_edge.quarantine_until(vm);
        let idxs: HashSet<u32> = self
            .vm_edge
            .taps
            .values()
            .filter(|(_, v)| v == vm)
            .map(|(i, _)| *i)
            .collect();
        if !idxs.is_empty() {
            let cfgs = self
                .dp
                .hash_entries::<u32, VmEdgeCfg>("VM_EDGE")
                .unwrap_or_default();
            for (idx, mut cfg) in cfgs.into_iter().filter(|(i, _)| idxs.contains(i)) {
                cfg.quarantine_until_ns = until;
                if let Err(e) = self.dp.cni_hash_insert("VM_EDGE", idx, cfg) {
                    tracing::warn!("quarantine {vm}: VM_EDGE[{idx}]: {e:#}");
                }
            }
        }
        self.vm_edge_refresh();
    }

    pub(in crate::server) fn vm_quarantines(&self) -> Vec<VmQuarantine> {
        let now = loader::monotonic_ns();
        self.vm_edge
            .quarantines
            .values()
            .map(|q| {
                let mut taps: Vec<String> = self
                    .vm_edge
                    .taps
                    .iter()
                    .filter(|(_, (_, v))| *v == q.info.vm)
                    .map(|(t, _)| t.clone())
                    .collect();
                taps.sort();
                VmQuarantine {
                    remaining_secs: q.until_mono.saturating_sub(now).div_ceil(NSEC),
                    taps,
                    ..q.info.clone()
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(direction: &str, peer: &str, proto: &str, port: u16) -> VmQuarantineAllow {
        VmQuarantineAllow {
            direction: direction.into(),
            peer: peer.into(),
            proto: proto.into(),
            port,
        }
    }

    #[test]
    fn allowlist_becomes_quarantine_keys() {
        let mut rt = VmEdgeRuntime::default();
        rt.state.vms.push(VmEdgeVm {
            name: "db".into(),
            identity: Some(77),
            ..Default::default()
        });
        let info = VmQuarantine {
            vm: "web".into(),
            allow: vec![
                allow("egress", "db", "tcp", 5432),
                allow("ingress", "world", "icmp", 8),
                allow("egress", "not-a-vm", "", 0),
            ],
            ..Default::default()
        };
        rt.quarantines.insert(
            "web".into(),
            Held {
                info,
                until_mono: 1,
            },
        );
        let mut ips = HashMap::new();
        let mut policy = HashMap::new();
        let mut index = FlowIndex::default();
        rt.quarantine_entries(&mut ips, &mut policy, &mut index);

        let web = crate::netpol::vm_identity("web");
        let key = |peer, dir, proto, port: u16| PolicyKey {
            subject_identity: web,
            peer_identity: peer,
            direction: dir | POLICY_QUARANTINE,
            proto,
            port: port.to_be_bytes(),
        };
        assert_eq!(policy.len(), 2);
        assert!(policy.contains_key(&key(77, POLICY_EGRESS, 6, 5432)));
        assert!(policy.contains_key(&key(IDENTITY_WORLD, POLICY_INGRESS, 1, 0)));
        assert!(ips.is_empty());
        assert_eq!(index.names.get(&web).map(|n| n.0.as_str()), Some("web"));
        assert_eq!(rt.quarantine_until("web"), 1);
        assert_eq!(rt.quarantine_until("db"), 0);
    }

    #[test]
    fn rejects_bad_allow_entries() {
        assert!(dir_of("sideways").is_err());
        assert!(proto_of("gre").is_err());
        assert_eq!(proto_of("ICMPv6").unwrap(), 58);
    }
}
