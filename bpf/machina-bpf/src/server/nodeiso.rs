// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Emergency node isolation: the `XDP_F_NODEISO` bit of the uplink
//! dispatcher (ingress) plus `mn_nodeiso` first on TCX egress. Its lease is
//! separate from the policy lease, mandatory and short; the datapath stops
//! dropping at the deadline by itself and the maintenance tick detaches.

use super::*;
use crate::policy::parse_prefix;

pub(super) const NODEISO_MIN_LEASE: u64 = 10;
pub(super) const NODEISO_MAX_LEASE: u64 = 900;
const MAX_PORTS: usize = 128;
const MAX_EXEMPT: usize = 1024;
const EGRESS_PROG: &str = "mn_nodeiso";

#[derive(Default)]
pub(super) struct NodeIsoRuntime {
    pub config: NodeIsoConfig,
    /// Interface carrying the egress program and the dispatcher bit.
    attached: Option<String>,
    deadline_mono: u64,
    lease_wall: Option<chrono::DateTime<chrono::Utc>>,
    lapsed: bool,
}

/// Validate and normalise a request; returns the lease in seconds when enabling.
pub(super) fn validate(c: &NodeIsoConfig) -> Result<Option<u64>> {
    if !c.enabled {
        return Ok(None);
    }
    if c.iface.is_empty() {
        return Err(anyhow!("node isolation needs `iface` (the uplink)"));
    }
    let secs = c.lease_secs.ok_or_else(|| {
        anyhow!("node isolation needs `lease_secs` ({NODEISO_MIN_LEASE}..={NODEISO_MAX_LEASE})")
    })?;
    if !(NODEISO_MIN_LEASE..=NODEISO_MAX_LEASE).contains(&secs) {
        return Err(anyhow!(
            "lease_secs must be {NODEISO_MIN_LEASE}..={NODEISO_MAX_LEASE}"
        ));
    }
    if c.allow_tcp.len() + c.allow_udp.len() > MAX_PORTS {
        return Err(anyhow!("at most {MAX_PORTS} allowlisted ports"));
    }
    if c.allow_tcp.contains(&0) || c.allow_udp.contains(&0) {
        return Err(anyhow!("port 0 cannot be allowlisted"));
    }
    if c.exempt.len() > MAX_EXEMPT {
        return Err(anyhow!("at most {MAX_EXEMPT} exempt CIDRs"));
    }
    if !c.dry_run && !c.allow_tcp.contains(&22) && c.exempt.is_empty() {
        return Err(anyhow!(
            "refusing to isolate without SSH (22) or an exempt CIDR; you would lock yourself out"
        ));
    }
    Ok(Some(secs))
}

impl Engine {
    pub(super) fn nodeiso_configure(&mut self, mut config: NodeIsoConfig) -> Result<NodeIsoStatus> {
        if config.iface.is_empty() {
            config.iface = self.nodeiso.config.iface.clone();
        }
        let Some(secs) = validate(&config)? else {
            self.nodeiso_off(false)?;
            self.nodeiso.config = config;
            return Ok(self.nodeiso_status());
        };
        let exempt: Vec<crate::policy::Prefix> = config
            .exempt
            .iter()
            .map(|s| parse_prefix(s).map_err(|e| anyhow!("exempt `{s}`: {e}")))
            .collect::<Result<_>>()?;
        if let Some(old) = self.nodeiso.attached.clone() {
            if old != config.iface {
                self.nodeiso_off(false)?;
            }
        }

        self.dp.hash_clear::<u32, u8>("NODEISO_PORTS");
        for p in &config.allow_tcp {
            self.dp
                .cni_hash_insert("NODEISO_PORTS", nodeiso_port_key(6, *p), 1u8)?;
        }
        for p in &config.allow_udp {
            self.dp
                .cni_hash_insert("NODEISO_PORTS", nodeiso_port_key(17, *p), 1u8)?;
        }
        self.dp.addr_lpm_clear::<u8>("NODEISO_EXEMPT")?;
        for p in &exempt {
            self.dp
                .addr_lpm_insert("NODEISO_EXEMPT", p.addr, p.bits, 1u8)?;
        }
        let deadline = loader::monotonic_ns() + secs * 1_000_000_000;
        self.dp.array_set(
            "NODEISO_CFG",
            0,
            NodeIsoCfg {
                enabled: 1,
                dry_run: config.dry_run as u32,
                allow_icmp: config.allow_icmp as u32,
                _pad: 0,
                deadline_ns: deadline,
            },
        )?;
        let iface = config.iface.clone();
        let attach = (|| -> Result<()> {
            self.dp.attach_tc_first(&iface, EGRESS_PROG, false)?;
            self.xdp_uplink_set(&iface, XDP_F_NODEISO, true)
        })();
        if let Err(e) = attach {
            self.nodeiso.attached = Some(iface);
            let _ = self.nodeiso_off(false);
            return Err(e);
        }
        tracing::warn!(iface = %iface, secs, dry_run = config.dry_run, "node isolation armed");
        self.nodeiso.attached = Some(iface);
        self.nodeiso.deadline_mono = deadline;
        self.nodeiso.lease_wall = Some(chrono::Utc::now() + chrono::Duration::seconds(secs as i64));
        self.nodeiso.lapsed = false;
        self.nodeiso.config = config;
        Ok(self.nodeiso_status())
    }

    /// Zero the config (fail open first), then detach both halves.
    fn nodeiso_off(&mut self, lapsed: bool) -> Result<()> {
        self.dp.array_set("NODEISO_CFG", 0, NodeIsoCfg::default())?;
        if let Some(iface) = self.nodeiso.attached.take() {
            self.dp.detach_tc_one(&iface, EGRESS_PROG);
            if self.uplink.flags & XDP_F_NODEISO != 0 {
                self.xdp_uplink_set(&iface, XDP_F_NODEISO, false)?;
            }
        }
        self.nodeiso.deadline_mono = 0;
        self.nodeiso.lease_wall = None;
        self.nodeiso.lapsed = lapsed;
        self.nodeiso.config.enabled = false;
        Ok(())
    }

    /// Maintenance: tear down isolation whose lease ran out.
    pub(super) fn nodeiso_expire(&mut self) -> Result<()> {
        if self.nodeiso.attached.is_some() && loader::monotonic_ns() >= self.nodeiso.deadline_mono {
            tracing::warn!("node isolation lease lapsed; detaching");
            self.nodeiso_off(true)?;
        }
        Ok(())
    }

    pub(super) fn nodeiso_status(&mut self) -> NodeIsoStatus {
        let s: NodeIsoStats = self
            .dp
            .percpu_array_sum(
                "NODEISO_STATS",
                0,
                |a: &mut NodeIsoStats, b: &NodeIsoStats| {
                    a.checked += b.checked;
                    a.passed += b.passed;
                    a.dropped_in += b.dropped_in;
                    a.dropped_out += b.dropped_out;
                    a.would_drop += b.would_drop;
                },
            )
            .unwrap_or_default();
        let now = loader::monotonic_ns();
        let live = self.nodeiso.attached.is_some() && now < self.nodeiso.deadline_mono;
        NodeIsoStatus {
            config: self.nodeiso.config.clone(),
            attached: self.nodeiso.attached.clone(),
            isolating: live && !self.nodeiso.config.dry_run,
            lease_expires_at: live
                .then(|| self.nodeiso.lease_wall.map(|w| w.to_rfc3339()))
                .flatten(),
            lease_remaining_secs: live.then(|| (self.nodeiso.deadline_mono - now) / 1_000_000_000),
            lease_expired: self.nodeiso.lapsed,
            stats: NodeIsoCounters {
                checked: s.checked,
                passed: s.passed,
                dropped_in: s.dropped_in,
                dropped_out: s.dropped_out,
                would_drop: s.would_drop,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(lease: Option<u64>) -> NodeIsoConfig {
        NodeIsoConfig {
            enabled: true,
            iface: "eth0".into(),
            lease_secs: lease,
            ..NodeIsoConfig::default()
        }
    }

    #[test]
    fn nodeiso_needs_a_short_lease_and_ssh() {
        let d = NodeIsoConfig::default();
        assert!(
            !d.enabled && d.allow_icmp && d.allow_tcp.contains(&22) && d.allow_tcp.contains(&6443)
        );
        assert_eq!(validate(&d).unwrap(), None);
        assert!(validate(&on(None)).is_err());
        assert!(validate(&on(Some(5))).is_err());
        assert!(validate(&on(Some(3600))).is_err());
        assert_eq!(validate(&on(Some(120))).unwrap(), Some(120));
        let mut no_ssh = on(Some(60));
        no_ssh.allow_tcp = vec![6443];
        assert!(validate(&no_ssh).is_err());
        no_ssh.exempt = vec!["10.0.0.0/8".into()];
        assert!(validate(&no_ssh).is_ok());
        let mut no_iface = on(Some(60));
        no_iface.iface.clear();
        assert!(validate(&no_iface).is_err());
    }
}
