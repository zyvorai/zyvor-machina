// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Sampled plaintext L7 (`mn_l7s_ingress`/`mn_l7s_egress` cgroup_skb).
//! Samples are decoded to an operation name here and the payload dropped.

use super::*;
use crate::l7sample;

const TAG: &str = "l7s";
const MAX_PORTS: usize = 64;
const MAX_RATE: u32 = 10_000;
const TOP_OPS: usize = 30;

#[derive(Default)]
pub(super) struct L7sRuntime {
    pub config: L7SampleConfig,
    ports: Vec<u16>,
    note: Option<String>,
}

/// Per (protocol, op) sample counts plus undecoded samples.
#[derive(Default)]
pub(super) struct L7sCounts {
    ops: HashMap<(String, String), u64>,
    undecoded: u64,
}

fn resolve_ports(cfg: &L7SampleConfig) -> Result<Vec<(u16, u8)>> {
    if cfg.ports.len() > MAX_PORTS {
        return Err(anyhow!("at most {MAX_PORTS} ports"));
    }
    if cfg.ports.is_empty() {
        return Ok(l7sample::DEFAULT_PORTS
            .iter()
            .map(|(p, n)| (*p, l7sample::proto_id(n).expect("known")))
            .collect());
    }
    cfg.ports
        .iter()
        .map(|p| {
            let id = l7sample::proto_id(&p.protocol).ok_or_else(|| {
                anyhow!(
                    "port {}: protocol must be redis, postgres, mysql, kafka or http2",
                    p.port
                )
            })?;
            if p.port == 0 {
                return Err(anyhow!("port 0 is not a service port"));
            }
            Ok((p.port, id))
        })
        .collect()
}

impl Engine {
    pub(super) fn l7s_configure(&mut self, cfg: L7SampleConfig) -> Result<L7SampleStatus> {
        if !(1..=MAX_RATE).contains(&cfg.rate) {
            return Err(anyhow!("rate must be 1..={MAX_RATE} per second"));
        }
        let ports = resolve_ports(&cfg)?;
        for p in std::mem::take(&mut self.l7s.ports) {
            if !ports.iter().any(|(q, _)| *q == p) {
                self.dp.cni_hash_remove::<u16, u8>("L7S_PORTS", &p);
            }
        }
        for (p, id) in &ports {
            self.dp.cni_hash_insert("L7S_PORTS", *p, *id)?;
        }
        self.l7s.ports = ports.iter().map(|(p, _)| *p).collect();
        self.dp.array_set(
            "L7S_CFG",
            0,
            L7sCfg {
                enabled: cfg.enabled as u32,
                rate: cfg.rate,
                flow_gap_ns: cfg.flow_gap_ms.saturating_mul(1_000_000),
            },
        )?;
        self.l7s.note = None;
        if cfg.enabled && !self.l7s.config.enabled {
            lock(&self.shared).l7s = L7sCounts::default();
        }
        if cfg.enabled {
            let cg = std::env::var_os("MACHINA_BPF_L7S_CGROUP")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(attribution::CGROUP_ROOT));
            if self
                .dp
                .cgroup_tagged(TAG)
                .is_some_and(|c| Path::new(&c) != cg)
            {
                self.dp.detach_cgroup_tag(TAG);
            }
            if let Err(e) =
                self.dp
                    .attach_cgroup_skb_tagged(&cg, TAG, "mn_l7s_ingress", "mn_l7s_egress")
            {
                let n = format!("l7 sampling unavailable: {e:#}");
                self.l7s.note = Some(n.clone());
                self.l7s.config = cfg;
                return Err(anyhow!(n));
            }
        } else {
            self.dp.detach_cgroup_tag(TAG);
        }
        self.l7s.config = cfg;
        Ok(self.l7s_status())
    }

    pub(super) fn l7s_status(&mut self) -> L7SampleStatus {
        let mut sum = |i| {
            self.dp
                .percpu_array_sum::<u64>("L7S_STATS", i, |a, b| *a += *b)
                .unwrap_or(0)
        };
        let (eligible, emitted, rate_limited, ringbuf_full, load_fail) = (
            sum(L7S_STAT_ELIGIBLE),
            sum(L7S_STAT_EMITTED),
            sum(L7S_STAT_RATE_LIMITED),
            sum(L7S_STAT_RINGBUF_FULL),
            sum(L7S_STAT_LOAD_FAIL),
        );
        let (top, undecoded) = {
            let s = lock(&self.shared);
            let mut top: Vec<L7SampleOp> = s
                .l7s
                .ops
                .iter()
                .map(|((protocol, op), n)| L7SampleOp {
                    protocol: protocol.clone(),
                    op: op.clone(),
                    count: *n,
                })
                .collect();
            top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.op.cmp(&b.op)));
            top.truncate(TOP_OPS);
            (top, s.l7s.undecoded)
        };
        L7SampleStatus {
            config: self.l7s.config.clone(),
            attached: self.dp.cgroup_tagged(TAG),
            eligible,
            emitted,
            rate_limited,
            ringbuf_full,
            load_fail,
            undecoded,
            top,
            notes: self.l7s.note.iter().cloned().collect(),
        }
    }
}

pub(super) fn on_l7s(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    if b.len() < std::mem::size_of::<L7sEvent>() {
        return;
    }
    let ev: L7sEvent = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const L7sEvent) };
    let n = (ev.cap_len as usize).min(L7S_COPY);
    let to_server = ev.to_server != 0;
    let protocol = l7sample::proto_name(ev.proto).to_string();
    let decoded = l7sample::decode(ev.proto, to_server, &ev.data[..n]);
    let rec = {
        let mut s = lock(sh);
        let Some(d) = decoded else {
            s.l7s.undecoded += 1;
            return;
        };
        *s.l7s
            .ops
            .entry((protocol.clone(), d.op.clone()))
            .or_default() += 1;
        // Client is whichever side owns the ephemeral port.
        let (client, cport, server, sport) = if to_server {
            (fmt_addr(&ev.src), ev.sport, fmt_addr(&ev.dst), ev.dport)
        } else {
            (fmt_addr(&ev.dst), ev.dport, fmt_addr(&ev.src), ev.sport)
        };
        let cgroup = s.cgroups.lookup(ev.cgroup_id);
        // Local socket sends a request (egress, to server) → it's the client.
        let outbound = (ev.ingress == 0) == to_server;
        let rec = L7Record {
            workload: cgroup.as_deref().and_then(|p| s.cgroup_workload(p)),
            ts: mono_to_rfc3339(ev.ts_ns),
            protocol: protocol.clone(),
            direction: if outbound { "outbound" } else { "inbound" }.into(),
            client,
            client_port: cport,
            server,
            server_port: sport,
            method: Some(if to_server {
                d.op
            } else {
                format!("reply {}", d.op)
            }),
            path: d.detail,
            ..L7Record::default()
        };
        Shared::push_capped(&mut s.l7, rec.clone(), L7_STORE_CAP);
        rec
    };
    publish(bus, "l7", &rec);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_default_and_validate() {
        let c = L7SampleConfig::default();
        assert!(!c.enabled);
        assert_eq!(
            resolve_ports(&c).unwrap().len(),
            l7sample::DEFAULT_PORTS.len()
        );
        let bad = L7SampleConfig {
            ports: vec![L7SamplePort {
                port: 1,
                protocol: "smtp".into(),
            }],
            ..c.clone()
        };
        assert!(resolve_ports(&bad).is_err());
        let ok = L7SampleConfig {
            ports: vec![L7SamplePort {
                port: 6380,
                protocol: "Redis".into(),
            }],
            ..c
        };
        assert_eq!(resolve_ports(&ok).unwrap(), vec![(6380, L7S_REDIS)]);
    }
}
