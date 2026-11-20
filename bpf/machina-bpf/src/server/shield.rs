// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! XDP DDoS shield: configuration of the `SHIELD_*` maps and the shield bit
//! of the uplink dispatcher.

use std::collections::HashSet;

use super::cni::addr16;
use super::*;
use crate::policy::parse_prefix;

const MAX_PPS: u32 = 1_000_000;
const MAX_ENTRIES: usize = 8192;
const TOP_SOURCES: usize = 50;

#[derive(Default)]
pub(super) struct ShieldRuntime {
    pub config: ShieldConfig,
    protected: HashSet<[u8; ADDR_LEN]>,
}

fn shield_mode(m: &str) -> Result<u32> {
    match m {
        "off" => Ok(SHIELD_OFF),
        "audit" => Ok(SHIELD_AUDIT),
        "enforce" => Ok(SHIELD_ENFORCE),
        _ => Err(anyhow!("shield mode `{m}`: expected off, audit or enforce")),
    }
}

fn class_name(c: u8) -> &'static str {
    match c {
        SHIELD_CLASS_SYN => "syn",
        SHIELD_CLASS_UDP => "udp",
        SHIELD_CLASS_ICMP => "icmp",
        _ => "other",
    }
}

fn prefixes(what: &str, v: &[String]) -> Result<Vec<crate::policy::Prefix>> {
    if v.len() > MAX_ENTRIES {
        return Err(anyhow!("at most {MAX_ENTRIES} {what} entries"));
    }
    v.iter()
        .map(|s| parse_prefix(s).map_err(|e| anyhow!("{what} `{s}`: {e}")))
        .collect()
}

impl Engine {
    pub(super) fn shield_configure(&mut self, mut config: ShieldConfig) -> Result<ShieldStatus> {
        let mode = shield_mode(&config.mode)?;
        if config.iface.is_empty() {
            config.iface = self.shield.config.iface.clone();
        }
        if mode != SHIELD_OFF && config.iface.is_empty() {
            return Err(anyhow!("shield needs `iface` (the uplink)"));
        }
        for (name, v) in [
            ("syn_pps", config.syn_pps),
            ("udp_pps", config.udp_pps),
            ("icmp_pps", config.icmp_pps),
            ("other_pps", config.other_pps),
        ] {
            if v > MAX_PPS {
                return Err(anyhow!("{name} must be at most {MAX_PPS}"));
            }
        }
        if !(1..=60).contains(&config.burst_secs) {
            return Err(anyhow!("burst_secs must be 1..=60"));
        }
        if config.protected.len() > MAX_ENTRIES {
            return Err(anyhow!("at most {MAX_ENTRIES} protected addresses"));
        }
        let protected: HashSet<[u8; ADDR_LEN]> = config
            .protected
            .iter()
            .map(|a| addr16(a))
            .collect::<Result<_>>()?;
        if mode != SHIELD_OFF && !config.protect_all && protected.is_empty() {
            return Err(anyhow!(
                "nothing to protect: set `protected` addresses or `protect_all`"
            ));
        }
        let allow = prefixes("allow", &config.allow)?;
        let deny = prefixes("deny", &config.deny)?;

        // Moving to another interface: drop the shield bit from the old one.
        let old = self.shield.config.iface.clone();
        if !old.is_empty() && old != config.iface && self.uplink.flags & XDP_F_SHIELD != 0 {
            self.xdp_uplink_set(&old, XDP_F_SHIELD, false)?;
        }

        self.dp.array_set(
            "SHIELD_CFG",
            0,
            ShieldCfg {
                mode,
                protect_all: config.protect_all as u32,
                pps: [
                    config.syn_pps,
                    config.udp_pps,
                    config.icmp_pps,
                    config.other_pps,
                ],
                burst_secs: config.burst_secs,
                _pad: 0,
            },
        )?;
        for k in self.shield.protected.clone() {
            if !protected.contains(&k) {
                self.dp
                    .cni_hash_remove::<[u8; ADDR_LEN], u8>("SHIELD_PROTECTED", &k);
            }
        }
        for k in &protected {
            self.dp.cni_hash_insert("SHIELD_PROTECTED", *k, 1u8)?;
        }
        self.dp.addr_lpm_clear::<u8>("SHIELD_ALLOW")?;
        self.dp.addr_lpm_clear::<u8>("SHIELD_DENY")?;
        for p in &allow {
            self.dp
                .addr_lpm_insert("SHIELD_ALLOW", p.addr, p.bits, 1u8)?;
        }
        for p in &deny {
            self.dp
                .addr_lpm_insert("SHIELD_DENY", p.addr, p.bits, 1u8)?;
        }
        self.shield.protected = protected;

        if mode != SHIELD_OFF || self.uplink.flags & XDP_F_SHIELD != 0 {
            self.xdp_uplink_set(&config.iface, XDP_F_SHIELD, mode != SHIELD_OFF)?;
        }
        self.shield.config = config;
        Ok(self.shield_status())
    }

    pub(super) fn shield_status(&mut self) -> ShieldStatus {
        let s: ShieldStats = self
            .dp
            .percpu_array_sum("SHIELD_STATS", 0, |a: &mut ShieldStats, b: &ShieldStats| {
                a.checked += b.checked;
                a.passed += b.passed;
                a.audited += b.audited;
                a.dropped += b.dropped;
                a.dropped_bytes += b.dropped_bytes;
                a.denied += b.denied;
                a.malformed += b.malformed;
                for i in 0..SHIELD_CLASSES {
                    a.limited[i] += b.limited[i];
                }
            })
            .unwrap_or_default();
        let entries = self
            .dp
            .hash_entries::<ShieldSrcKey, ShieldSrcState>("SHIELD_SOURCES")
            .unwrap_or_default();
        let tracked_sources = entries.len();
        let mut sources: Vec<ShieldSource> = entries
            .into_iter()
            .filter(|(_, v)| v.hits > 0)
            .map(|(k, v)| ShieldSource {
                addr: fmt_addr(&k.addr),
                class: class_name(k.class).into(),
                hits: v.hits,
            })
            .collect();
        sources.sort_by_key(|s| std::cmp::Reverse(s.hits));
        sources.truncate(TOP_SOURCES);
        let on = self.uplink.flags & XDP_F_SHIELD != 0;
        ShieldStatus {
            config: self.shield.config.clone(),
            attached: on.then(|| self.uplink.iface.clone()).flatten(),
            enforcing: on && self.shield.config.mode == "enforce" && self.lease_live(),
            stats: ShieldCounters {
                checked: s.checked,
                passed: s.passed,
                audited: s.audited,
                dropped: s.dropped,
                dropped_bytes: s.dropped_bytes,
                denied: s.denied,
                malformed: s.malformed,
                syn_limited: s.limited[SHIELD_CLASS_SYN as usize],
                udp_limited: s.limited[SHIELD_CLASS_UDP as usize],
                icmp_limited: s.limited[SHIELD_CLASS_ICMP as usize],
                other_limited: s.limited[SHIELD_CLASS_OTHER as usize],
            },
            sources,
            tracked_sources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shield_defaults_and_modes() {
        let c = ShieldConfig::default();
        assert_eq!(
            (c.mode.as_str(), c.syn_pps, c.icmp_pps, c.burst_secs),
            ("off", 1000, 100, 2)
        );
        assert_eq!(shield_mode("enforce").unwrap(), SHIELD_ENFORCE);
        assert!(shield_mode("drop").is_err());
        assert_eq!(class_name(SHIELD_CLASS_SYN), "syn");
        assert!(prefixes("deny", &["10.0.0.0/8".into(), "2001:db8::/32".into()]).is_ok());
        assert!(prefixes("deny", &["10.0.0.0/33".into()]).is_err());
    }
}
