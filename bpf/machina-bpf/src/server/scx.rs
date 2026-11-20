// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Supervises the `machina-scx` helper (sched_ext struct_ops, loaded with
//! libbpf). bpfd hands it vCPU profiles, moves those threads to SCHED_EXT
//! and back, and stops the helper on request, lease expiry or helper exit.
//! Closing the helper's stdin unloads the scheduler; the kernel then runs
//! everything on CFS again.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::Mutex;

use machina_scx::{Command as ScxCmd, Output, Profile, VmStats};

use super::*;
use crate::vmintel;

const SCHED_OTHER: u32 = 0;
const SCHED_EXT: u32 = 7;
pub(super) const SCX_MAX_LEASE: u64 = 3600;
const SYSFS: &str = "/sys/kernel/sched_ext";

#[derive(Default)]
struct Shared {
    ready: Option<std::result::Result<(), String>>,
    stats: BTreeMap<u64, VmStats>,
    exit_kind: u64,
}

#[derive(Default)]
pub(super) struct ScxRuntime {
    config: ScxConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    shared: Arc<Mutex<Shared>>,
    /// (tid, vm key)
    tids: Vec<(u32, u64)>,
    names: BTreeMap<u64, (String, usize)>,
    deadline_mono: u64,
    lapsed: bool,
    last_exit: Option<String>,
}

impl Drop for ScxRuntime {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn sysfs(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{SYSFS}/{name}"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MACHINA_SCX_BIN") {
        return Some(PathBuf::from(p));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("machina-scx")));
    beside
        .into_iter()
        .chain([
            "/usr/local/bin/machina-scx".into(),
            "/usr/bin/machina-scx".into(),
        ])
        .find(|p| p.exists())
}

/// `sched_setattr(tid, policy)` keeping the thread's nice value.
fn set_policy(tid: u32, policy: u32) -> std::io::Result<()> {
    #[repr(C)]
    struct SchedAttr {
        size: u32,
        policy: u32,
        flags: u64,
        nice: i32,
        priority: u32,
        runtime: u64,
        deadline: u64,
        period: u64,
    }
    unsafe { *libc::__errno_location() = 0 };
    let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, tid) };
    let nice = if nice == -1 && std::io::Error::last_os_error().raw_os_error().unwrap_or(0) != 0 {
        0
    } else {
        nice
    };
    let attr = SchedAttr {
        size: std::mem::size_of::<SchedAttr>() as u32,
        policy,
        flags: 0,
        nice,
        priority: 0,
        runtime: 0,
        deadline: 0,
        period: 0,
    };
    let rc = unsafe {
        libc::syscall(
            libc::SYS_sched_setattr,
            tid as libc::pid_t,
            &attr as *const SchedAttr,
            0u32,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// vCPU threads (`CPU n/KVM`) of a process.
fn vcpu_threads(pid: u32) -> Vec<(u32, u32)> {
    let Ok(rd) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|tid| {
            let comm =
                std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/comm")).unwrap_or_default();
            vmintel::vcpu_index(&comm).is_some()
        })
        .map(|tid| (tid, pid))
        .collect()
}

pub(super) fn validate(c: &ScxConfig) -> Result<u64> {
    let secs = c
        .lease_secs
        .ok_or_else(|| anyhow!("sched_ext needs `lease_secs` (1..={SCX_MAX_LEASE})"))?;
    if !(1..=SCX_MAX_LEASE).contains(&secs) {
        return Err(anyhow!("lease_secs must be 1..={SCX_MAX_LEASE}"));
    }
    if c.vms.is_empty() && c.extra.is_empty() {
        return Err(anyhow!("name at least one VM"));
    }
    Ok(secs)
}

impl Engine {
    pub(super) fn scx_configure(&mut self, c: ScxConfig) -> Result<ScxStatus> {
        if !c.enabled {
            self.scx_stop("stopped on request");
            self.scx.config = c;
            return Ok(self.scx_status());
        }
        let secs = validate(&c)?;
        if sysfs("state").is_none() {
            return Err(anyhow!(
                "this kernel has no sched_ext (CONFIG_SCHED_CLASS_EXT)"
            ));
        }
        let helper = helper_path()
            .ok_or_else(|| anyhow!("machina-scx helper not found; set MACHINA_SCX_BIN"))?;

        // Resolve vCPU threads before touching the scheduler.
        let scopes = vm::qemu_scopes();
        let mut targets: Vec<(String, Vec<(u32, u32)>)> = Vec::new();
        for name in &c.vms {
            let rel = scopes
                .get(name)
                .ok_or_else(|| anyhow!("VM {name} is not running"))?;
            let threads: Vec<(u32, u32)> = super::vmintel::discover(rel)
                .threads
                .into_iter()
                .filter(|(_, _, v)| v.is_some())
                .map(|(t, g, _)| (t, g))
                .collect();
            targets.push((name.clone(), threads));
        }
        for t in &c.extra {
            targets.push((t.name.clone(), vcpu_threads(t.pid)));
        }
        if let Some((name, _)) = targets.iter().find(|(_, th)| th.is_empty()) {
            return Err(anyhow!("no vCPU threads (`CPU n/KVM`) found for {name}"));
        }

        self.scx_stop("restarted");
        match sysfs("state").as_deref() {
            Some("disabled") => {}
            Some(s) => return Err(anyhow!("another sched_ext scheduler is active (state {s})")),
            None => {}
        }
        let latency = c.latency_target_us.unwrap_or(0) * 1000;
        let mut profiles = Vec::new();
        let mut names = BTreeMap::new();
        for (key, (name, threads)) in targets.iter().enumerate() {
            names.insert(key as u64, (name.clone(), threads.len()));
            for (tid, tgid) in threads {
                profiles.push(Profile {
                    tid: *tid,
                    tgid: *tgid,
                    vm: key as u64,
                    weight: 100,
                    slice_ns: 1_000_000,
                    latency_target_ns: latency,
                });
            }
        }

        let mut child = Proc::new(&helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {}", helper.display()))?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("helper stdout"))?;
        let sh = shared.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                let Ok(out) = serde_json::from_str::<Output>(&line) else {
                    continue;
                };
                let mut s = sh.lock().unwrap_or_else(|e| e.into_inner());
                match out {
                    Output::Ready { .. } => s.ready = Some(Ok(())),
                    Output::Error { error } => {
                        if s.ready.is_none() {
                            s.ready = Some(Err(error));
                        }
                    }
                    Output::Stats { stats, exit_kind } => {
                        s.stats = stats.into_iter().collect();
                        s.exit_kind = exit_kind;
                    }
                }
            }
        });
        let start = Instant::now();
        let ready = loop {
            if let Some(r) = shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .ready
                .clone()
            {
                break r;
            }
            if let Ok(Some(st)) = child.try_wait() {
                break Err(format!("helper exited ({st}) before attaching"));
            }
            if start.elapsed() > Duration::from_secs(10) {
                break Err("helper did not attach within 10s".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if let Err(e) = ready {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("machina-scx: {e}"));
        }
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("helper stdin"))?;
        let mut line = serde_json::to_vec(&ScxCmd::Profiles {
            profiles: profiles.clone(),
        })?;
        line.push(b'\n');
        stdin.write_all(&line)?;
        stdin.flush()?;

        let mut tids = Vec::new();
        let mut failed = 0;
        for p in &profiles {
            match set_policy(p.tid, SCHED_EXT) {
                Ok(()) => tids.push((p.tid, p.vm)),
                Err(e) => {
                    failed += 1;
                    tracing::warn!(tid = p.tid, "sched_setattr SCHED_EXT: {e}");
                }
            }
        }
        tracing::warn!(
            vcpus = tids.len(),
            failed,
            secs,
            "sched_ext scheduler running"
        );
        self.scx = ScxRuntime {
            config: c,
            child: Some(child),
            stdin: Some(stdin),
            shared,
            tids,
            names,
            deadline_mono: loader::monotonic_ns() + secs * 1_000_000_000,
            lapsed: false,
            last_exit: None,
        };
        Ok(self.scx_status())
    }

    /// Threads back to CFS first, then close the helper's stdin (it unloads
    /// the scheduler and exits); kill it if it lingers.
    fn scx_stop(&mut self, why: &str) {
        let Some(mut child) = self.scx.child.take() else {
            return;
        };
        for (tid, _) in std::mem::take(&mut self.scx.tids) {
            let _ = set_policy(tid, SCHED_OTHER);
        }
        drop(self.scx.stdin.take());
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Ok(None) = child.try_wait() {
            let _ = child.kill();
            let _ = child.wait();
        }
        tracing::warn!("sched_ext scheduler stopped: {why}");
        self.scx.last_exit = Some(why.to_string());
    }

    /// Maintenance: lease expiry, helper exit or kernel ejection.
    pub(super) fn scx_tick(&mut self) {
        if self.scx.child.is_none() {
            return;
        }
        if loader::monotonic_ns() >= self.scx.deadline_mono {
            self.scx_stop("lease expired");
            self.scx.lapsed = true;
            return;
        }
        let exited = self
            .scx
            .child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten());
        let kind = self
            .scx
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit_kind;
        if let Some(st) = exited {
            self.scx_stop(&format!("helper exited ({st}); kernel exit kind {kind}"));
        } else if kind != 0 {
            self.scx_stop(&format!("kernel ejected the scheduler (exit kind {kind})"));
        }
    }

    pub(super) fn scx_status(&mut self) -> ScxStatus {
        let now = loader::monotonic_ns();
        let running = self.scx.child.is_some();
        let stats = self
            .scx
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stats
            .clone();
        let vms = self
            .scx
            .names
            .iter()
            .map(|(key, (name, vcpus))| {
                let s = stats.get(key).copied().unwrap_or_default();
                ScxVmStatus {
                    name: name.clone(),
                    vcpus: *vcpus,
                    enqueues: s.enqueues,
                    dispatches: s.running_calls,
                    avg_queue_delay_us: if s.running_calls > 0 {
                        s.queue_delay_ns as f64 / s.running_calls as f64 / 1000.0
                    } else {
                        0.0
                    },
                    max_queue_delay_us: s.queue_delay_max_ns as f64 / 1000.0,
                    runtime_ms: s.runtime_ns / 1_000_000,
                    latency_violations: s.latency_violations,
                }
            })
            .collect();
        let mut notes = Vec::new();
        let helper = helper_path();
        if helper.is_none() {
            notes.push("machina-scx helper not installed (build with `--features scx`)".into());
        }
        ScxStatus {
            supported: sysfs("state").is_some(),
            helper: helper.map(|p| p.display().to_string()),
            running,
            kernel_state: sysfs("state").unwrap_or_else(|| "unsupported".into()),
            ops: sysfs("root/ops"),
            nr_rejected: sysfs("nr_rejected")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            lease_remaining_secs: (running && now < self.scx.deadline_mono)
                .then(|| (self.scx.deadline_mono - now) / 1_000_000_000),
            lease_expired: self.scx.lapsed,
            last_exit: self.scx.last_exit.clone(),
            vms,
            notes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scx_needs_a_lease_and_targets() {
        let mut c = ScxConfig {
            enabled: true,
            vms: vec!["a".into()],
            ..Default::default()
        };
        assert!(validate(&c).is_err());
        c.lease_secs = Some(0);
        assert!(validate(&c).is_err());
        c.lease_secs = Some(7200);
        assert!(validate(&c).is_err());
        c.lease_secs = Some(60);
        assert_eq!(validate(&c).unwrap(), 60);
        c.vms.clear();
        assert!(validate(&c).is_err());
    }
}
