// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! QUIC CID-aware load balancer: services on the uplink XDP dispatcher's
//! `XDP_F_QUICLB` slot, with per-service Maglev tables from `cni`.

use std::collections::BTreeMap;
use std::net::IpAddr;

use super::cni::{addr16, fnv64, maglev_table};
use super::direct::{fmt_mac, link_mac, parse_mac};
use super::*;

const STATS: usize = 6;

struct Svc {
    id: u32,
    key: QlbSvcKey,
    cfg: QuicLbConfig,
    backends: Vec<QuicLbBackendStatus>,
    /// Counter values when the service was created (ids are reused).
    base: [u64; STATS],
}

#[derive(Default)]
pub(super) struct QuicLbRuntime {
    iface: Option<String>,
    svcs: BTreeMap<(String, u16), Svc>,
}

/// Default server id: stable per backend address, independent of the set.
pub(crate) fn default_server_id(addr: &str) -> u16 {
    (fnv64(addr, 0x9c1d) & 0xffff) as u16
}

fn arp_mac(ip: &str, iface: &str) -> Option<[u8; 6]> {
    let t = std::fs::read_to_string("/proc/net/arp").ok()?;
    t.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.len() >= 6 && f[0] == ip && f[5] == iface && f[2] != "0x0")
            .then(|| parse_mac(f[3]).ok())
            .flatten()
    })
}

fn iface_ipv4(iface: &str) -> Option<std::net::Ipv4Addr> {
    let out = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", "dev", iface])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let w = s.split_whitespace().skip_while(|w| *w != "inet").nth(1)?;
    w.split('/').next()?.parse().ok()
}

fn canon(ip: &str) -> Result<IpAddr> {
    ip.trim()
        .parse()
        .map_err(|_| anyhow!("invalid IP address `{ip}`"))
}

pub(crate) fn validate(c: &QuicLbConfig) -> Result<()> {
    if c.port == 0 {
        return Err(anyhow!("port is required"));
    }
    if c.backends.is_empty() || c.backends.len() > QLB_MAX_BACKENDS as usize {
        return Err(anyhow!("1..={QLB_MAX_BACKENDS} backends required"));
    }
    if !(3..=20).contains(&c.cid_len) {
        return Err(anyhow!("cid_len must be 3..=20"));
    }
    if c.config_id > 6 {
        return Err(anyhow!("config_id must be 0..=6 (7 marks unroutable CIDs)"));
    }
    if c.mode == QuicLbMode::Ipip {
        let v4 = |s: &str| canon(s).map(|a| a.is_ipv4()).unwrap_or(false);
        if !v4(&c.vip) || c.backends.iter().any(|b| !v4(&b.addr)) {
            return Err(anyhow!("IPIP delivery needs an IPv4 VIP and IPv4 backends"));
        }
    }
    let mut sids = std::collections::HashSet::new();
    for b in &c.backends {
        let a = canon(&b.addr)?.to_string();
        if !sids.insert(b.server_id.unwrap_or_else(|| default_server_id(&a))) {
            return Err(anyhow!(
                "server id collision at backend {a}; set server_id explicitly"
            ));
        }
    }
    Ok(())
}

impl Engine {
    fn quiclb_counters(&mut self, id: u32) -> [u64; STATS] {
        let mut out = [0u64; STATS];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self
                .dp
                .percpu_array_sum::<u64>("QLB_STATS", id * QLB_STAT_SLOTS + i as u32, |a, b| {
                    *a += *b
                })
                .unwrap_or(0);
        }
        out
    }

    fn quiclb_clear(&mut self, id: u32, n: usize, sids: &[u16]) {
        for idx in 0..n as u32 {
            self.dp.cni_hash_remove::<QlbBeKey, QlbBackend>(
                "QLB_BACKENDS",
                &QlbBeKey { svc_id: id, idx },
            );
        }
        for sid in sids {
            self.dp.cni_hash_remove::<QlbSidKey, u32>(
                "QLB_SID",
                &QlbSidKey {
                    svc_id: id,
                    sid: *sid,
                    _pad: 0,
                },
            );
        }
        for slot in 0..MAGLEV_M {
            self.dp
                .cni_hash_remove::<MaglevKey, u32>("QLB_MAGLEV", &MaglevKey { svc_id: id, slot });
        }
    }

    fn quiclb_remove(&mut self, name: &(String, u16)) -> Result<()> {
        let Some(s) = self.quiclb.svcs.remove(name) else {
            return Ok(());
        };
        self.dp
            .cni_hash_remove::<QlbSvcKey, QlbSvc>("QLB_SVCS", &s.key);
        let sids: Vec<u16> = s.backends.iter().map(|b| b.server_id).collect();
        self.quiclb_clear(s.id, s.backends.len(), &sids);
        if self.quiclb.svcs.is_empty() {
            if let Some(iface) = self.quiclb.iface.take() {
                if self.uplink.flags & XDP_F_QUICLB != 0 {
                    self.xdp_uplink_set(&iface, XDP_F_QUICLB, false)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn quiclb_configure(&mut self, c: QuicLbConfig) -> Result<QuicLbStatus> {
        let vip = canon(&c.vip)?;
        let name = (vip.to_string(), c.port);
        if !c.enabled {
            self.quiclb_remove(&name)?;
            return Ok(self.quiclb_status());
        }
        validate(&c)?;
        let iface = if c.iface.is_empty() {
            self.quiclb
                .iface
                .clone()
                .ok_or_else(|| anyhow!("iface (the uplink) is required"))?
        } else {
            c.iface.clone()
        };
        if let Some(cur) = &self.quiclb.iface {
            if *cur != iface && !self.quiclb.svcs.is_empty() {
                return Err(anyhow!(
                    "QUIC LB already runs on {cur}; one uplink per host"
                ));
            }
        }
        if if_nametoindex(&iface).is_none() {
            return Err(anyhow!("interface {iface} not found"));
        }
        let src_mac = link_mac(&iface).ok_or_else(|| anyhow!("cannot read MAC of {iface}"))?;
        let encap_src = match c.mode {
            QuicLbMode::Ipip => match &c.encap_src {
                Some(s) => match canon(s)? {
                    IpAddr::V4(a) => a.octets(),
                    IpAddr::V6(_) => return Err(anyhow!("encap_src must be IPv4")),
                },
                None => iface_ipv4(&iface)
                    .ok_or_else(|| anyhow!("{iface} has no IPv4 address; pass encap_src"))?
                    .octets(),
            },
            QuicLbMode::Dsr => [0; 4],
        };
        let mut backends = Vec::with_capacity(c.backends.len());
        let mut entries = Vec::with_capacity(c.backends.len());
        for b in &c.backends {
            let a = canon(&b.addr)?;
            let mac = match &b.mac {
                Some(m) => parse_mac(m)?,
                None => arp_mac(&a.to_string(), &iface).ok_or_else(|| {
                    anyhow!("no neighbour entry for {a} on {iface}; reach it once or pass mac")
                })?,
            };
            let sid = b
                .server_id
                .unwrap_or_else(|| default_server_id(&a.to_string()));
            entries.push(QlbBackend {
                addr: addr16(&a.to_string())?,
                mac,
                _pad: [0; 2],
            });
            backends.push(QuicLbBackendStatus {
                addr: a.to_string(),
                mac: fmt_mac(&mac),
                server_id: sid,
            });
        }
        let id = match self.quiclb.svcs.get(&name) {
            Some(s) => s.id,
            None => (0..QLB_MAX_SVCS)
                .find(|i| !self.quiclb.svcs.values().any(|s| s.id == *i))
                .ok_or_else(|| anyhow!("at most {QLB_MAX_SVCS} QUIC services"))?,
        };
        if let Some(old) = self.quiclb.svcs.get(&name) {
            let sids: Vec<u16> = old.backends.iter().map(|b| b.server_id).collect();
            let n = old.backends.len();
            self.quiclb_clear(id, n, &sids);
        }
        for (idx, (e, b)) in entries.iter().zip(&backends).enumerate() {
            self.dp.cni_hash_insert(
                "QLB_BACKENDS",
                QlbBeKey {
                    svc_id: id,
                    idx: idx as u32,
                },
                *e,
            )?;
            self.dp.cni_hash_insert(
                "QLB_SID",
                QlbSidKey {
                    svc_id: id,
                    sid: b.server_id,
                    _pad: 0,
                },
                idx as u32,
            )?;
        }
        if backends.len() >= 2 {
            let names: Vec<String> = backends.iter().map(|b| b.addr.clone()).collect();
            for (slot, idx) in maglev_table(&names, MAGLEV_M).iter().enumerate() {
                self.dp.cni_hash_insert(
                    "QLB_MAGLEV",
                    MaglevKey {
                        svc_id: id,
                        slot: slot as u32,
                    },
                    *idx,
                )?;
            }
        }
        let key = QlbSvcKey {
            addr: addr16(&name.0)?,
            port: c.port.to_be_bytes(),
            _pad: [0; 2],
        };
        self.dp.cni_hash_insert(
            "QLB_SVCS",
            key,
            QlbSvc {
                svc_id: id,
                backend_count: backends.len() as u32,
                cid_len: c.cid_len,
                mode: if c.mode == QuicLbMode::Ipip {
                    QLB_MODE_IPIP
                } else {
                    QLB_MODE_DSR
                },
                config_id: c.config_id,
                _pad: 0,
                src_mac,
                _pad2: [0; 2],
                encap_src,
            },
        )?;
        let base = match self.quiclb.svcs.get(&name) {
            Some(s) => s.base,
            None => self.quiclb_counters(id),
        };
        self.quiclb.svcs.insert(
            name.clone(),
            Svc {
                id,
                key,
                cfg: QuicLbConfig {
                    iface: iface.clone(),
                    ..c
                },
                backends,
                base,
            },
        );
        self.quiclb.iface = Some(iface.clone());
        if let Err(e) = self.xdp_uplink_set(&iface, XDP_F_QUICLB, true) {
            self.quiclb_remove(&name)?;
            return Err(e);
        }
        Ok(self.quiclb_status())
    }

    pub(super) fn quiclb_status(&mut self) -> QuicLbStatus {
        let list: Vec<(u32, [u64; STATS], QuicLbConfig, Vec<QuicLbBackendStatus>)> = self
            .quiclb
            .svcs
            .values()
            .map(|s| (s.id, s.base, s.cfg.clone(), s.backends.clone()))
            .collect();
        let mut services = Vec::with_capacity(list.len());
        for (id, base, cfg, backends) in list {
            let now = self.quiclb_counters(id);
            let d = |i: u32| now[i as usize].saturating_sub(base[i as usize]);
            services.push(QuicLbServiceStatus {
                vip: canon(&cfg.vip)
                    .map(|a| a.to_string())
                    .unwrap_or(cfg.vip.clone()),
                port: cfg.port,
                mode: cfg.mode,
                cid_len: cfg.cid_len,
                config_id: cfg.config_id,
                backends,
                routed_cid: d(QLB_STAT_CID),
                maglev: d(QLB_STAT_MAGLEV),
                initial: d(QLB_STAT_INITIAL),
                unknown_sid: d(QLB_STAT_UNKNOWN_SID),
                tx: d(QLB_STAT_TX),
                errors: d(QLB_STAT_ERR),
            });
        }
        let attached = self.quiclb.iface.as_deref().is_some_and(|i| {
            self.dp.xdp_attached(i) == Some("mn_xdp_uplink")
                && self.uplink.flags & XDP_F_QUICLB != 0
        });
        QuicLbStatus {
            iface: self.quiclb.iface.clone(),
            attached,
            services,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(backends: &[&str]) -> QuicLbConfig {
        QuicLbConfig {
            iface: "eth0".into(),
            vip: "10.0.0.10".into(),
            port: 443,
            backends: backends
                .iter()
                .map(|a| QuicLbBackend {
                    addr: (*a).into(),
                    ..Default::default()
                })
                .collect(),
            cid_len: 8,
            config_id: 0,
            mode: QuicLbMode::Dsr,
            encap_src: None,
            enabled: true,
        }
    }

    #[test]
    fn quiclb_validation() {
        assert!(validate(&cfg(&["10.0.0.1", "10.0.0.2"])).is_ok());
        assert!(validate(&cfg(&[])).is_err());
        let mut c = cfg(&["10.0.0.1"]);
        c.cid_len = 2;
        assert!(validate(&c).is_err());
        c.cid_len = 8;
        c.config_id = 7;
        assert!(validate(&c).is_err());
        let mut v6 = cfg(&["fd00::1"]);
        v6.mode = QuicLbMode::Ipip;
        assert!(validate(&v6).is_err());
        let mut dup = cfg(&["10.0.0.1", "10.0.0.2"]);
        dup.backends[0].server_id = Some(7);
        dup.backends[1].server_id = Some(7);
        assert!(validate(&dup).is_err());
        assert_eq!(default_server_id("10.0.0.1"), default_server_id("10.0.0.1"));
        assert_ne!(default_server_id("10.0.0.1"), default_server_id("10.0.0.2"));
    }
}
