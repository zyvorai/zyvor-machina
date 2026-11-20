// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Typed map access on [`Datapath`].

use anyhow::{anyhow, Result};
use aya::maps::lpm_trie::Key;
use aya::maps::{Array, HashMap, LpmTrie, MapData, PerCpuHashMap, RingBuf};
use aya::Pod;
use machina_bpf_common::*;

use crate::loader::Datapath;
use crate::policy::Prefix;

impl Datapath {
    fn hash<K: Pod, V: Pod>(&mut self, name: &str) -> Result<HashMap<&mut MapData, K, V>> {
        let m = self
            .ebpf
            .map_mut(name)
            .ok_or_else(|| anyhow!("map {name} missing"))?;
        Ok(HashMap::try_from(m)?)
    }

    fn lpm<K: Pod, V: Pod>(&mut self, name: &str) -> Result<LpmTrie<&mut MapData, K, V>> {
        let m = self
            .ebpf
            .map_mut(name)
            .ok_or_else(|| anyhow!("map {name} missing"))?;
        Ok(LpmTrie::try_from(m)?)
    }

    fn array<V: Pod>(&mut self, name: &str) -> Result<Array<&mut MapData, V>> {
        let m = self
            .ebpf
            .map_mut(name)
            .ok_or_else(|| anyhow!("map {name} missing"))?;
        Ok(Array::try_from(m)?)
    }

    pub fn take_ringbuf(&mut self, name: &str) -> Result<RingBuf<MapData>> {
        let m = self
            .ebpf
            .take_map(name)
            .ok_or_else(|| anyhow!("map {name} missing"))?;
        Ok(RingBuf::try_from(m)?)
    }

    pub fn take_hash<K: Pod, V: Pod>(&mut self, name: &str) -> Result<HashMap<MapData, K, V>> {
        let m = self
            .ebpf
            .take_map(name)
            .ok_or_else(|| anyhow!("map {name} missing"))?;
        Ok(HashMap::try_from(m)?)
    }

    // ---- config ----------------------------------------------------------

    pub fn set_global(&mut self, cfg: GlobalCfg) -> Result<()> {
        self.array::<GlobalCfg>("CONFIG")?.set(0, cfg, 0)?;
        Ok(())
    }

    pub fn set_vm_proxy(&mut self, cfg: VmProxyCfg) -> Result<()> {
        self.array::<VmProxyCfg>("VM_PROXY_CFG")?.set(0, cfg, 0)?;
        Ok(())
    }

    pub fn global(&mut self) -> Result<GlobalCfg> {
        Ok(self.array::<GlobalCfg>("CONFIG")?.get(&0, 0)?)
    }

    pub fn set_tp_offsets(&mut self, offs: &[u32]) -> Result<()> {
        let mut a = self.array::<u32>("TP_OFF")?;
        for (i, v) in offs.iter().enumerate() {
            a.set(i as u32, *v, 0)?;
        }
        Ok(())
    }

    pub fn set_iface_cfg(&mut self, ifindex: u32, cfg: IfaceCfg) -> Result<()> {
        self.hash::<u32, IfaceCfg>("IFACE_CFG")?
            .insert(ifindex, cfg, 0)?;
        Ok(())
    }

    pub fn iface_cfg(&mut self, ifindex: u32) -> Result<IfaceCfg> {
        Ok(self.hash::<u32, IfaceCfg>("IFACE_CFG")?.get(&ifindex, 0)?)
    }

    pub fn remove_iface_cfg(&mut self, ifindex: u32) {
        if let Ok(mut m) = self.hash::<u32, IfaceCfg>("IFACE_CFG") {
            let _ = m.remove(&ifindex);
        }
        if let Ok(mut m) = self.hash::<u32, QosState>("QOS_STATE") {
            let _ = m.remove(&ifindex);
        }
        if let Ok(mut m) = self.hash::<u32, QosState>("CONN_RATE") {
            let _ = m.remove(&ifindex);
        }
    }

    // ---- rate limit / accounting -----------------------------------------

    pub fn rate_set(&mut self, scope: u32, per_sec: u32, burst: u32, policy: u32) -> Result<()> {
        let v = RateCfg {
            per_sec,
            burst,
            policy_id: policy,
            _pad: 0,
        };
        self.hash::<u32, RateCfg>("RATE_CFG")?.insert(scope, v, 0)?;
        Ok(())
    }

    pub fn rate_remove(&mut self, scope: u32) {
        if let Ok(mut m) = self.hash::<u32, RateCfg>("RATE_CFG") {
            let _ = m.remove(&scope);
        }
    }

    /// Per-interface counters summed across CPUs.
    pub fn iface_stats(&mut self) -> Result<Vec<(u32, IfaceStats)>> {
        let m = self
            .ebpf
            .map_mut("IFACE_STATS")
            .ok_or_else(|| anyhow!("map IFACE_STATS missing"))?;
        let m: PerCpuHashMap<&mut MapData, u32, IfaceStats> = PerCpuHashMap::try_from(m)?;
        Ok(m.iter()
            .filter_map(|r| r.ok())
            .map(|(k, per_cpu)| {
                let mut s = IfaceStats::default();
                for c in per_cpu.iter() {
                    s.tx_bytes += c.tx_bytes;
                    s.rx_bytes += c.rx_bytes;
                    s.tx_pkts += c.tx_pkts;
                    s.rx_pkts += c.rx_pkts;
                    s.drops += c.drops;
                }
                (k, s)
            })
            .collect())
    }

    pub fn remove_iface_stats(&mut self, ifindex: u32) {
        if let Some(m) = self.ebpf.map_mut("IFACE_STATS") {
            if let Ok(mut m) = PerCpuHashMap::<&mut MapData, u32, IfaceStats>::try_from(m) {
                let _ = m.remove(&ifindex);
            }
        }
    }

    pub fn set_cgroup_scope(&mut self, cgroup_id: u64, scope: u32) -> Result<()> {
        self.hash::<u64, u32>("CGROUP_SCOPE")?
            .insert(cgroup_id, scope, 0)?;
        Ok(())
    }

    pub fn remove_cgroup_scope(&mut self, cgroup_id: u64) {
        if let Ok(mut m) = self.hash::<u64, u32>("CGROUP_SCOPE") {
            let _ = m.remove(&cgroup_id);
        }
    }

    pub fn set_scope_flags(&mut self, scope: u32, flags: u32) -> Result<()> {
        self.hash::<u32, u32>("SCOPE_FLAGS")?
            .insert(scope, flags, 0)?;
        Ok(())
    }

    // ---- enforcement -----------------------------------------------------

    fn deny_key(scope: u32, p: &Prefix) -> Key<DenyKey> {
        Key::new(
            DENY_KEY_FIXED_BITS + p.bits,
            DenyKey {
                scope: scope.to_be_bytes(),
                addr: p.addr,
            },
        )
    }

    pub fn deny_insert(&mut self, scope: u32, p: &Prefix, policy: u32) -> Result<()> {
        let v = RuleVal {
            policy_id: policy,
            flags: 0,
        };
        self.lpm::<DenyKey, RuleVal>("DENY_LPM")?
            .insert(&Self::deny_key(scope, p), v, 0)?;
        Ok(())
    }

    pub fn deny_remove(&mut self, scope: u32, p: &Prefix) {
        if let Ok(mut m) = self.lpm::<DenyKey, RuleVal>("DENY_LPM") {
            let _ = m.remove(&Self::deny_key(scope, p));
        }
    }

    fn allow_key(scope: u32, proto: u8, port: u16, p: &Prefix) -> Key<AllowKey> {
        Key::new(
            ALLOW_KEY_FIXED_BITS + p.bits,
            AllowKey {
                scope: scope.to_be_bytes(),
                proto,
                _pad: 0,
                port: port.to_be_bytes(),
                addr: p.addr,
            },
        )
    }

    pub fn allow_insert(
        &mut self,
        scope: u32,
        proto: u8,
        port: u16,
        p: &Prefix,
        policy: u32,
    ) -> Result<()> {
        let v = RuleVal {
            policy_id: policy,
            flags: 0,
        };
        self.lpm::<AllowKey, RuleVal>("ALLOW_LPM")?.insert(
            &Self::allow_key(scope, proto, port, p),
            v,
            0,
        )?;
        Ok(())
    }

    pub fn allow_remove(&mut self, scope: u32, proto: u8, port: u16, p: &Prefix) {
        if let Ok(mut m) = self.lpm::<AllowKey, RuleVal>("ALLOW_LPM") {
            let _ = m.remove(&Self::allow_key(scope, proto, port, p));
        }
    }

    pub fn port_insert(&mut self, scope: u32, proto: u8, port: u16, policy: u32) -> Result<()> {
        let k = PortKey {
            scope,
            proto,
            _pad: 0,
            port: port.to_be_bytes(),
        };
        self.hash::<PortKey, RuleVal>("DENY_PORTS")?.insert(
            k,
            RuleVal {
                policy_id: policy,
                flags: 0,
            },
            0,
        )?;
        Ok(())
    }

    pub fn port_remove(&mut self, scope: u32, proto: u8, port: u16) {
        let k = PortKey {
            scope,
            proto,
            _pad: 0,
            port: port.to_be_bytes(),
        };
        if let Ok(mut m) = self.hash::<PortKey, RuleVal>("DENY_PORTS") {
            let _ = m.remove(&k);
        }
    }

    pub fn exec_insert(&mut self, hash: u64, policy: u32) -> Result<()> {
        self.hash::<u64, u32>("EXEC_DENY")?
            .insert(hash, policy, 0)?;
        Ok(())
    }

    pub fn exec_remove(&mut self, hash: u64) {
        if let Ok(mut m) = self.hash::<u64, u32>("EXEC_DENY") {
            let _ = m.remove(&hash);
        }
    }

    pub fn cap_insert(&mut self, scope: u32, cap: u32, policy: u32) -> Result<()> {
        self.hash::<CapKey, u32>("CAP_DENY")?
            .insert(CapKey { scope, cap }, policy, 0)?;
        Ok(())
    }

    pub fn cap_remove(&mut self, scope: u32, cap: u32) {
        if let Ok(mut m) = self.hash::<CapKey, u32>("CAP_DENY") {
            let _ = m.remove(&CapKey { scope, cap });
        }
    }

    /// Replace all FILE_WATCH slots. Entries: (prefix, deny policy id).
    pub fn set_file_watch(&mut self, entries: &[(String, Option<u32>)]) -> Result<()> {
        let mut a = self.array::<FileWatch>("FILE_WATCH")?;
        for slot in 0..FILE_WATCH_SLOTS {
            let mut w = FileWatch {
                len: 0,
                flags: 0,
                policy_id: 0,
                _pad: 0,
                prefix: [0; FILE_WATCH_PREFIX_LEN],
            };
            if let Some((prefix, deny)) = entries.get(slot as usize) {
                let b = prefix.as_bytes();
                let n = b.len().min(FILE_WATCH_PREFIX_LEN);
                w.prefix[..n].copy_from_slice(&b[..n]);
                w.len = n as u32;
                if let Some(p) = deny {
                    w.flags = WATCH_DENY;
                    w.policy_id = *p;
                }
            }
            a.set(slot, w, 0)?;
        }
        Ok(())
    }

    // ---- telemetry reads -------------------------------------------------

    pub fn dump_flows(&mut self) -> Result<Vec<(FlowKey, FlowVal)>> {
        let m = self.hash::<FlowKey, FlowVal>("FLOWS")?;
        Ok(m.iter().filter_map(|r| r.ok()).collect())
    }

    pub fn remove_flow(&mut self, k: &FlowKey) {
        if let Ok(mut m) = self.hash::<FlowKey, FlowVal>("FLOWS") {
            let _ = m.remove(k);
        }
    }

    pub fn drop_reasons(&mut self) -> Result<Vec<(u32, u64)>> {
        let m = self.hash::<u32, u64>("DROP_REASONS")?;
        Ok(m.iter().filter_map(|r| r.ok()).collect())
    }

    pub fn tcp_health(&mut self) -> Result<Vec<(HealthKey, u64)>> {
        let m = self.hash::<HealthKey, u64>("TCP_HEALTH")?;
        Ok(m.iter().filter_map(|r| r.ok()).collect())
    }

    // ---- uplink XDP ------------------------------------------------------

    pub fn xdp_set_cfg(&mut self, cfg: XdpCfg) -> Result<()> {
        self.array::<XdpCfg>("XDP_CFG")?.set(0, cfg, 0)?;
        Ok(())
    }

    /// Point a dispatcher tail-call slot at an XDP program (loading it).
    pub fn xdp_set_slot(&mut self, slot: u32, prog: &str) -> Result<()> {
        let fd = self.xdp_program(prog)?.fd()?.try_clone()?;
        let map = self
            .ebpf
            .map_mut("XDP_PROGS")
            .ok_or_else(|| anyhow!("map XDP_PROGS missing"))?;
        let mut arr: aya::maps::ProgramArray<&mut MapData> =
            aya::maps::ProgramArray::try_from(map)?;
        arr.set(slot, &fd, 0)?;
        Ok(())
    }

    // ---- AF_XDP ----------------------------------------------------------

    /// The map takes its own socket reference; the caller may close `fd`.
    pub fn xsk_set(&mut self, queue: u32, fd: impl std::os::fd::AsRawFd) -> Result<()> {
        let map = self
            .ebpf
            .map_mut("AFXDP_XSKS")
            .ok_or_else(|| anyhow!("map AFXDP_XSKS missing"))?;
        let mut m: aya::maps::XskMap<&mut MapData> = aya::maps::XskMap::try_from(map)?;
        m.set(queue, fd, 0)?;
        Ok(())
    }

    pub fn xsk_unset(&mut self, queue: u32) {
        if let Some(map) = self.ebpf.map_mut("AFXDP_XSKS") {
            if let Ok(mut m) = aya::maps::XskMap::<&mut MapData>::try_from(map) {
                let _ = m.unset(queue);
            }
        }
    }

    // ---- CNI -------------------------------------------------------------

    pub fn cni_set_node(&mut self, cfg: NodeCfg) -> Result<()> {
        self.array::<NodeCfg>("CNI_NODE")?.set(0, cfg, 0)?;
        Ok(())
    }

    pub fn cni_hash_insert<K: Pod, V: Pod>(&mut self, map: &str, k: K, v: V) -> Result<()> {
        self.hash::<K, V>(map)?.insert(k, v, 0)?;
        Ok(())
    }

    pub fn cni_hash_remove<K: Pod, V: Pod>(&mut self, map: &str, k: &K) {
        if let Ok(mut m) = self.hash::<K, V>(map) {
            let _ = m.remove(k);
        }
    }

    pub fn cni_hash_keys<K: Pod, V: Pod>(&mut self, map: &str) -> Result<Vec<K>> {
        let m = self.hash::<K, V>(map)?;
        Ok(m.keys().filter_map(|r| r.ok()).collect())
    }

    /// Every entry of a hash / LRU hash map.
    pub fn hash_entries<K: Pod, V: Pod>(&mut self, map: &str) -> Result<Vec<(K, V)>> {
        let m = self.hash::<K, V>(map)?;
        Ok(m.iter().filter_map(|r| r.ok()).collect())
    }

    pub fn array_set<V: Pod>(&mut self, map: &str, index: u32, v: V) -> Result<()> {
        self.array::<V>(map)?.set(index, v, 0)?;
        Ok(())
    }

    /// Per-CPU hash entries folded with `add`.
    pub fn percpu_sum<K: Pod, V: Pod + Default>(
        &mut self,
        map: &str,
        add: impl Fn(&mut V, &V),
    ) -> Result<Vec<(K, V)>> {
        let m = self
            .ebpf
            .map_mut(map)
            .ok_or_else(|| anyhow!("map {map} missing"))?;
        let m: PerCpuHashMap<&mut MapData, K, V> = PerCpuHashMap::try_from(m)?;
        Ok(m.iter()
            .filter_map(|r| r.ok())
            .map(|(k, per_cpu)| {
                let mut s = V::default();
                for c in per_cpu.iter() {
                    add(&mut s, c);
                }
                (k, s)
            })
            .collect())
    }

    pub fn percpu_remove<K: Pod, V: Pod>(&mut self, map: &str, k: &K) {
        if let Some(m) = self.ebpf.map_mut(map) {
            if let Ok(mut m) = PerCpuHashMap::<&mut MapData, K, V>::try_from(m) {
                let _ = m.remove(k);
            }
        }
    }

    pub fn cni_cidr_insert(&mut self, addr: [u8; ADDR_LEN], bits: u32, id: u32) -> Result<()> {
        self.lpm::<[u8; ADDR_LEN], u32>("CNI_CIDR_IDS")?
            .insert(&Key::new(bits, addr), id, 0)?;
        Ok(())
    }

    pub fn cni_cidr_clear(&mut self) -> Result<()> {
        self.addr_lpm_clear::<u32>("CNI_CIDR_IDS")
    }

    /// Insert into an LPM trie keyed by a 16-byte (IPv4-mapped) address.
    pub fn addr_lpm_insert<V: Pod>(
        &mut self,
        map: &str,
        addr: [u8; ADDR_LEN],
        bits: u32,
        v: V,
    ) -> Result<()> {
        self.lpm::<[u8; ADDR_LEN], V>(map)?
            .insert(&Key::new(bits, addr), v, 0)?;
        Ok(())
    }

    pub fn addr_lpm_clear<V: Pod>(&mut self, map: &str) -> Result<()> {
        let mut m = self.lpm::<[u8; ADDR_LEN], V>(map)?;
        let keys: Vec<Key<[u8; ADDR_LEN]>> = m.keys().filter_map(|r| r.ok()).collect();
        for k in keys {
            let _ = m.remove(&k);
        }
        Ok(())
    }

    /// One per-CPU array slot folded with `add`.
    pub fn percpu_array_sum<V: Pod + Default>(
        &mut self,
        map: &str,
        index: u32,
        add: impl Fn(&mut V, &V),
    ) -> Result<V> {
        let m = self
            .ebpf
            .map_mut(map)
            .ok_or_else(|| anyhow!("map {map} missing"))?;
        let m: aya::maps::PerCpuArray<&mut MapData, V> = aya::maps::PerCpuArray::try_from(m)?;
        let mut s = V::default();
        for c in m.get(&index, 0)?.iter() {
            add(&mut s, c);
        }
        Ok(s)
    }

    pub fn hash_clear<K: Pod, V: Pod>(&mut self, map: &str) {
        if let Ok(mut m) = self.hash::<K, V>(map) {
            let keys: Vec<K> = m.keys().filter_map(|r| r.ok()).collect();
            for k in keys {
                let _ = m.remove(&k);
            }
        }
    }
}
