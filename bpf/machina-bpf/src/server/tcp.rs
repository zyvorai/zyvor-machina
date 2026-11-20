// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! TCP connect latency / pressure (`mn_sockops`) and ICMP error counters.

use super::*;

const TOP: usize = 200;

/// Upper bound of the bucket holding the 90th percentile.
pub(crate) fn p90_bucket(hist: &[u64]) -> Option<u64> {
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return None;
    }
    let want = total.saturating_mul(9).div_ceil(10);
    let mut seen = 0;
    for (i, n) in hist.iter().enumerate() {
        seen += n;
        if seen >= want {
            return CONNECT_BUCKETS_US.get(i).copied();
        }
    }
    None
}

fn icmp_kind(k: u8) -> &'static str {
    match k {
        ICMP_ERR_UNREACH => "unreachable",
        ICMP_ERR_TIME_EXCEEDED => "time_exceeded",
        ICMP_ERR_PARAM => "param_problem",
        ICMP_ERR_PTB => "packet_too_big",
        _ => "other",
    }
}

impl Engine {
    /// Attach or detach `mn_sockops` per the `tcp` telemetry switch.
    pub(super) fn sync_sockops(&mut self) {
        if !(self.telemetry.tcp && self.features.cgroup2) {
            self.dp.detach_sockops();
            return;
        }
        let cg = std::env::var_os("MACHINA_BPF_SOCKOPS_CGROUP")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(attribution::CGROUP_ROOT));
        if let Err(e) = self.dp.attach_sockops(&cg) {
            let n = format!("tcp connect/pressure telemetry unavailable: {e:#}");
            tracing::warn!("{n}");
            if !self.dp.notes.contains(&n) {
                self.dp.notes.push(n);
            }
        }
    }

    pub(super) fn connect_health(&mut self) -> Vec<ConnectHealth> {
        let mut out: Vec<ConnectHealth> = self
            .dp
            .hash_entries::<ConnKey, ConnStats>("CONNECT_HEALTH")
            .unwrap_or_default()
            .into_iter()
            .map(|(k, s)| ConnectHealth {
                addr: fmt_addr(&k.addr),
                port: k.port,
                count: s.count,
                failures: s.failures,
                avg_us: s.sum_us.checked_div(s.count).unwrap_or(0),
                max_us: s.max_us,
                p90_le_us: p90_bucket(&s.hist),
                hist: s.hist.to_vec(),
            })
            .collect();
        out.sort_by_key(|c| std::cmp::Reverse(c.count + c.failures));
        out.truncate(TOP);
        out
    }

    pub(super) fn tcp_pressure(&mut self) -> Vec<TcpPeerPressure> {
        let now = loader::monotonic_ns();
        let mut out: Vec<(u64, TcpPeerPressure)> = self
            .dp
            .hash_entries::<[u8; ADDR_LEN], TcpPressure>("TCP_PRESSURE")
            .unwrap_or_default()
            .into_iter()
            .map(|(a, p)| {
                let rate = if p.rate_interval_us > 0 {
                    p.rate_delivered as u64 * p.mss as u64 * 1_000_000 / p.rate_interval_us as u64
                } else {
                    0
                };
                (
                    p.last_ns,
                    TcpPeerPressure {
                        addr: fmt_addr(&a),
                        srtt_us: p.srtt_us8 >> 3,
                        cwnd: p.cwnd,
                        ssthresh: p.ssthresh,
                        mss: p.mss,
                        total_retrans: p.total_retrans,
                        retrans_events: p.retrans_events,
                        delivery_rate_bps: rate,
                        age_secs: now.saturating_sub(p.last_ns) / 1_000_000_000,
                    },
                )
            })
            .collect();
        out.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
        out.truncate(TOP);
        out.into_iter().map(|(_, p)| p).collect()
    }

    pub(super) fn icmp_errors(&mut self) -> Vec<IcmpError> {
        let mut out: Vec<IcmpError> = self
            .dp
            .hash_entries::<IcmpErrKey, u64>("ICMP_ERRORS")
            .unwrap_or_default()
            .into_iter()
            .map(|(k, count)| {
                let r = self.ifaces.get(&k.ifindex);
                IcmpError {
                    iface: r
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| format!("if{}", k.ifindex)),
                    vm: r.and_then(|r| r.vm.clone()),
                    kind: icmp_kind(k.kind).into(),
                    code: k.code,
                    family: if k.v6 != 0 { "ipv6" } else { "ipv4" }.into(),
                    direction: if k.from_workload != 0 {
                        "from_workload"
                    } else {
                        "to_workload"
                    }
                    .into(),
                    count,
                }
            })
            .collect();
        out.sort_by_key(|e| std::cmp::Reverse(e.count));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p90_picks_bucket() {
        assert_eq!(p90_bucket(&[0; CONNECT_BUCKETS]), None);
        assert_eq!(p90_bucket(&[10, 0, 0, 0, 0, 0, 0, 0]), Some(100));
        assert_eq!(p90_bucket(&[5, 4, 1, 0, 0, 0, 0, 0]), Some(1_000));
        assert_eq!(p90_bucket(&[0, 0, 0, 0, 0, 0, 0, 3]), None);
        assert_eq!(icmp_kind(ICMP_ERR_PTB), "packet_too_big");
    }
}
