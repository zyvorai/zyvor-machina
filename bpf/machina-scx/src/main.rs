// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `machina-scx`: loads the scx_machina struct_ops scheduler, takes vCPU
//! profiles on stdin and reports per-VM stats on stdout every second. Runs
//! until stdin closes or the kernel ejects the scheduler.

#[cfg(not(feature = "scx"))]
fn main() {
    println!(r#"{{"error":"machina-scx was built without the `scx` feature"}}"#);
    std::process::exit(2);
}

#[cfg(feature = "scx")]
fn main() {
    if let Err(e) = scx::run() {
        let out = machina_scx::Output::Error {
            error: format!("{e:#}"),
        };
        println!("{}", serde_json::to_string(&out).unwrap_or_default());
        std::process::exit(1);
    }
}

#[cfg(feature = "scx")]
mod scx {
    use std::collections::HashSet;
    use std::io::{BufRead, Write};
    use std::mem::MaybeUninit;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use anyhow::{anyhow, Context, Result};
    use libbpf_rs::skel::{OpenSkel, SkelBuilder};
    use libbpf_rs::{MapCore, MapFlags};
    use machina_scx::{kernel_state, Command, Output, Profile, VmStats};

    mod skel {
        include!(concat!(env!("OUT_DIR"), "/scx_machina.skel.rs"));
    }
    use skel::*;

    /// `struct task_profile` in scx_machina.bpf.c.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RawProfile {
        vm_key: u64,
        tgid: u32,
        weight: u32,
        slice_ns: u64,
        latency_target_ns: u64,
    }

    fn bytes<T: Copy>(v: &T) -> &[u8] {
        unsafe { std::slice::from_raw_parts((v as *const T).cast(), std::mem::size_of::<T>()) }
    }

    fn emit(o: &Output) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", serde_json::to_string(o).unwrap_or_default());
        let _ = out.flush();
    }

    pub fn run() -> Result<()> {
        match kernel_state().as_deref() {
            None => {
                return Err(anyhow!(
                    "kernel has no sched_ext ({} missing)",
                    machina_scx::STATE_PATH
                ))
            }
            Some("disabled") => {}
            Some(s) => return Err(anyhow!("another sched_ext scheduler is active (state {s})")),
        }
        let mut obj = MaybeUninit::uninit();
        let open = ScxMachinaSkelBuilder::default()
            .open(&mut obj)
            .context("open scx_machina")?;
        let mut skel = open.load().context("load scx_machina")?;
        let _link = skel
            .maps
            .machina_scx_ops
            .attach_struct_ops()
            .context("attach machina_scx_ops")?;
        emit(&Output::Ready { ready: true });

        let (tx, rx) = mpsc::channel::<Option<String>>();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(l) => {
                        if tx.send(Some(l)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(None);
        });

        let mut tids: HashSet<u32> = HashSet::new();
        let mut vms: HashSet<u64> = HashSet::new();
        let mut last = Instant::now() - Duration::from_secs(1);
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Ok(Some(line)) if !line.trim().is_empty() => {
                    match serde_json::from_str::<Command>(&line) {
                        Ok(Command::Profiles { profiles }) => {
                            apply(&skel, &profiles, &mut tids, &mut vms)?
                        }
                        Err(e) => emit(&Output::Error {
                            error: format!("bad command: {e}"),
                        }),
                    }
                }
                _ => {}
            }
            let exit_kind = skel
                .maps
                .scx_meta
                .lookup(&0u32.to_ne_bytes(), MapFlags::ANY)?
                .map(|v| u64::from_ne_bytes(v[..8].try_into().unwrap_or_default()))
                .unwrap_or(0);
            if last.elapsed() >= Duration::from_secs(1) || exit_kind != 0 {
                let mut stats = Vec::new();
                for vm in &vms {
                    if let Some(v) = skel
                        .maps
                        .scx_vm_stats
                        .lookup(&vm.to_ne_bytes(), MapFlags::ANY)?
                    {
                        let f = |i: usize| {
                            u64::from_ne_bytes(v[i * 8..i * 8 + 8].try_into().unwrap_or_default())
                        };
                        stats.push((
                            *vm,
                            VmStats {
                                enqueues: f(0),
                                direct_dispatches: f(1),
                                shared_dispatches: f(2),
                                running_calls: f(3),
                                runtime_ns: f(4),
                                queue_delay_ns: f(5),
                                queue_delay_max_ns: f(6),
                                latency_violations: f(7),
                            },
                        ));
                    }
                }
                emit(&Output::Stats { stats, exit_kind });
                last = Instant::now();
            }
            if exit_kind != 0 {
                std::process::exit(3);
            }
        }
    }

    fn apply(
        skel: &ScxMachinaSkel,
        profiles: &[Profile],
        tids: &mut HashSet<u32>,
        vms: &mut HashSet<u64>,
    ) -> Result<()> {
        let want: HashSet<u32> = profiles.iter().map(|p| p.tid).collect();
        for tid in tids.difference(&want) {
            let _ = skel.maps.scx_task_profiles.delete(&tid.to_ne_bytes());
        }
        for p in profiles {
            let raw = RawProfile {
                vm_key: p.vm,
                tgid: p.tgid,
                weight: p.weight,
                slice_ns: p.slice_ns,
                latency_target_ns: p.latency_target_ns,
            };
            skel.maps
                .scx_task_profiles
                .update(&p.tid.to_ne_bytes(), bytes(&raw), MapFlags::ANY)?;
            if vms.insert(p.vm) {
                skel.maps
                    .scx_vm_stats
                    .update(&p.vm.to_ne_bytes(), bytes(&[0u64; 8]), MapFlags::NO_EXIST)
                    .ok();
            }
        }
        *tids = want;
        Ok(())
    }
}
