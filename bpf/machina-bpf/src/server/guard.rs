// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! VMM guard: BPF-LSM hooks (`mn_guard_exec/mprotect/open`) on QEMU cgroups.
//! Audit by default; enforce only with `bpf` an active LSM and a lease.

use std::collections::{BTreeMap, HashSet};
use std::os::unix::fs::MetadataExt;

use super::*;

const HOOKS: &[(&str, &str)] = &[
    ("mn_guard_exec", "bprm_check_security"),
    ("mn_guard_mprotect", "file_mprotect"),
    ("mn_guard_open", "file_open"),
];
const MAX_LEASE_SECS: u64 = 3600;
pub(super) const GUARD_STORE_CAP: usize = 2000;
const LSM_LIST: &str = "/sys/kernel/security/lsm";
/// Test-only: attach even when `bpf` is not an active LSM (hooks never run).
const ASSUME_ENV: &str = "MACHINA_BPF_GUARD_ASSUME_LSM";

const QEMU_BINARIES: &[&str] = &[
    "/usr/bin/qemu-system-x86_64",
    "/usr/bin/qemu-system-aarch64",
    "/usr/libexec/qemu-kvm",
    "/usr/bin/qemu-kvm",
    "/usr/bin/kvm",
    "/usr/lib/qemu/qemu-bridge-helper",
    "/usr/libexec/qemu-bridge-helper",
];

#[derive(Default)]
pub(super) struct GuardRuntime {
    pub config: GuardConfig,
    deadline_mono: u64,
    lease_expired: bool,
    targets: BTreeMap<String, GuardTarget>,
    in_policies: HashSet<u64>,
    files: Vec<(String, GuardFileKey)>,
    devices: Vec<(String, u64)>,
    offsets: Option<Option<[u32; 8]>>,
    notes: Vec<String>,
}

pub(super) fn lsm_list() -> String {
    std::fs::read_to_string(LSM_LIST)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub(super) fn lsm_active() -> bool {
    lsm_list().split(',').any(|l| l.trim() == "bpf")
}

fn layout() -> Option<[u32; 8]> {
    let btf = crate::btf::Btf::from_sys_fs().ok()?;
    Some([
        btf.offset("linux_binprm", "file")?,
        btf.offset("file", "f_inode")?,
        btf.offset("inode", "i_ino")?,
        btf.offset("inode", "i_sb")?,
        btf.offset("super_block", "s_dev")?,
        btf.offset("inode", "i_mode")?,
        btf.offset("inode", "i_rdev")?,
        btf.offset("vm_area_struct", "vm_flags")?,
    ])
}

/// Kernel-internal dev_t (MKDEV: major << 20 | minor) of a stat st_dev.
fn kdev(st_dev: u64) -> u32 {
    let (ma, mi) = (
        libc::major(st_dev as libc::dev_t),
        libc::minor(st_dev as libc::dev_t),
    );
    (ma << 20) | (mi & 0xfffff)
}

fn file_key(path: &str) -> Option<GuardFileKey> {
    let m = std::fs::metadata(path).ok()?;
    m.is_file().then(|| GuardFileKey {
        ino: m.ino(),
        dev: kdev(m.dev()),
        _pad: 0,
    })
}

fn cgroup_ids(rel: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut stack = vec![Path::new(attribution::CGROUP_ROOT).join(rel)];
    while let Some(dir) = stack.pop() {
        let Ok(md) = std::fs::metadata(&dir) else {
            continue;
        };
        out.push(md.ino());
        if let Ok(rd) = std::fs::read_dir(&dir) {
            stack.extend(
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path()),
            );
        }
    }
    out
}

fn char_name(major: u32) -> Option<String> {
    let s = std::fs::read_to_string("/proc/devices").ok()?;
    let chars = s.split("Block devices:").next()?;
    chars.lines().find_map(|l| {
        let (n, name) = l.trim().split_once(' ')?;
        (n.parse::<u32>().ok()? == major).then(|| name.trim().to_string())
    })
}

impl Engine {
    pub(super) fn guard_configure(&mut self, cfg: GuardConfig) -> Result<GuardStatus> {
        let enforce = match cfg.mode.as_str() {
            "audit" => false,
            "enforce" => true,
            m => return Err(anyhow!("mode must be audit or enforce (got `{m}`)")),
        };
        let active = lsm_active();
        let assume = std::env::var_os(ASSUME_ENV).is_some();
        if cfg.enabled && !active && !assume {
            return Err(anyhow!(
                "lsm_inactive: `bpf` is not an active LSM ({LSM_LIST}: {}); add it to the kernel command line, \
                 e.g. lsm={},bpf in GRUB_CMDLINE_LINUX, and reboot",
                lsm_list(),
                lsm_list()
            ));
        }
        let lease = if enforce {
            if !cfg.enabled {
                return Err(anyhow!("enforce needs enabled"));
            }
            if !active {
                return Err(anyhow!(
                    "lsm_inactive: refusing to arm enforcement while `bpf` is not an active LSM"
                ));
            }
            let s = cfg
                .lease_secs
                .ok_or_else(|| anyhow!("enforce needs lease_secs (1..={MAX_LEASE_SECS})"))?;
            if !(1..=MAX_LEASE_SECS).contains(&s) {
                return Err(anyhow!("lease_secs must be 1..={MAX_LEASE_SECS}"));
            }
            Some(s)
        } else {
            None
        };
        let mut devices = Vec::new();
        for r in super::vm::default_dev_rules()
            .into_iter()
            .filter(|r| r.dev_type == DEVCG_DEV_CHAR)
        {
            devices.push((
                super::vm::dev_rule_string(&r),
                (r.major as u64) << 32 | r.minor as u64,
            ));
        }
        for s in &cfg.allow_devices {
            let r = super::vm::parse_dev_rule(s)?;
            if r.dev_type != DEVCG_DEV_CHAR {
                return Err(anyhow!(
                    "allow_devices `{s}`: only char devices are policed"
                ));
            }
            devices.push((
                super::vm::dev_rule_string(&r),
                (r.major as u64) << 32 | r.minor as u64,
            ));
        }
        let mut files = Vec::new();
        for p in QEMU_BINARIES
            .iter()
            .map(|s| s.to_string())
            .chain(cfg.allow_exec.iter().cloned())
        {
            match file_key(&p) {
                Some(k) => files.push((p, k)),
                None if cfg.allow_exec.contains(&p) => {
                    return Err(anyhow!("allow_exec `{p}`: not a file"))
                }
                None => {}
            }
        }
        for t in &cfg.extra {
            if t.name.is_empty()
                || t.cgroup.contains("..")
                || !Path::new(attribution::CGROUP_ROOT).join(&t.cgroup).is_dir()
            {
                return Err(anyhow!(
                    "extra target `{}`: cgroup `{}` not found",
                    t.name,
                    t.cgroup
                ));
            }
        }
        let offs = *self.guard.offsets.get_or_insert_with(layout);
        let Some(o) = offs else {
            return Err(anyhow!(
                "kernel BTF lacks the inode/file layout the guard needs"
            ));
        };

        self.guard.notes.clear();
        let progs: Vec<&str> = HOOKS.iter().map(|(p, _)| *p).collect();
        if cfg.enabled {
            for (prog, hook) in HOOKS {
                if let Err(e) = self.dp.attach_lsm(prog, hook) {
                    self.dp.detach_traces(&progs);
                    return Err(e);
                }
            }
            if !active {
                self.guard.notes.push(format!("lsm_inactive: hooks are attached but `bpf` is not an active LSM ({}), so they never run", lsm_list()));
            }
        } else {
            self.dp.detach_traces(&progs);
        }

        self.dp.hash_clear::<GuardFileKey, u8>("GUARD_FILES");
        for (_, k) in &files {
            self.dp.cni_hash_insert("GUARD_FILES", *k, 1u8)?;
        }
        self.dp.hash_clear::<u64, u8>("GUARD_DEVICES");
        for (_, k) in &devices {
            self.dp.cni_hash_insert("GUARD_DEVICES", *k, 1u8)?;
        }
        let now = loader::monotonic_ns();
        self.guard.deadline_mono = lease.map(|s| now + s * 1_000_000_000).unwrap_or(0);
        if enforce {
            self.guard.lease_expired = false;
        }
        self.dp.array_set(
            "GUARD_CFG",
            0,
            GuardCfg {
                enabled: cfg.enabled as u32,
                enforce: enforce as u32,
                lease_deadline_ns: self.guard.deadline_mono,
                off_bprm_file: o[0],
                off_file_inode: o[1],
                off_inode_ino: o[2],
                off_inode_sb: o[3],
                off_sb_dev: o[4],
                off_inode_mode: o[5],
                off_inode_rdev: o[6],
                off_vma_flags: o[7],
            },
        )?;
        self.guard.files = files;
        self.guard.devices = devices;
        self.guard.config = cfg;
        self.guard_refresh();
        Ok(self.guard_status())
    }

    /// Follow VM scopes into GUARD_POLICIES (emptied when disabled).
    pub(super) fn guard_refresh(&mut self) {
        let c = self.guard.config.clone();
        let mut targets: BTreeMap<String, String> = BTreeMap::new();
        if c.enabled {
            targets = super::vm::qemu_scopes()
                .into_iter()
                .filter(|(vm, _)| c.vms.is_empty() || c.vms.contains(vm))
                .collect();
            for t in &c.extra {
                targets.insert(t.name.clone(), t.cgroup.clone());
            }
        }
        let flags = (c.exec as u32 * GUARD_EXEC)
            | (c.wx as u32 * GUARD_WX)
            | (c.devices as u32 * GUARD_DEV);
        let mut want: HashMap<u64, String> = HashMap::new();
        let mut out = BTreeMap::new();
        for (name, rel) in targets {
            let ids = cgroup_ids(&rel);
            for id in &ids {
                want.insert(*id, name.clone());
            }
            out.insert(
                name.clone(),
                GuardTarget {
                    name,
                    cgroup: rel,
                    cgroups: ids.len(),
                },
            );
        }
        for id in self.guard.in_policies.clone() {
            if !want.contains_key(&id) {
                self.dp.cni_hash_remove::<u64, u32>("GUARD_POLICIES", &id);
                self.guard.in_policies.remove(&id);
            }
        }
        for id in want.keys() {
            if self
                .dp
                .cni_hash_insert("GUARD_POLICIES", *id, flags)
                .is_ok()
            {
                self.guard.in_policies.insert(*id);
            }
        }
        lock(&self.shared).guard_cgroups = want;
        self.guard.targets = out;
    }

    /// Lease ran out: back to audit (the kernel already stopped denying).
    pub(super) fn guard_expire(&mut self) -> Result<()> {
        if self.guard.config.mode != "enforce" || loader::monotonic_ns() < self.guard.deadline_mono
        {
            return Ok(());
        }
        let mut cfg = self.guard.config.clone();
        cfg.mode = "audit".into();
        cfg.lease_secs = None;
        tracing::warn!("vmm guard lease expired; back to audit");
        self.guard_configure(cfg)?;
        self.guard.lease_expired = true;
        Ok(())
    }

    pub(super) fn guard_status(&mut self) -> GuardStatus {
        let mut sum = |i| {
            self.dp
                .percpu_array_sum::<u64>("GUARD_STATS", i, |a, b| *a += *b)
                .unwrap_or(0)
        };
        let (audited, denied, dropped) = (
            sum(GUARD_STAT_AUDITED),
            sum(GUARD_STAT_DENIED),
            sum(GUARD_STAT_DROPPED),
        );
        let now = loader::monotonic_ns();
        let enforcing =
            self.guard.config.mode == "enforce" && now < self.guard.deadline_mono && lsm_active();
        let hooks = HOOKS
            .iter()
            .filter(|(p, _)| self.dp.trace_attached(p))
            .map(|(p, h)| format!("{p}@{h}"))
            .collect();
        GuardStatus {
            config: self.guard.config.clone(),
            lsm_active: lsm_active(),
            lsm_list: lsm_list(),
            hooks,
            enforcing,
            lease_remaining_secs: (self.guard.config.mode == "enforce"
                && now < self.guard.deadline_mono)
                .then(|| (self.guard.deadline_mono - now) / 1_000_000_000),
            lease_expired: self.guard.lease_expired,
            guarded: self.guard.targets.values().cloned().collect(),
            allowed_exec: self.guard.files.iter().map(|(p, _)| p.clone()).collect(),
            allowed_devices: self.guard.devices.iter().map(|(s, _)| s.clone()).collect(),
            audited,
            denied,
            dropped,
            notes: self.guard.notes.clone(),
        }
    }

    /// Persisted form: never enforce.
    pub(super) fn guard_persisted(&self) -> Option<GuardConfig> {
        let mut c = self.guard.config.clone();
        c.mode = "audit".into();
        c.lease_secs = None;
        (c != GuardConfig::default()).then_some(c)
    }
}

pub(super) fn on_guard(sh: &SharedState, bus: &broadcast::Sender<StreamEvent>, b: &[u8]) {
    if b.len() < std::mem::size_of::<GuardEvent>() {
        return;
    }
    let ev: GuardEvent = unsafe { std::ptr::read_unaligned(b.as_ptr() as *const GuardEvent) };
    let n = ev.comm.iter().position(|c| *c == 0).unwrap_or(16);
    let (hook, detail) = match ev.hook {
        GUARD_HOOK_EXEC => (
            "exec",
            format!(
                "binary not allowlisted (dev {}:{}, inode {})",
                ev.a >> 20,
                ev.a & 0xfffff,
                ev.b
            ),
        ),
        GUARD_HOOK_MPROTECT => (
            "mprotect",
            format!(
                "writable+executable mapping (prot {:#x}, vm_flags {:#x})",
                ev.a, ev.b
            ),
        ),
        _ => (
            "open",
            format!(
                "char device {}:{}{}",
                ev.a,
                ev.b,
                char_name(ev.a)
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default()
            ),
        ),
    };
    let rec = {
        let mut s = lock(sh);
        let rec = GuardRecord {
            ts: mono_to_rfc3339(ev.ts_ns),
            hook: hook.into(),
            denied: ev.denied != 0,
            vm: s.guard_cgroups.get(&ev.cgroup_id).cloned(),
            tgid: ev.tgid,
            pid: ev.pid,
            comm: String::from_utf8_lossy(&ev.comm[..n]).into_owned(),
            detail,
        };
        Shared::push_capped(&mut s.guard, rec.clone(), GUARD_STORE_CAP);
        rec
    };
    publish(bus, "guard", &rec);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_dev_encoding() {
        let st = libc::makedev(8, 1) as u64;
        assert_eq!(kdev(st), (8 << 20) | 1);
        let st = libc::makedev(259, 300) as u64;
        assert_eq!(kdev(st), (259 << 20) | 300);
    }

    #[test]
    fn defaults_are_audit() {
        let c = GuardConfig::default();
        assert!(!c.enabled && c.mode == "audit" && c.exec && c.wx && c.devices);
    }
}
