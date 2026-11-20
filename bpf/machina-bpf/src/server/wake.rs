// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! The wake set (`netpol::wake`): the nftables tables, plus a poller that
//! turns a moving counter into a `vm_wake` stream event. The poller only
//! runs `nft` while something sleeps, and repeats a wake every few seconds
//! until the VM leaves the set, so a lagging subscriber still sees it.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write as _;
use std::net::IpAddr;
use std::process::{Command, Stdio};

use crate::netpol::wake::{self as wk, CounterKey, TABLE};

use super::*;

const POLL: Duration = Duration::from_millis(250);
const REPEAT: Duration = Duration::from_secs(3);
const REPEAT_FOR: Duration = Duration::from_secs(60);
const HISTORY: usize = 64;

#[derive(Default)]
pub(super) struct WakeInner {
    config: VmWake,
    error: Option<String>,
    counts: BTreeMap<CounterKey, u64>,
    /// vm -> (first wake, last publish, address, hook).
    pending: BTreeMap<String, (Instant, Instant, String, &'static str)>,
    history: VecDeque<VmWakeEvent>,
    /// address -> (dev, lladdr) pinned as a permanent neighbor while its VM
    /// sleeps; otherwise host-originated traffic fails ARP resolution (about
    /// 3 s) before the restore finishes and the caller sees EHOSTUNREACH.
    pins: BTreeMap<IpAddr, (String, String)>,
}

#[derive(Default, Clone)]
pub(super) struct WakeRuntime(Arc<Mutex<WakeInner>>);

fn nft_apply(script: &str) -> Result<()> {
    let mut child = Command::new("nft")
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run nft")?;
    child
        .stdin
        .take()
        .context("nft stdin")?
        .write_all(script.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "nft: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn read_counters() -> BTreeMap<CounterKey, u64> {
    let mut m = BTreeMap::new();
    for fam in ["inet", "bridge"] {
        if let Ok(out) = Command::new("nft")
            .args(["-j", "list", "table", fam, TABLE])
            .stderr(Stdio::null())
            .output()
        {
            if out.status.success() {
                wk::parse_counters(&String::from_utf8_lossy(&out.stdout), &mut m);
            }
        }
    }
    m
}

fn neighbor(addr: IpAddr) -> Option<(String, String)> {
    let out = Command::new("ip")
        .args(["-j", "neigh", "show", "to", &addr.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.as_array()?.iter().find_map(|n| {
        Some((
            n["dev"].as_str()?.to_string(),
            n["lladdr"].as_str()?.to_string(),
        ))
    })
}

fn set_neighbor(addr: IpAddr, dev: &str, mac: &str, nud: &str) -> bool {
    Command::new("ip")
        .args([
            "neigh",
            "replace",
            &addr.to_string(),
            "lladdr",
            mac,
            "dev",
            dev,
            "nud",
            nud,
        ])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl WakeRuntime {
    pub(super) fn config(&self) -> VmWake {
        lock(&self.0).config.clone()
    }

    pub(super) fn set(&self, cfg: VmWake) -> Result<VmWakeStatus> {
        wk::validate(&cfg).map_err(|e| anyhow!(e))?;
        let res = nft_apply(&wk::transaction(wk::render(&cfg).as_deref()));
        let mut g = lock(&self.0);
        if let Err(e) = &res {
            tracing::warn!("wake: {e:#}");
        } else {
            tracing::info!(vms = cfg.entries.len(), "wake set applied");
        }
        g.error = res.err().map(|e| format!("{e:#}"));
        g.counts.clear();
        let keep: Vec<String> = cfg.entries.iter().map(|e| e.vm.clone()).collect();
        g.pending.retain(|vm, _| keep.contains(vm));
        let wanted: Vec<IpAddr> = cfg
            .entries
            .iter()
            .flat_map(|e| e.addresses.iter().filter_map(|a| a.parse().ok()))
            .collect();
        let mut pins = std::mem::take(&mut g.pins);
        g.config = cfg;
        drop(g);
        pins.retain(|addr, (dev, mac)| {
            wanted.contains(addr) || {
                set_neighbor(*addr, dev, mac, "stale");
                false
            }
        });
        for addr in wanted {
            if pins.contains_key(&addr) {
                continue;
            }
            if let Some((dev, mac)) = neighbor(addr) {
                if set_neighbor(addr, &dev, &mac, "permanent") {
                    pins.insert(addr, (dev, mac));
                }
            }
        }
        lock(&self.0).pins = pins;
        Ok(self.status())
    }

    pub(super) fn status(&self) -> VmWakeStatus {
        let g = lock(&self.0);
        VmWakeStatus {
            entries: g.config.entries.clone(),
            wakes: g.history.iter().cloned().collect(),
            error: g.error.clone(),
            hostname: None,
        }
    }

    /// One poll: read counters, record and publish new wakes, repeat pending ones.
    fn poll(&self, bus: &broadcast::Sender<StreamEvent>) {
        if lock(&self.0).config.entries.is_empty() {
            return;
        }
        let now_counts = read_counters();
        let mut g = lock(&self.0);
        let fresh = wk::risen(&g.counts, &now_counts);
        g.counts = now_counts;
        let now = Instant::now();
        for (vm, ip, via) in fresh {
            if g.pending.contains_key(&vm) {
                continue;
            }
            let ev = VmWakeEvent {
                vm: vm.clone(),
                address: ip.to_string(),
                via: via.to_string(),
                at: unix_now(),
            };
            tracing::info!(vm = %vm, address = %ip, via, "wake: traffic for sleeping vm");
            publish(bus, "vm_wake", &ev);
            g.history.push_back(ev);
            while g.history.len() > HISTORY {
                g.history.pop_front();
            }
            g.pending.insert(vm, (now, now, ip.to_string(), via));
        }
        g.pending
            .retain(|_, (first, _, _, _)| now.duration_since(*first) < REPEAT_FOR);
        for (vm, (_, last, addr, via)) in g.pending.iter_mut() {
            if now.duration_since(*last) >= REPEAT {
                *last = now;
                publish(
                    bus,
                    "vm_wake",
                    &VmWakeEvent {
                        vm: vm.clone(),
                        address: addr.clone(),
                        via: via.to_string(),
                        at: unix_now(),
                    },
                );
            }
        }
    }

    pub(super) fn spawn_poller(&self, bus: broadcast::Sender<StreamEvent>) {
        let rt = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(POLL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let (rt2, b) = (rt.clone(), bus.clone());
                let _ = tokio::task::spawn_blocking(move || rt2.poll(&b)).await;
            }
        });
    }
}
