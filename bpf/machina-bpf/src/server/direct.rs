// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Bridge-less direct redirect (`mn_direct` on the outer device's ingress,
//! `mn_direct_out` on the tap's ingress), per VM, under the enforcement lease.

use std::collections::BTreeMap;

use super::cni::addr16;
use super::*;

const IN: &str = "mn_direct";
const OUT: &str = "mn_direct_out";

/// (entry, mac key, ip keys, tap ifindex)
type DirectSlot = (DirectEntry, u64, Vec<[u8; ADDR_LEN]>, u32);

#[derive(Default)]
pub(super) struct DirectRuntime {
    entries: BTreeMap<String, DirectSlot>,
}

pub(crate) fn parse_mac(s: &str) -> Result<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return Err(anyhow!("bad MAC `{s}`"));
    }
    let mut m = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        m[i] = u8::from_str_radix(p, 16).map_err(|_| anyhow!("bad MAC `{s}`"))?;
    }
    Ok(m)
}

pub(super) fn fmt_mac(m: &[u8; 6]) -> String {
    m.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(super) fn link_mac(iface: &str) -> Option<[u8; 6]> {
    parse_mac(
        std::fs::read_to_string(format!("/sys/class/net/{iface}/address"))
            .ok()?
            .trim(),
    )
    .ok()
}

/// A NIC backed by a bus device (not veth/tap/bridge/dummy).
fn is_physical(iface: &str) -> bool {
    Path::new(&format!("/sys/class/net/{iface}/device")).exists()
}

impl Engine {
    pub(super) fn direct_push_cfg(&mut self) -> Result<()> {
        let live = self.lease_live() && !self.direct.entries.is_empty();
        self.dp.array_set(
            "DIRECT_CFG",
            0,
            DirectCfg {
                enabled: !self.direct.entries.is_empty() as u32,
                _pad: 0,
                lease_deadline_ns: if live { self.lease_deadline_mono } else { 0 },
            },
        )
    }

    fn direct_remove(&mut self, vm: &str) {
        let Some((e, mac, ips, tap_idx)) = self.direct.entries.remove(vm) else {
            return;
        };
        self.dp.cni_hash_remove::<u64, u32>("DIRECT_MAC", &mac);
        for ip in &ips {
            self.dp
                .cni_hash_remove::<[u8; ADDR_LEN], u32>("DIRECT_IP", ip);
        }
        self.dp.cni_hash_remove::<u32, u32>("DIRECT_OUT", &tap_idx);
        self.dp.detach_tc_one(&e.tap, OUT);
        if !self
            .direct
            .entries
            .values()
            .any(|(o, ..)| o.outer_iface == e.outer_iface)
        {
            self.dp.detach_tc_one(&e.outer_iface, IN);
        }
    }

    pub(super) fn direct_configure(&mut self, c: DirectConfig) -> Result<DirectStatus> {
        if c.vm.is_empty() {
            return Err(anyhow!("vm is required"));
        }
        if !c.enabled {
            self.direct_remove(&c.vm);
            self.direct_push_cfg()?;
            return Ok(self.direct_status());
        }
        let outer_idx = if_nametoindex(&c.outer_iface)
            .ok_or_else(|| anyhow!("outer interface `{}` not found", c.outer_iface))?;
        if is_physical(&c.outer_iface) && !c.force {
            return Err(anyhow!(
                "`{}` is a physical NIC; redirecting its traffic can cut the host off. Pass force=true to use it",
                c.outer_iface
            ));
        }
        let tap = match &c.tap {
            Some(t) => t.clone(),
            None => self
                .ifaces
                .values()
                .find(|r| r.vm.as_deref() == Some(c.vm.as_str()))
                .map(|r| r.name.clone())
                .ok_or_else(|| anyhow!("no tap known for VM {}; pass tap", c.vm))?,
        };
        if tap == c.outer_iface {
            return Err(anyhow!("outer interface and tap must differ"));
        }
        let tap_idx = if_nametoindex(&tap).ok_or_else(|| anyhow!("tap `{tap}` not found"))?;
        let mac = match &c.mac {
            Some(m) => parse_mac(m)?,
            None => {
                let mut m =
                    link_mac(&tap).ok_or_else(|| anyhow!("cannot read MAC of {tap}; pass mac"))?;
                if m[0] == 0xfe {
                    m[0] = 0x52;
                }
                m
            }
        };
        let ips: Vec<[u8; ADDR_LEN]> = c.ips.iter().map(|s| addr16(s)).collect::<Result<_>>()?;

        self.direct_remove(&c.vm);
        self.dp
            .cni_hash_insert("DIRECT_MAC", mac_key(&mac), tap_idx)?;
        for ip in &ips {
            self.dp.cni_hash_insert("DIRECT_IP", *ip, tap_idx)?;
        }
        if c.reverse {
            self.dp.cni_hash_insert("DIRECT_OUT", tap_idx, outer_idx)?;
        }
        let entry = DirectEntry {
            vm: c.vm.clone(),
            outer_iface: c.outer_iface.clone(),
            tap: tap.clone(),
            mac: fmt_mac(&mac),
            ips: c.ips.clone(),
            reverse: c.reverse,
        };
        self.direct
            .entries
            .insert(c.vm.clone(), (entry, mac_key(&mac), ips, tap_idx));
        let attach = self
            .dp
            .attach_tc_one(&c.outer_iface, IN, true)
            .and_then(|_| {
                if c.reverse {
                    self.dp.attach_tc_one(&tap, OUT, true)
                } else {
                    Ok(())
                }
            });
        if let Err(e) = attach {
            self.direct_remove(&c.vm);
            self.direct_push_cfg()?;
            return Err(e);
        }
        self.direct_push_cfg()?;
        Ok(self.direct_status())
    }

    pub(super) fn direct_status(&mut self) -> DirectStatus {
        let mut sum = |i| {
            self.dp
                .percpu_array_sum::<u64>("DIRECT_STATS", i, |a, b| *a += *b)
                .unwrap_or(0)
        };
        let (redirected_in, redirected_out, idle) = (
            sum(DIRECT_STAT_IN),
            sum(DIRECT_STAT_OUT),
            sum(DIRECT_STAT_IDLE),
        );
        let mut attached = Vec::new();
        for (e, ..) in self.direct.entries.values() {
            for (iface, prog) in [(&e.outer_iface, IN), (&e.tap, OUT)] {
                let k = format!("{iface}:{prog}");
                if self.dp.tc_one_attached(iface, prog) && !attached.contains(&k) {
                    attached.push(k);
                }
            }
        }
        DirectStatus {
            entries: self
                .direct
                .entries
                .values()
                .map(|(e, ..)| e.clone())
                .collect(),
            attached,
            active: self.lease_live() && !self.direct.entries.is_empty(),
            redirected_in,
            redirected_out,
            idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs() {
        let m = parse_mac("52:54:00:AA:bb:0c").unwrap();
        assert_eq!(fmt_mac(&m), "52:54:00:aa:bb:0c");
        assert!(parse_mac("52:54:00").is_err());
        assert!(parse_mac("zz:54:00:aa:bb:cc").is_err());
    }
}
