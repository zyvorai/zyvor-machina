// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VM runtime intelligence: follows QEMU scopes into the VMI_* tracking maps
//! and turns VMI_HIST into per-VM reports.

use std::collections::{BTreeMap, HashSet};
use std::os::unix::fs::MetadataExt;

use aya::Pod;

use super::*;
use crate::{tracefs, vmintel};

const REFRESH_EVERY: Duration = Duration::from_secs(5);
const MAX_EXTRA: usize = 64;

enum Hook {
    Tp(&'static str, &'static str),
    /// First function that attaches wins.
    Kp(&'static [&'static str]),
}

const HOOKS: &[(u32, &str, Hook)] = &[
    (VMI_F_FLIGHT, "mn_vmi_kvm_exit", Hook::Tp("kvm", "kvm_exit")),
    (
        VMI_F_FLIGHT,
        "mn_vmi_wakeup",
        Hook::Tp("sched", "sched_wakeup"),
    ),
    (
        VMI_F_FLIGHT,
        "mn_vmi_switch",
        Hook::Tp("sched", "sched_switch"),
    ),
    (
        VMI_F_FLIGHT,
        "mn_vmi_migrate",
        Hook::Tp("sched", "sched_migrate_task"),
    ),
    (
        VMI_F_IO,
        "mn_vmi_blk_start",
        Hook::Tp("block", "block_bio_queue"),
    ),
    (
        VMI_F_IO,
        "mn_vmi_blk_done",
        Hook::Tp("block", "block_rq_complete"),
    ),
    (
        VMI_F_IO,
        "mn_vmi_vhost_work",
        Hook::Kp(&["vhost_vq_work_queue", "vhost_work_queue"]),
    ),
    (
        VMI_F_IO,
        "mn_vmi_vhost_kick",
        Hook::Kp(&["vhost_poll_wakeup"]),
    ),
    (
        VMI_F_FLIGHT | VMI_F_MEM,
        "mn_vmi_kvm_entry",
        Hook::Tp("kvm", "kvm_entry"),
    ),
    (VMI_F_MEM, "mn_vmi_fault", Hook::Kp(&["handle_mm_fault"])),
    (
        VMI_F_MEM,
        "mn_vmi_fault_ret",
        Hook::Kp(&["handle_mm_fault"]),
    ),
    (
        VMI_F_MEM,
        "mn_vmi_reclaim_begin",
        Hook::Tp("vmscan", "mm_vmscan_direct_reclaim_begin"),
    ),
    (
        VMI_F_MEM,
        "mn_vmi_reclaim_end",
        Hook::Tp("vmscan", "mm_vmscan_direct_reclaim_end"),
    ),
    (
        VMI_F_TOPO,
        "mn_vmi_irq_entry",
        Hook::Tp("irq", "irq_handler_entry"),
    ),
    (
        VMI_F_TOPO,
        "mn_vmi_irq_exit",
        Hook::Tp("irq", "irq_handler_exit"),
    ),
    (
        VMI_F_TOPO,
        "mn_vmi_softirq_entry",
        Hook::Tp("irq", "softirq_entry"),
    ),
    (
        VMI_F_TOPO,
        "mn_vmi_softirq_exit",
        Hook::Tp("irq", "softirq_exit"),
    ),
];

#[derive(Default)]
pub(super) struct Found {
    cgroup: String,
    cgroup_ids: Vec<u64>,
    tgids: HashSet<u32>,
    pub(super) threads: Vec<(u32, u32, Option<u32>)>, // (tid, tgid, vcpu)
}

#[derive(Default)]
pub(super) struct VmiRuntime {
    pub config: VmIntelConfig,
    keys: HashMap<String, u32>,
    next_key: u32,
    tracked: BTreeMap<String, VmIntelTracked>,
    /// Process start (clock ticks) of each VM's main QEMU process.
    starts: HashMap<String, u64>,
    in_tgids: HashSet<u32>,
    in_tids: HashSet<u32>,
    in_cgroups: HashSet<u64>,
    last_refresh: Option<Instant>,
    exit_names: HashMap<u32, String>,
    notes: Vec<String>,
}

fn read_ids(path: &Path) -> Vec<u32> {
    std::fs::read_to_string(path)
        .map(|s| s.lines().filter_map(|l| l.trim().parse().ok()).collect())
        .unwrap_or_default()
}

fn tgid_of(tid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{tid}/status")).ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("Tgid:"))
        .and_then(|v| v.trim().parse().ok())
}

/// Walk a cgroup subtree: cgroup ids (directory inodes) and every thread.
pub(super) fn discover(rel: &str) -> Found {
    let mut f = Found {
        cgroup: rel.to_string(),
        ..Default::default()
    };
    let mut stack = vec![Path::new(attribution::CGROUP_ROOT).join(rel)];
    while let Some(dir) = stack.pop() {
        let Ok(md) = std::fs::metadata(&dir) else {
            continue;
        };
        f.cgroup_ids.push(md.ino());
        for tid in read_ids(&dir.join("cgroup.threads")) {
            let Some(tgid) = tgid_of(tid) else { continue };
            let comm = std::fs::read_to_string(format!("/proc/{tid}/comm")).unwrap_or_default();
            f.tgids.insert(tgid);
            f.threads.push((tid, tgid, vmintel::vcpu_index(&comm)));
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            stack.extend(
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path()),
            );
        }
    }
    f
}

fn tp_offsets(root: Option<&Path>) -> (VmiCfg, HashMap<u32, String>) {
    let mut c = VmiCfg::default();
    let Some(root) = root else {
        return (c, HashMap::new());
    };
    let read = |cat: &str, ev: &str| {
        std::fs::read_to_string(root.join("events").join(cat).join(ev).join("format"))
            .unwrap_or_default()
    };
    let off = |text: &str, f: &str| {
        tracefs::parse_format(text)
            .get(f)
            .map(|x| x.offset)
            .unwrap_or(0)
    };
    let exit = read("kvm", "kvm_exit");
    c.off_exit_reason = off(&exit, "exit_reason");
    c.off_wakeup_pid = off(&read("sched", "sched_wakeup"), "pid");
    let sw = read("sched", "sched_switch");
    c.off_switch_prev_pid = off(&sw, "prev_pid");
    c.off_switch_prev_state = off(&sw, "prev_state");
    c.off_switch_next_pid = off(&sw, "next_pid");
    c.off_migrate_pid = off(&read("sched", "sched_migrate_task"), "pid");
    c.off_entry_vcpu = off(&read("kvm", "kvm_entry"), "vcpu_id");
    let bio = read("block", "block_bio_queue");
    c.off_bio_dev = off(&bio, "dev");
    c.off_bio_sector = off(&bio, "sector");
    let rqc = read("block", "block_rq_complete");
    c.off_rqc_dev = off(&rqc, "dev");
    c.off_rqc_sector = off(&rqc, "sector");
    (c, tracefs::parse_symbolic(&exit))
}

impl Engine {
    fn vmi_note(&mut self, n: String) {
        if !self.vmi.notes.contains(&n) {
            tracing::info!("{n}");
            self.vmi.notes.push(n);
        }
    }

    pub(super) fn vmi_configure(&mut self, cfg: VmIntelConfig) -> Result<VmIntelStatus> {
        let mask = vmintel::features_mask(&cfg.features).map_err(|e| anyhow!(e))?;
        if cfg.extra.len() > MAX_EXTRA {
            return Err(anyhow!("at most {MAX_EXTRA} extra targets"));
        }
        for t in &cfg.extra {
            if t.name.is_empty()
                || t.cgroup.contains("..")
                || !Path::new(attribution::CGROUP_ROOT).join(&t.cgroup).is_dir()
            {
                return Err(anyhow!(
                    "extra target `{}`: cgroup `{}` not found under /sys/fs/cgroup",
                    t.name,
                    t.cgroup
                ));
            }
        }
        let was = self.vmi.config.enabled;
        self.vmi.notes.clear();
        let progs: Vec<&str> = HOOKS.iter().map(|(_, p, _)| *p).collect();
        self.dp.detach_traces(&progs);
        if cfg.enabled && !was {
            self.vmi_clear_stats();
        }
        let (mut c, names) = tp_offsets(tracefs::tracefs_root().as_deref());
        self.vmi.exit_names = names;
        if cfg.enabled {
            for (bit, prog, hook) in HOOKS {
                if mask & bit == 0 {
                    continue;
                }
                let res = match hook {
                    Hook::Tp(cat, ev) => self.dp.attach_tracepoint(prog, cat, ev),
                    Hook::Kp(funcs) => {
                        let mut last = Err(anyhow!("no candidate"));
                        for f in *funcs {
                            last = self.dp.attach_kprobe(prog, f);
                            if last.is_ok() {
                                break;
                            }
                        }
                        last
                    }
                };
                if let Err(e) = res {
                    self.vmi_note(format!("{prog}: {e:#}"));
                }
            }
        }
        c.enabled = cfg.enabled as u32;
        c.features = mask;
        self.dp.array_set("VMI_CFG", 0, c)?;
        self.vmi.config = cfg;
        self.vmi_refresh_now();
        Ok(self.vmi_status())
    }

    fn vmi_clear_stats(&mut self) {
        let keys = self
            .dp
            .percpu_sum::<VmiKey, u64>("VMI_HIST", |a, b| *a += *b)
            .unwrap_or_default();
        for (k, _) in keys {
            self.dp.percpu_remove::<VmiKey, u64>("VMI_HIST", &k);
        }
        self.dp.hash_clear::<u32, u64>("VMI_BOOT");
    }

    pub(super) fn vmi_refresh(&mut self) {
        if !self.vmi.config.enabled && self.vmi.tracked.is_empty() {
            return;
        }
        if self
            .vmi
            .last_refresh
            .is_some_and(|t| t.elapsed() < REFRESH_EVERY)
        {
            return;
        }
        self.vmi_refresh_now();
    }

    /// Re-discover tracked VMs and sync the tracking maps (emptied when off).
    pub(super) fn vmi_refresh_now(&mut self) {
        self.vmi.last_refresh = Some(Instant::now());
        let mut targets: BTreeMap<String, String> = if self.vmi.config.enabled {
            super::vm::qemu_scopes()
        } else {
            BTreeMap::new()
        };
        if self.vmi.config.enabled {
            for t in &self.vmi.config.extra {
                targets.insert(t.name.clone(), t.cgroup.clone());
            }
        }
        // vCPU threads learned in the kernel on kvm_entry (unnamed threads).
        let learned: HashMap<u32, VmiThread> = self
            .dp
            .hash_entries::<u32, VmiThread>("VMI_TIDS")
            .unwrap_or_default()
            .into_iter()
            .collect();
        self.vmi.in_tids.extend(learned.keys().copied());
        let (mut tgids, mut tids, mut cgs) = (HashMap::new(), HashMap::new(), HashMap::new());
        let mut tracked = BTreeMap::new();
        for (name, rel) in targets {
            let key = match self.vmi.keys.get(&name) {
                Some(k) => *k,
                None => {
                    self.vmi.next_key += 1;
                    self.vmi.keys.insert(name.clone(), self.vmi.next_key);
                    self.vmi.next_key
                }
            };
            let mut f = discover(&rel);
            for t in f.threads.iter_mut().filter(|t| t.2.is_none()) {
                if let Some(l) = learned
                    .get(&t.0)
                    .filter(|l| l.vm == key && l.vcpu != u32::MAX)
                {
                    t.2 = Some(l.vcpu);
                }
            }
            for id in &f.cgroup_ids {
                cgs.insert(*id, key);
            }
            for t in &f.tgids {
                tgids.insert(*t, key);
            }
            for (tid, _, vcpu) in &f.threads {
                tids.insert(
                    *tid,
                    VmiThread {
                        vm: key,
                        vcpu: vcpu.unwrap_or(u32::MAX),
                    },
                );
            }
            // Main process = the tgid owning the vCPU threads (else any).
            let main = f
                .threads
                .iter()
                .find(|t| t.2.is_some())
                .map(|t| t.1)
                .or_else(|| f.tgids.iter().min().copied());
            if let Some(start) = main
                .and_then(|p| std::fs::read_to_string(format!("/proc/{p}/stat")).ok())
                .and_then(|s| vmintel::stat_starttime(&s))
            {
                self.vmi.starts.insert(name.clone(), start);
            }
            tracked.insert(
                name.clone(),
                VmIntelTracked {
                    name,
                    cgroup: f.cgroup,
                    processes: f.tgids.len(),
                    threads: f.threads.len(),
                    vcpus: f.threads.iter().filter(|t| t.2.is_some()).count(),
                },
            );
        }
        sync(&mut self.dp, "VMI_TGIDS", &mut self.vmi.in_tgids, &tgids);
        sync(&mut self.dp, "VMI_TIDS", &mut self.vmi.in_tids, &tids);
        sync(&mut self.dp, "VMI_CGROUPS", &mut self.vmi.in_cgroups, &cgs);
        self.vmi.tracked = tracked;
    }

    pub(super) fn vmi_status(&mut self) -> VmIntelStatus {
        let hist = self
            .dp
            .percpu_sum::<VmiKey, u64>("VMI_HIST", |a, b| *a += *b)
            .unwrap_or_default();
        let mut cpus: BTreeMap<u32, VmIntelCpu> = BTreeMap::new();
        for (k, v) in hist.iter().filter(|(k, _)| k.vm == 0) {
            let e = cpus.entry(k.slot as u32).or_insert_with(|| VmIntelCpu {
                cpu: k.slot as u32,
                ..Default::default()
            });
            match k.kind {
                vmi_kind::IRQ => e.irq_ns += v,
                vmi_kind::SOFTIRQ => e.softirq_ns += v,
                _ => {}
            }
        }
        let prefixes: Vec<&str> = HOOKS.iter().map(|(_, p, _)| *p).collect();
        VmIntelStatus {
            config: self.vmi.config.clone(),
            hooks: self
                .dp
                .traces()
                .into_iter()
                .filter(|t| {
                    prefixes
                        .iter()
                        .any(|p| t == p || t.starts_with(&format!("{p}@")))
                })
                .collect(),
            vms: self.vmi.tracked.values().cloned().collect(),
            cpus: cpus.into_values().collect(),
            notes: self.vmi.notes.clone(),
        }
    }

    pub(super) fn vmi_vm(&mut self, name: &str) -> Result<VmIntelReport> {
        let key = *self.vmi.keys.get(name).ok_or_else(|| {
            anyhow!("VM {name} is not tracked (is VM runtime intelligence enabled?)")
        })?;
        let hist = self
            .dp
            .percpu_sum::<VmiKey, u64>("VMI_HIST", |a, b| *a += *b)?;
        let mut slots: HashMap<u16, Vec<(u16, u64)>> = HashMap::new();
        let mut r = VmIntelReport {
            name: name.to_string(),
            ..Default::default()
        };
        for (k, v) in hist.into_iter().filter(|(k, _)| k.vm == key) {
            match k.kind {
                vmi_kind::EXIT => r.exits.push(VmIntelExit {
                    reason: k.slot as u32,
                    name: self
                        .vmi
                        .exit_names
                        .get(&(k.slot as u32))
                        .cloned()
                        .unwrap_or_else(|| format!("reason {}", k.slot)),
                    count: v,
                }),
                vmi_kind::VHOST_WORK => r.vhost_work += v,
                vmi_kind::VHOST_KICK => r.vhost_kicks += v,
                vmi_kind::MIGRATE => r.migrations += v,
                vmi_kind::RESIDENCY => r.residency.push(VmIntelResidency {
                    cpu: k.slot as u32,
                    ns: v,
                }),
                kind => slots.entry(kind).or_default().push((k.slot, v)),
            }
        }
        r.exits.sort_by_key(|x| std::cmp::Reverse(x.count));
        r.residency.sort_by_key(|x| x.cpu);
        let h = |kind| vmintel::hist(slots.get(&kind).map(Vec::as_slice).unwrap_or_default());
        r.runq = h(vmi_kind::RUNQ);
        r.block = h(vmi_kind::BLK);
        r.fault = h(vmi_kind::FAULT);
        r.reclaim = h(vmi_kind::RECLAIM);
        let boot: HashMap<u32, u64> = self
            .dp
            .hash_entries::<u32, u64>("VMI_BOOT")
            .unwrap_or_default()
            .into_iter()
            .collect();
        let tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(0) as u64;
        r.boot_to_first_entry_ms = boot
            .get(&key)
            .zip(self.vmi.starts.get(name))
            .and_then(|(first, start)| vmintel::boot_ms(*start, tck, *first));
        Ok(r)
    }
}

/// Make a tracking map hold exactly `want`.
fn sync<K: Pod + Eq + std::hash::Hash, V: Pod>(
    dp: &mut Datapath,
    map: &str,
    have: &mut HashSet<K>,
    want: &HashMap<K, V>,
) {
    for k in have.iter().filter(|k| !want.contains_key(k)) {
        dp.cni_hash_remove::<K, V>(map, k);
    }
    have.retain(|k| want.contains_key(k));
    for (k, v) in want {
        if dp.cni_hash_insert(map, *k, *v).is_ok() {
            have.insert(*k);
        }
    }
}
