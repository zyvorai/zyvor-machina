// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Streaming anomaly detectors over native flow / process / DNS events.
//! Time is passed in (seconds) so detectors are deterministic under test.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::json;

use crate::api::Anomaly;

pub const SCAN_WINDOW_SECS: f64 = 60.0;
pub const SCAN_DISTINCT_PORTS: usize = 50;
pub const BEACON_MIN_SAMPLES: usize = 6;
pub const BEACON_MAX_CV: f64 = 0.1;
pub const VOLUME_BYTES_PER_MIN: u64 = 500 * 1024 * 1024;
pub const DENY_BURST: usize = 20;
pub const NEW_DEST_LEARNING_SECS: f64 = 600.0;
const ALERT_COOLDOWN_SECS: f64 = 600.0;

#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub vm: Option<String>,
    pub iface: Option<String>,
}

#[derive(Default)]
pub struct Detector {
    started: Option<f64>,
    seq: u64,
    /// (local, remote) → recent (ts, port)
    outbound_ports: HashMap<(String, String), VecDeque<(f64, u16)>>,
    /// (remote, local) → recent (ts, port)
    inbound_ports: HashMap<(String, String), VecDeque<(f64, u16)>>,
    /// (local, remote, port) → flow-open timestamps
    beacons: HashMap<(String, String, u16), VecDeque<f64>>,
    /// local → known remotes
    known_dests: HashMap<String, HashSet<String>>,
    /// flow key → (ts, tx_bytes)
    volume: HashMap<String, (f64, u64)>,
    denies: HashMap<String, VecDeque<f64>>,
    cooldown: HashMap<String, f64>,
}

fn entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

impl Detector {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        now: f64,
        dedupe: String,
        kind: &str,
        severity: &str,
        summary: String,
        ctx: &Ctx,
        local: Option<&str>,
        remote: Option<&str>,
        details: serde_json::Value,
    ) -> Option<Anomaly> {
        if let Some(t) = self.cooldown.get(&dedupe) {
            if now - t < ALERT_COOLDOWN_SECS {
                return None;
            }
        }
        self.cooldown.insert(dedupe, now);
        self.seq += 1;
        Some(Anomaly {
            id: format!("bpf-{}-{}", now as u64, self.seq),
            ts: chrono::DateTime::from_timestamp(now as i64, 0)
                .unwrap_or_default()
                .to_rfc3339(),
            kind: kind.into(),
            severity: severity.into(),
            summary,
            vm: ctx.vm.clone(),
            iface: ctx.iface.clone(),
            local: local.map(String::from),
            remote: remote.map(String::from),
            details,
        })
    }

    fn window(q: &mut VecDeque<(f64, u16)>, now: f64, port: u16) -> usize {
        q.push_back((now, port));
        while q
            .front()
            .map(|(t, _)| now - t > SCAN_WINDOW_SECS)
            .unwrap_or(false)
        {
            q.pop_front();
        }
        while q.len() > 4096 {
            q.pop_front();
        }
        q.iter().map(|(_, p)| *p).collect::<HashSet<_>>().len()
    }

    /// A new flow. `origin_local` = the workload initiated it.
    #[allow(clippy::too_many_arguments)]
    pub fn on_flow_open(
        &mut self,
        now: f64,
        ctx: &Ctx,
        local: &str,
        remote: &str,
        remote_port: u16,
        local_port: u16,
        origin_local: bool,
    ) -> Vec<Anomaly> {
        let started = *self.started.get_or_insert(now);
        let mut out = Vec::new();
        if origin_local {
            let q = self
                .outbound_ports
                .entry((local.into(), remote.into()))
                .or_default();
            let distinct = Self::window(q, now, remote_port);
            if distinct >= SCAN_DISTINCT_PORTS {
                out.extend(self.emit(
                    now,
                    format!("scan:{local}:{remote}"),
                    "port_scan",
                    "high",
                    format!(
                        "{local} probed {distinct} ports on {remote} within {SCAN_WINDOW_SECS}s"
                    ),
                    ctx,
                    Some(local),
                    Some(remote),
                    json!({ "distinct_ports": distinct }),
                ));
            }

            let beacon = {
                let ts = self
                    .beacons
                    .entry((local.into(), remote.into(), remote_port))
                    .or_default();
                ts.push_back(now);
                while ts.len() > 32 {
                    ts.pop_front();
                }
                if ts.len() >= BEACON_MIN_SAMPLES {
                    let iv: Vec<f64> = ts
                        .iter()
                        .zip(ts.iter().skip(1))
                        .map(|(a, b)| b - a)
                        .collect();
                    let mean = iv.iter().sum::<f64>() / iv.len() as f64;
                    let var = iv.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / iv.len() as f64;
                    let cv = if mean > 0.0 { var.sqrt() / mean } else { 1.0 };
                    ((10.0..=3600.0).contains(&mean) && cv < BEACON_MAX_CV).then_some((
                        mean,
                        cv,
                        ts.len(),
                    ))
                } else {
                    None
                }
            };
            if let Some((mean, cv, samples)) = beacon {
                out.extend(self.emit(
                    now,
                    format!("beacon:{local}:{remote}:{remote_port}"),
                    "beaconing",
                    "high",
                    format!(
                        "{local} contacts {remote}:{remote_port} every {mean:.0}s (jitter {:.0}%)",
                        cv * 100.0
                    ),
                    ctx,
                    Some(local),
                    Some(remote),
                    json!({ "interval_secs": mean, "cv": cv, "samples": samples }),
                ));
            }

            let known = self.known_dests.entry(local.into()).or_default();
            let is_new = known.insert(remote.into());
            if known.len() > 10_000 {
                known.clear();
            }
            if is_new && now - started > NEW_DEST_LEARNING_SECS {
                out.extend(self.emit(
                    now,
                    format!("newdest:{local}:{remote}"),
                    "new_destination",
                    "low",
                    format!("{local} contacted a new destination {remote}:{remote_port}"),
                    ctx,
                    Some(local),
                    Some(remote),
                    json!({ "remote_port": remote_port }),
                ));
            }
        } else {
            let q = self
                .inbound_ports
                .entry((remote.into(), local.into()))
                .or_default();
            let distinct = Self::window(q, now, local_port);
            if distinct >= SCAN_DISTINCT_PORTS {
                out.extend(self.emit(
                    now,
                    format!("inscan:{remote}:{local}"),
                    "inbound_scan",
                    "medium",
                    format!(
                        "{remote} probed {distinct} ports on {local} within {SCAN_WINDOW_SECS}s"
                    ),
                    ctx,
                    Some(local),
                    Some(remote),
                    json!({ "distinct_ports": distinct }),
                ));
            }
        }
        out
    }

    /// Periodic per-flow byte counters (cumulative tx from the workload).
    pub fn on_flow_bytes(
        &mut self,
        now: f64,
        ctx: &Ctx,
        flow_key: &str,
        local: &str,
        remote: &str,
        tx_bytes: u64,
    ) -> Option<Anomaly> {
        let prev = self.volume.insert(flow_key.into(), (now, tx_bytes));
        let (t0, b0) = prev?;
        let dt = now - t0;
        if dt <= 0.0 || tx_bytes < b0 {
            return None;
        }
        let per_min = ((tx_bytes - b0) as f64 / dt * 60.0) as u64;
        if per_min < VOLUME_BYTES_PER_MIN {
            return None;
        }
        self.emit(
            now,
            format!("volume:{flow_key}"),
            "egress_volume_spike",
            "medium",
            format!(
                "{local} → {remote} sending {} MiB/min",
                per_min / (1024 * 1024)
            ),
            ctx,
            Some(local),
            Some(remote),
            json!({ "bytes_per_min": per_min }),
        )
    }

    pub fn prune(&mut self, now: f64) {
        self.volume.retain(|_, (t, _)| now - *t < 600.0);
        self.beacons
            .retain(|_, q| q.back().map(|t| now - t < 7200.0).unwrap_or(false));
        self.outbound_ports.retain(|_, q| {
            q.back()
                .map(|(t, _)| now - t < SCAN_WINDOW_SECS)
                .unwrap_or(false)
        });
        self.inbound_ports.retain(|_, q| {
            q.back()
                .map(|(t, _)| now - t < SCAN_WINDOW_SECS)
                .unwrap_or(false)
        });
        self.denies.retain(|_, q| {
            q.back()
                .map(|t| now - t < SCAN_WINDOW_SECS)
                .unwrap_or(false)
        });
        self.cooldown.retain(|_, t| now - *t < ALERT_COOLDOWN_SECS);
    }

    pub fn on_deny(&mut self, now: f64, ctx: &Ctx, local: &str, remote: &str) -> Option<Anomaly> {
        let q = self.denies.entry(local.into()).or_default();
        q.push_back(now);
        while q
            .front()
            .map(|t| now - t > SCAN_WINDOW_SECS)
            .unwrap_or(false)
        {
            q.pop_front();
        }
        let n = q.len();
        if n < DENY_BURST {
            return None;
        }
        self.emit(
            now,
            format!("denyburst:{local}"),
            "policy_violation_burst",
            "high",
            format!("{local} hit {n} policy denials in {SCAN_WINDOW_SECS}s (latest {remote})"),
            ctx,
            Some(local),
            Some(remote),
            json!({ "denials": n }),
        )
    }

    pub fn on_exec(
        &mut self,
        now: f64,
        ctx: &Ctx,
        path: &str,
        comm: &str,
        pid: u32,
    ) -> Option<Anomaly> {
        let suspicious_dir = ["/tmp/", "/dev/shm/", "/var/tmp/", "/run/user/"]
            .iter()
            .any(|p| path.starts_with(p));
        let shell = matches!(
            path.rsplit('/').next().unwrap_or(""),
            "sh" | "bash" | "dash" | "zsh" | "ash"
        );
        let hypervisor_parent = comm.starts_with("qemu") || comm.starts_with("libvirt");
        let (kind_sev, why) = if suspicious_dir {
            (
                "high",
                format!("executed {path} from a world-writable directory"),
            )
        } else if shell && hypervisor_parent {
            ("critical", format!("{comm} spawned a shell ({path})"))
        } else {
            return None;
        };
        self.emit(
            now,
            format!("exec:{path}:{comm}"),
            "suspicious_exec",
            kind_sev,
            why,
            ctx,
            None,
            None,
            json!({ "path": path, "comm": comm, "pid": pid }),
        )
    }

    pub fn on_dns_query(
        &mut self,
        now: f64,
        ctx: &Ctx,
        client: &str,
        qname: &str,
    ) -> Option<Anomaly> {
        let first = qname.split('.').next().unwrap_or("");
        let long = first.len() > 50 || qname.len() > 180;
        let high_entropy = first.len() >= 24 && entropy(first) > 4.0;
        if !long && !high_entropy {
            return None;
        }
        let parent: String = qname.split('.').rev().take(2).collect::<Vec<_>>().join(".");
        self.emit(
            now,
            format!("dnstun:{client}:{parent}"),
            "dns_tunneling",
            "medium",
            format!("{client} queried an unusually long/random name under {parent}"),
            ctx,
            Some(client),
            None,
            json!({ "qname": qname, "label_len": first.len(), "entropy": entropy(first) }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_port_scan_once() {
        let mut d = Detector::new();
        let c = Ctx::default();
        let mut hits = 0;
        for p in 0..80u16 {
            hits += d
                .on_flow_open(
                    100.0 + p as f64 * 0.1,
                    &c,
                    "10.0.0.5",
                    "10.0.0.9",
                    1000 + p,
                    40000,
                    true,
                )
                .iter()
                .filter(|a| a.kind == "port_scan")
                .count();
        }
        assert_eq!(hits, 1, "cooldown should dedupe");
    }

    #[test]
    fn detects_beaconing_not_jitter() {
        let mut d = Detector::new();
        let c = Ctx::default();
        let mut found = false;
        for i in 0..8 {
            let a = d.on_flow_open(
                1000.0 + i as f64 * 60.0,
                &c,
                "10.0.0.5",
                "203.0.113.7",
                443,
                5000,
                true,
            );
            found |= a.iter().any(|a| a.kind == "beaconing");
        }
        assert!(found);
        let mut d = Detector::new();
        let jitter = [0.0, 13.0, 95.0, 120.0, 400.0, 410.0, 900.0, 1500.0];
        let any = jitter.iter().any(|t| {
            d.on_flow_open(*t, &c, "a", "b", 443, 1, true)
                .iter()
                .any(|a| a.kind == "beaconing")
        });
        assert!(!any);
    }

    #[test]
    fn new_destination_after_learning() {
        let mut d = Detector::new();
        let c = Ctx::default();
        assert!(d.on_flow_open(0.0, &c, "a", "b", 80, 1, true).is_empty());
        let a = d.on_flow_open(NEW_DEST_LEARNING_SECS + 1.0, &c, "a", "c", 80, 1, true);
        assert!(a.iter().any(|x| x.kind == "new_destination"));
        let again = d.on_flow_open(NEW_DEST_LEARNING_SECS + 2.0, &c, "a", "c", 80, 1, true);
        assert!(!again.iter().any(|x| x.kind == "new_destination"));
    }

    #[test]
    fn volume_and_denies_and_exec_and_dns() {
        let mut d = Detector::new();
        let c = Ctx::default();
        assert!(d.on_flow_bytes(0.0, &c, "k", "a", "b", 0).is_none());
        assert!(d
            .on_flow_bytes(60.0, &c, "k", "a", "b", 10 * 1024 * 1024)
            .is_none());
        assert!(d
            .on_flow_bytes(120.0, &c, "k", "a", "b", 2 * 1024 * 1024 * 1024)
            .is_some());

        let mut hit = None;
        for i in 0..DENY_BURST {
            hit = d.on_deny(i as f64, &c, "a", "x").or(hit);
        }
        assert!(hit.is_some());

        assert!(d.on_exec(0.0, &c, "/tmp/x", "bash", 1).is_some());
        assert!(d
            .on_exec(0.0, &c, "/bin/sh", "qemu-system-x86", 1)
            .is_some());
        assert!(d.on_exec(0.0, &c, "/usr/bin/ls", "bash", 1).is_none());

        assert!(d.on_dns_query(0.0, &c, "a", "www.example.com").is_none());
        assert!(d
            .on_dns_query(
                0.0,
                &c,
                "a",
                "aGVsbG8gd29ybGQgdGhpcyBpcyBleGZpbHRyYXRpb24.t.evil.io"
            )
            .is_some());
    }
}
