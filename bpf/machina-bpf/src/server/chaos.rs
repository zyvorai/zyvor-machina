// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Chaos faults (`netpol::chaos`). A ticker ends faults at their lease;
//! the root qdisc a tap had before its first fault (bpfd's QoS `fq`) is put
//! back afterwards, and a restarted bpfd removes `netem` qdiscs it no longer
//! holds, so a fault never outlives its lease by more than a restart.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::process::{Command, Stdio};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::netpol::chaos::{self as ch, HANDLE};

use super::*;

const TICK: Duration = Duration::from_secs(1);

struct Held {
    fault: VmChaosFault,
    taps: Vec<String>,
    until: DateTime<Utc>,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct ChaosInner {
    faults: BTreeMap<String, Held>,
    /// tap -> root qdisc kind before bpfd's netem replaced it.
    prior: BTreeMap<String, String>,
    started: bool,
    /// This instance has the table installed (another bpfd on the host may
    /// own one too, so it is never touched otherwise).
    table: bool,
}

#[derive(Default, Clone)]
pub(super) struct ChaosRuntime(Arc<Mutex<ChaosInner>>);

fn tc(args: &[&str]) -> Result<String> {
    let out = Command::new("tc")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("run tc")?;
    if !out.status.success() {
        return Err(anyhow!(
            "tc {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn root_of(dev: &str) -> Option<(String, String)> {
    tc(&["-j", "qdisc", "show", "dev", dev, "root"])
        .ok()
        .and_then(|s| ch::root_qdisc(&s))
}

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

fn exists(dev: &str) -> bool {
    Path::new(&format!("/sys/class/net/{dev}")).exists()
}

/// Put back what the tap had: bpfd's `fq`, or the kernel default.
fn restore(dev: &str, prior: Option<&str>) {
    if !exists(dev) {
        return;
    }
    let ours = root_of(dev).is_some_and(|(k, h)| k == "netem" && h == HANDLE);
    if !ours {
        return;
    }
    let r = match prior {
        Some("fq") => tc(&["qdisc", "replace", "dev", dev, "root", "fq"]),
        _ => tc(&["qdisc", "del", "dev", dev, "root"]),
    };
    if let Err(e) = r {
        tracing::warn!("chaos: restore {dev}: {e:#}");
    }
}

fn taps_of(f: &VmChaosFault) -> Vec<String> {
    if let Some(t) = &f.tap {
        return if exists(t) {
            vec![t.clone()]
        } else {
            Vec::new()
        };
    }
    let mut v: Vec<String> = crate::attribution::scan_libvirt()
        .into_iter()
        .filter(|(dev, i)| i.vm == f.vm && ch::ifname_ok(dev))
        .map(|(dev, _)| dev)
        .collect();
    v.sort();
    v
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl ChaosInner {
    /// Bring qdiscs and the table in line with the held faults.
    fn apply(&mut self) {
        let mut netem: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        let mut parts = Vec::new();
        for (id, h) in &self.faults {
            for t in &h.taps {
                if ch::has_netem(&h.fault) {
                    netem.insert(t.clone(), (id.clone(), ch::netem_args(&h.fault)));
                }
                if !h.fault.partition.is_empty() {
                    parts.push((t.clone(), h.fault.partition.clone()));
                }
            }
        }
        self.started = true;
        let stale: Vec<String> = self
            .prior
            .keys()
            .filter(|t| !netem.contains_key(*t))
            .cloned()
            .collect();
        for t in stale {
            let p = self.prior.remove(&t);
            restore(&t, p.as_deref());
        }
        let mut errors: BTreeMap<String, String> = BTreeMap::new();
        for (tap, (id, args)) in &netem {
            if !self.prior.contains_key(tap) {
                let kind = root_of(tap).map(|(k, _)| k).unwrap_or_default();
                let kind = if kind == "netem" { String::new() } else { kind };
                self.prior.insert(tap.clone(), kind);
            }
            let mut a = vec![
                "qdisc", "replace", "dev", tap, "root", "handle", HANDLE, "netem",
            ];
            a.extend(args.iter().map(String::as_str));
            if let Err(e) = tc(&a) {
                errors.insert(id.clone(), format!("{e:#}"));
            }
        }
        if self.table || !parts.is_empty() {
            match nft_apply(&ch::transaction(ch::render(&parts).as_deref())) {
                Ok(()) => self.table = !parts.is_empty(),
                Err(e) => {
                    for (id, h) in &self.faults {
                        if !h.fault.partition.is_empty() {
                            errors.insert(id.clone(), format!("{e:#}"));
                        }
                    }
                }
            }
        }
        for (id, h) in self.faults.iter_mut() {
            h.error = errors.remove(id);
        }
    }

    fn expire(&mut self) -> bool {
        let now = Utc::now();
        let gone: Vec<String> = self
            .faults
            .iter()
            .filter(|(_, h)| h.until <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &gone {
            tracing::info!("chaos: fault {id} lease ended");
            self.faults.remove(id);
        }
        !gone.is_empty()
    }
}

impl ChaosRuntime {
    pub(super) fn start(&self, fault: VmChaosFault) -> Result<VmChaosActive> {
        ch::validate(&fault).map_err(|e| anyhow!(e))?;
        let taps = taps_of(&fault);
        if taps.is_empty() {
            return Err(anyhow!(
                "no running interface for {}",
                fault.tap.as_deref().unwrap_or(&fault.vm)
            ));
        }
        let mut g = lock(&self.0);
        if ch::has_netem(&fault) {
            if let Some((other, _)) = g.faults.iter().find(|(id, h)| {
                **id != fault.id
                    && ch::has_netem(&h.fault)
                    && h.taps.iter().any(|t| taps.contains(t))
            }) {
                return Err(anyhow!(
                    "{} already has a latency/loss fault ({other}); stop it first",
                    fault.tap.as_deref().unwrap_or(&fault.vm)
                ));
            }
        }
        let until = Utc::now() + chrono::Duration::seconds(fault.secs as i64);
        let id = fault.id.clone();
        tracing::info!(id = %id, vm = %fault.vm, secs = fault.secs, "chaos: fault started");
        g.faults.insert(
            id.clone(),
            Held {
                fault,
                taps,
                until,
                error: None,
            },
        );
        g.apply();
        let st = Self::active(&g, &id).context("fault vanished")?;
        if let Some(e) = &st.error {
            g.faults.remove(&id);
            g.apply();
            return Err(anyhow!("fault not applied: {e}"));
        }
        Ok(st)
    }

    pub(super) fn stop(&self, id: &str, prefix: &str) -> Vec<String> {
        let mut g = lock(&self.0);
        let gone: Vec<String> = g
            .faults
            .keys()
            .filter(|k| {
                (!id.is_empty() && *k == id) || (!prefix.is_empty() && k.starts_with(prefix))
            })
            .cloned()
            .collect();
        for k in &gone {
            tracing::info!("chaos: fault {k} stopped");
            g.faults.remove(k);
        }
        if !gone.is_empty() {
            g.apply();
        }
        gone
    }

    fn active(g: &ChaosInner, id: &str) -> Option<VmChaosActive> {
        let h = g.faults.get(id)?;
        Some(VmChaosActive {
            fault: h.fault.clone(),
            taps: h.taps.clone(),
            until: rfc3339(h.until),
            remaining_secs: (h.until - Utc::now()).num_seconds().max(0) as u64,
            error: h.error.clone(),
        })
    }

    pub(super) fn status(&self) -> VmChaosStatus {
        let g = lock(&self.0);
        VmChaosStatus {
            faults: g
                .faults
                .keys()
                .filter_map(|id| Self::active(&g, id))
                .collect(),
            hostname: None,
        }
    }

    /// For the state file: faults with their end time, and saved qdiscs.
    pub(super) fn persisted(&self) -> (Vec<VmChaosActive>, BTreeMap<String, String>) {
        let st = self.status();
        (st.faults, lock(&self.0).prior.clone())
    }

    /// Hold persisted faults again for the time they have left; anything
    /// else bpfd left on a tap is removed on the first apply.
    pub(super) fn restore(&self, faults: Vec<VmChaosActive>, prior: BTreeMap<String, String>) {
        let mut g = lock(&self.0);
        g.prior = prior;
        g.table = faults.iter().any(|a| !a.fault.partition.is_empty());
        let now = Utc::now();
        for a in faults {
            let Ok(until) = DateTime::parse_from_rfc3339(&a.until) else {
                continue;
            };
            let until = until.with_timezone(&Utc);
            if until <= now {
                continue;
            }
            let taps = taps_of(&a.fault);
            let alive: BTreeSet<&String> = a.taps.iter().collect();
            if taps.iter().any(|t| alive.contains(t)) {
                g.faults.insert(
                    a.fault.id.clone(),
                    Held {
                        fault: a.fault,
                        taps,
                        until,
                        error: None,
                    },
                );
            }
        }
        g.apply();
    }

    pub(super) fn spawn_ticker(&self) {
        let rt = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let rt2 = rt.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let mut g = lock(&rt2.0);
                    if !g.started || g.expire() {
                        g.apply();
                    }
                })
                .await;
            }
        });
    }
}
