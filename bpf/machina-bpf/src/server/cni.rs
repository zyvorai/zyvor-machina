// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-cni datapath state: pod endpoints, identities, NetworkPolicy
//! entries and service frontends (with Maglev tables), reconciled into the
//! dual-stack CNI_* maps.

use std::collections::{BTreeSet, HashSet};
use std::net::IpAddr;

use super::*;

const CNI_SOCK_PROGS: &[&str] = &[
    "mn_cni_connect4",
    "mn_cni_sendmsg4",
    "mn_cni_recvmsg4",
    "mn_cni_connect6",
    "mn_cni_sendmsg6",
    "mn_cni_recvmsg6",
];

#[derive(Default)]
pub(super) struct CniRuntime {
    pub config: Option<CniNodeConfig>,
    pub endpoints: HashMap<String, CniEndpoint>,
    /// (addr, port, proto) → svc id, stable across syncs.
    svc_ids: HashMap<(String, u16, u8), u32>,
    next_svc: u32,
    /// Backend count last written per svc id (to clear stale indexes).
    svc_backends: HashMap<u32, u32>,
    /// Maglev table last written per svc id.
    maglev: HashMap<u32, Vec<u32>>,
    pub last: CniState,
    pub last_sync: Option<String>,
}

/// Any IP literal → the datapath's 16-byte form (IPv4-mapped for v4).
pub(crate) fn addr16(s: &str) -> Result<[u8; ADDR_LEN]> {
    match s
        .trim()
        .parse::<IpAddr>()
        .map_err(|_| anyhow!("invalid IP address `{s}`"))?
    {
        IpAddr::V4(a) => Ok(v4_mapped(a.octets())),
        IpAddr::V6(a) => Ok(a.octets()),
    }
}

fn mac(s: &str) -> Result<[u8; 6]> {
    let parts: Vec<u8> = s
        .split(':')
        .map(|p| u8::from_str_radix(p, 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| anyhow!("invalid MAC `{s}`"))?;
    parts.try_into().map_err(|_| anyhow!("invalid MAC `{s}`"))
}

/// `"10.0.0.0/8"` / `"fd00::/64"` → (16-byte network, prefix bits in the
/// 128-bit space; IPv4 prefixes are offset by 96).
pub(crate) fn cidr16(s: &str) -> Result<([u8; ADDR_LEN], u32)> {
    let (a, l) = s.split_once('/').unwrap_or((s, ""));
    let ip: IpAddr = a
        .trim()
        .parse()
        .map_err(|_| anyhow!("invalid CIDR `{s}`"))?;
    let (max, offset) = if ip.is_ipv4() { (32, 96) } else { (128, 0) };
    let bits: u32 = if l.is_empty() {
        max
    } else {
        l.parse()
            .ok()
            .filter(|b| *b <= max)
            .ok_or_else(|| anyhow!("invalid prefix in `{s}`"))?
    };
    let total = bits + offset;
    let addr = u128::from_be_bytes(addr16(a)?);
    let mask = if total == 0 {
        0
    } else {
        u128::MAX << (128 - total)
    };
    Ok(((addr & mask).to_be_bytes(), total))
}

pub(crate) fn fnv64(s: &str, seed: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ (h >> 29)
}

/// Maglev lookup table (Eisenbud et al., NSDI '16): each backend walks its
/// own permutation of the `m` slots; slots are claimed round-robin so every
/// backend owns ~m/n of them and removing one only remaps its own slots.
pub(crate) fn maglev_table(backends: &[String], m: u32) -> Vec<u32> {
    let n = backends.len();
    let m = m as usize;
    if n == 0 {
        return Vec::new();
    }
    let perm: Vec<(usize, usize)> = backends
        .iter()
        .map(|b| {
            (
                (fnv64(b, 0) % m as u64) as usize,
                (fnv64(b, 0x5bd1_e995) % (m as u64 - 1)) as usize + 1,
            )
        })
        .collect();
    let mut next = vec![0usize; n];
    let mut table = vec![u32::MAX; m];
    let mut filled = 0;
    while filled < m {
        for (i, (offset, skip)) in perm.iter().enumerate() {
            let mut c = (offset + next[i] * skip) % m;
            while table[c] != u32::MAX {
                next[i] += 1;
                c = (offset + next[i] * skip) % m;
            }
            table[c] = i as u32;
            next[i] += 1;
            filled += 1;
            if filled == m {
                break;
            }
        }
    }
    table
}

impl Engine {
    pub(super) fn cni_configure(&mut self, cfg: CniNodeConfig) -> Result<CniStatus> {
        let addr = addr16(&cfg.node_addr)?;
        if addr[..12] != v4_mapped([0; 4])[..12] {
            return Err(anyhow!("node_addr must be IPv4 (use node_addr6 for IPv6)"));
        }
        let addr6 = match cfg.node_addr6.as_deref().filter(|s| !s.is_empty()) {
            Some(s) => {
                let a = addr16(s)?;
                if a[..12] == v4_mapped([0; 4])[..12] {
                    return Err(anyhow!("node_addr6 must be IPv6"));
                }
                a
            }
            None => [0; ADDR_LEN],
        };
        let dsr = match cfg.lb_mode.as_deref().unwrap_or("snat") {
            "snat" | "" => false,
            "dsr" => true,
            other => return Err(anyhow!("lb_mode `{other}` not supported (snat, dsr)")),
        };
        let mut uplink_ifindex = 0;
        if let Some(up) = cfg.uplink.as_deref() {
            uplink_ifindex = if_nametoindex(up).ok_or_else(|| anyhow!("uplink {up} not found"))?;
        }
        self.dp.cni_set_node(NodeCfg {
            node_addr: addr,
            node_addr6: addr6,
            flags: if dsr { NODE_F_DSR } else { 0 },
            uplink_ifindex,
        })?;
        if let Some(up) = cfg.uplink.as_deref() {
            self.dp.attach_tc_one(up, "mn_cni_nodeport", true)?;
            if cfg.xdp {
                self.xdp_uplink_refresh(up, true)?;
            }
        }
        if self.features.cgroup2 {
            let root = Path::new(attribution::CGROUP_ROOT);
            if let Err(e) = self.dp.attach_cgroup_tagged(root, "cni", CNI_SOCK_PROGS) {
                self.dp
                    .notes
                    .push(format!("socket load balancing unavailable: {e:#}"));
            }
        }
        self.cni.config = Some(cfg);
        Ok(self.cni_status())
    }

    pub(super) fn cni_add_endpoint(&mut self, ep: CniEndpoint) -> Result<()> {
        let ip = addr16(&ep.ip)?;
        let idx = if_nametoindex(&ep.host_iface)
            .ok_or_else(|| anyhow!("interface {} not found", ep.host_iface))?;
        let (identity, flags) = self.cni_meta_for(&ep.ip);
        self.dp.cni_hash_insert(
            "CNI_ENDPOINTS",
            ip,
            Endpoint {
                host_ifindex: idx,
                identity,
                flags,
                _pad: 0,
                pod_mac: mac(&ep.pod_mac)?,
                host_mac: mac(&ep.host_mac)?,
                _pad2: 0,
            },
        )?;
        self.dp
            .attach_tc_pair(&ep.host_iface, "mn_cni_from_pod", "mn_cni_to_pod")?;
        self.cni.endpoints.insert(ep.ip.clone(), ep);
        Ok(())
    }

    pub(super) fn cni_del_endpoint(&mut self, ip: &str) -> Result<bool> {
        let Some(ep) = self.cni.endpoints.remove(ip) else {
            return Ok(false);
        };
        if let Ok(a) = addr16(ip) {
            self.dp
                .cni_hash_remove::<[u8; ADDR_LEN], Endpoint>("CNI_ENDPOINTS", &a);
        }
        // A dual-stack pod has one endpoint per family on the same veth.
        if self
            .cni
            .endpoints
            .values()
            .any(|e| e.host_iface == ep.host_iface)
        {
            return Ok(true);
        }
        if if_nametoindex(&ep.host_iface).is_some() {
            self.dp.detach_tc(&ep.host_iface);
        } else {
            self.dp.forget_tc(&ep.host_iface);
        }
        Ok(true)
    }

    fn cni_meta_for(&self, ip: &str) -> (u32, u32) {
        self.cni
            .last
            .identities
            .iter()
            .find(|i| i.ip == ip)
            .map(|i| {
                let mut f = 0;
                if i.ingress_isolated {
                    f |= EP_INGRESS_ISOLATED;
                }
                if i.egress_isolated {
                    f |= EP_EGRESS_ISOLATED;
                }
                (i.identity, f)
            })
            .unwrap_or((0, 0))
    }

    /// Replace the desired state; only changed map entries are written.
    pub(super) fn cni_sync(&mut self, st: CniState) -> Result<CniStatus> {
        if st.version != CNI_STATE_VERSION {
            return Err(anyhow!(
                "CNI state version {} does not match machina-bpfd ({CNI_STATE_VERSION}); upgrade machina-cni and machina-bpfd together",
                st.version
            ));
        }
        // Identities (cluster-wide pod IP → identity).
        let want: HashMap<[u8; ADDR_LEN], u32> = st
            .identities
            .iter()
            .filter_map(|i| addr16(&i.ip).ok().map(|a| (a, i.identity)))
            .collect();
        for k in self
            .dp
            .cni_hash_keys::<[u8; ADDR_LEN], u32>("CNI_IDENTITIES")?
        {
            if !want.contains_key(&k) {
                self.dp
                    .cni_hash_remove::<[u8; ADDR_LEN], u32>("CNI_IDENTITIES", &k);
            }
        }
        for (a, id) in &want {
            self.dp.cni_hash_insert("CNI_IDENTITIES", *a, *id)?;
        }

        // ipBlock CIDRs.
        self.dp.cni_cidr_clear()?;
        for (c, id) in &st.cidrs {
            let (a, bits) = cidr16(c)?;
            self.dp.cni_cidr_insert(a, bits, *id)?;
        }

        // Policy entries.
        let want_pol: HashSet<PolicyKey> = st
            .policy
            .iter()
            .map(|p| PolicyKey {
                subject_identity: p.subject,
                peer_identity: p.peer,
                direction: if p.egress {
                    POLICY_EGRESS
                } else {
                    POLICY_INGRESS
                },
                proto: p.proto,
                port: p.port.to_be_bytes(),
            })
            .collect();
        for k in self.dp.cni_hash_keys::<PolicyKey, u32>("CNI_POLICY")? {
            if !want_pol.contains(&k) {
                self.dp.cni_hash_remove::<PolicyKey, u32>("CNI_POLICY", &k);
            }
        }
        for k in &want_pol {
            self.dp.cni_hash_insert("CNI_POLICY", *k, 1u32)?;
        }

        self.cni_sync_services(&st.services)?;

        self.cni.last = st;
        self.cni.last_sync = Some(chrono::Utc::now().to_rfc3339());

        // Local endpoints pick up their identity / isolation.
        let eps: Vec<CniEndpoint> = self.cni.endpoints.values().cloned().collect();
        for ep in eps {
            if let Err(e) = self.cni_add_endpoint(ep.clone()) {
                tracing::debug!("cni endpoint {}: {e:#}", ep.ip);
            }
        }
        Ok(self.cni_status())
    }

    fn cni_sync_services(&mut self, services: &[CniService]) -> Result<()> {
        let mut live: BTreeSet<u32> = BTreeSet::new();
        let mut want_svc: HashSet<SvcKey> = HashSet::new();
        let mut want_np: HashSet<SvcKey> = HashSet::new();
        for s in services {
            let addr = addr16(&s.addr)?;
            let key = SvcKey {
                addr,
                port: s.port.to_be_bytes(),
                proto: s.proto,
                _pad: 0,
            };
            let id = match self.cni.svc_ids.get(&(s.addr.clone(), s.port, s.proto)) {
                Some(id) => *id,
                None => {
                    self.cni.next_svc += 1;
                    let id = self.cni.next_svc;
                    self.cni
                        .svc_ids
                        .insert((s.addr.clone(), s.port, s.proto), id);
                    id
                }
            };
            live.insert(id);
            // Stable order: Maglev slots and affinity entries hold indexes.
            let mut list: Vec<&CniBackend> = s.backends.iter().collect();
            list.sort_by(|a, b| (&a.addr, a.port).cmp(&(&b.addr, b.port)));
            let mut names = Vec::new();
            let mut backends = Vec::new();
            for b in list {
                let Ok(a) = addr16(&b.addr) else { continue };
                let node = b
                    .node
                    .as_deref()
                    .and_then(|n| addr16(n).ok())
                    .unwrap_or([0; ADDR_LEN]);
                names.push(format!("{}:{}", b.addr, b.port));
                backends.push(Backend {
                    addr: a,
                    port: b.port.to_be_bytes(),
                    flags: if b.remote { BE_F_REMOTE } else { 0 },
                    _pad: 0,
                    node,
                });
            }
            // Backends first so a frontend never points at missing indexes.
            for (i, b) in backends.iter().enumerate() {
                self.dp.cni_hash_insert(
                    "CNI_BACKENDS",
                    BackendKey {
                        svc_id: id,
                        index: i as u32,
                    },
                    *b,
                )?;
            }
            self.cni_sync_maglev(id, &names)?;
            let val = SvcVal {
                svc_id: id,
                backend_count: backends.len() as u32,
                flags: if s.affinity_secs.is_some() {
                    SVC_F_AFFINITY
                } else {
                    0
                },
                affinity_secs: s.affinity_secs.unwrap_or(0),
            };
            if addr == v4_mapped([0; 4]) || addr == [0; ADDR_LEN] {
                self.dp.cni_hash_insert("CNI_NODEPORTS", key, val)?;
                want_np.insert(key);
            } else {
                self.dp.cni_hash_insert("CNI_SERVICES", key, val)?;
                want_svc.insert(key);
            }
            let old = self
                .cni
                .svc_backends
                .insert(id, backends.len() as u32)
                .unwrap_or(0);
            for i in backends.len() as u32..old {
                self.dp.cni_hash_remove::<BackendKey, Backend>(
                    "CNI_BACKENDS",
                    &BackendKey {
                        svc_id: id,
                        index: i,
                    },
                );
            }
        }
        for (map, want) in [("CNI_SERVICES", &want_svc), ("CNI_NODEPORTS", &want_np)] {
            for k in self.dp.cni_hash_keys::<SvcKey, SvcVal>(map)? {
                if !want.contains(&k) {
                    self.dp.cni_hash_remove::<SvcKey, SvcVal>(map, &k);
                }
            }
        }
        let stale: Vec<u32> = self
            .cni
            .svc_backends
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in stale {
            let n = self.cni.svc_backends.remove(&id).unwrap_or(0);
            for i in 0..n {
                self.dp.cni_hash_remove::<BackendKey, Backend>(
                    "CNI_BACKENDS",
                    &BackendKey {
                        svc_id: id,
                        index: i,
                    },
                );
            }
            self.cni_sync_maglev(id, &[])?;
        }
        self.cni.svc_ids.retain(|_, id| live.contains(id));
        Ok(())
    }

    /// Write only the Maglev slots that changed; services with fewer than two
    /// backends have no table.
    fn cni_sync_maglev(&mut self, id: u32, names: &[String]) -> Result<()> {
        let want = if names.len() >= 2 {
            maglev_table(names, MAGLEV_M)
        } else {
            Vec::new()
        };
        let old = self.cni.maglev.remove(&id).unwrap_or_default();
        if want.is_empty() {
            for slot in 0..old.len() as u32 {
                self.dp.cni_hash_remove::<MaglevKey, u32>(
                    "CNI_MAGLEV",
                    &MaglevKey { svc_id: id, slot },
                );
            }
            return Ok(());
        }
        for (slot, idx) in want.iter().enumerate() {
            if old.get(slot) != Some(idx) {
                self.dp.cni_hash_insert(
                    "CNI_MAGLEV",
                    MaglevKey {
                        svc_id: id,
                        slot: slot as u32,
                    },
                    *idx,
                )?;
            }
        }
        self.cni.maglev.insert(id, want);
        Ok(())
    }

    pub(super) fn cni_status(&self) -> CniStatus {
        let mut endpoints: Vec<CniEndpoint> = self.cni.endpoints.values().cloned().collect();
        endpoints.sort_by(|a, b| a.ip.cmp(&b.ip));
        let cfg = self.cni.config.clone().unwrap_or_default();
        CniStatus {
            configured: self.cni.config.is_some(),
            version: CNI_STATE_VERSION,
            node_addr: self.cni.config.as_ref().map(|c| c.node_addr.clone()),
            node_addr6: cfg.node_addr6.clone(),
            uplink: cfg.uplink.clone(),
            lb_mode: cfg.lb_mode.clone().unwrap_or_else(|| "snat".into()),
            xdp: cfg.xdp,
            maglev_services: self.cni.maglev.len(),
            endpoints,
            identities: self.cni.last.identities.len(),
            policy_entries: self.cni.last.policy.len(),
            services: self.cni.last.services.len(),
            last_sync: self.cni.last_sync.clone(),
        }
    }

    pub(super) fn cni_services(&mut self) -> Vec<CniServiceStatus> {
        let mut pins: HashMap<u32, usize> = HashMap::new();
        for (k, _) in self
            .dp
            .hash_entries::<AffinityKey, AffinityVal>("CNI_AFFINITY")
            .unwrap_or_default()
        {
            *pins.entry(k.svc_id).or_default() += 1;
        }
        self.cni
            .last
            .services
            .iter()
            .map(|s| {
                let id = self
                    .cni
                    .svc_ids
                    .get(&(s.addr.clone(), s.port, s.proto))
                    .copied();
                CniServiceStatus {
                    service: s.clone(),
                    maglev: id.is_some_and(|i| self.cni.maglev.contains_key(&i)),
                    affinity_entries: id.and_then(|i| pins.get(&i).copied()).unwrap_or(0),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(
            cidr16("10.1.2.3/8").unwrap(),
            (v4_mapped([10, 0, 0, 0]), 104)
        );
        assert_eq!(
            cidr16("192.168.1.7").unwrap(),
            (v4_mapped([192, 168, 1, 7]), 128)
        );
        assert_eq!(cidr16("0.0.0.0/0").unwrap(), (v4_mapped([0, 0, 0, 0]), 96));
        assert!(cidr16("10.0.0.0/40").is_err());
        let (a, bits) = cidr16("fd00:1:2:3::9/64").unwrap();
        assert_eq!(bits, 64);
        assert_eq!(
            a,
            "fd00:1:2:3::"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets()
        );
        assert_eq!(addr16("::").unwrap(), [0; ADDR_LEN]);
        assert_eq!(addr16("0.0.0.0").unwrap(), v4_mapped([0; 4]));
        assert_eq!(
            mac("aa:bb:cc:00:11:22").unwrap(),
            [0xaa, 0xbb, 0xcc, 0, 0x11, 0x22]
        );
        assert!(mac("aa:bb").is_err());
    }

    #[test]
    fn maglev_is_balanced_and_minimally_disruptive() {
        let names: Vec<String> = (0..10).map(|i| format!("10.42.0.{i}:80")).collect();
        let t = maglev_table(&names, MAGLEV_M);
        assert_eq!(t.len(), MAGLEV_M as usize);
        let mut counts = [0u32; 10];
        for i in &t {
            counts[*i as usize] += 1;
        }
        let (lo, hi) = (*counts.iter().min().unwrap(), *counts.iter().max().unwrap());
        assert!(hi - lo <= 2, "uneven: {counts:?}");

        // Dropping backend 3 only remaps (roughly) the slots it owned.
        let mut fewer = names.clone();
        fewer.remove(3);
        let t2 = maglev_table(&fewer, MAGLEV_M);
        let moved = t
            .iter()
            .zip(&t2)
            .filter(|(a, b)| {
                let before = &names[**a as usize];
                let after = &fewer[**b as usize];
                *a != &3 && before != after
            })
            .count();
        assert!(
            moved < (MAGLEV_M as usize) / 10,
            "{moved} slots of surviving backends moved"
        );
        assert_eq!(maglev_table(&names, MAGLEV_M), t, "deterministic");
    }
}
