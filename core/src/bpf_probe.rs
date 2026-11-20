// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host eBPF capability probe: kernel features the native datapath
//! (machina-bpfd) needs, plus optional `bpftool` program counts.

use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::process::Command;

/// Default machina-bpfd control socket (overridden by `MACHINA_BPFD_SOCK`).
pub const BPFD_SOCKET: &str = "/run/machina-bpf/bpfd.sock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BpfProbeSummary {
    /// Kernel exposes BPF (BTF or a mounted bpffs).
    pub available: bool,
    pub kernel: String,
    pub btf: bool,
    pub bpffs: bool,
    /// TCX attach (kernel >= 6.6); older kernels fall back to clsact.
    pub tcx: bool,
    pub lsm_bpf: bool,
    pub jit_enabled: bool,
    pub unprivileged_bpf_disabled: bool,
    pub bpfd_socket: String,
    pub bpfd_present: bool,
    /// Empty when bpftool is not installed; the counts below are then 0.
    pub bpftool_path: String,
    pub program_count: u32,
    pub map_count: u32,
    pub cgroup_program_count: u32,
    pub tracepoint_count: u32,
    pub notes: Vec<String>,
}

#[cfg(target_os = "linux")]
fn find_bpftool() -> Option<String> {
    ["/usr/sbin/bpftool", "/sbin/bpftool", "/usr/bin/bpftool"]
        .into_iter()
        .find(|c| std::path::Path::new(c).exists())
        .map(String::from)
}

#[cfg(target_os = "linux")]
fn count_prog_lines(bpftool: &str, args: &[&str]) -> u32 {
    let Ok(o) = Command::new(bpftool).args(args).output() else {
        return 0;
    };
    if !o.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            // `bpftool prog show` prints one "<id>: <type> ..." header per object.
            t.split(':')
                .next()
                .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        })
        .count() as u32
}

#[cfg(target_os = "linux")]
fn read_trim(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// `(major, minor)` from a release string like `6.8.0-45-generic`.
pub fn kernel_version(release: &str) -> (u32, u32) {
    let mut it = release
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

pub fn probe_bpf_summary() -> BpfProbeSummary {
    #[cfg(not(target_os = "linux"))]
    {
        BpfProbeSummary {
            notes: vec!["eBPF requires a Linux host".into()],
            ..Default::default()
        }
    }
    #[cfg(target_os = "linux")]
    {
        let kernel = read_trim("/proc/sys/kernel/osrelease");
        let btf = std::path::Path::new("/sys/kernel/btf/vmlinux").exists();
        let bpffs = std::fs::read_to_string("/proc/mounts")
            .unwrap_or_default()
            .lines()
            .any(|l| l.split_whitespace().nth(2) == Some("bpf"));
        let tcx = kernel_version(&kernel) >= (6, 6);
        let lsm_bpf = read_trim("/sys/kernel/security/lsm")
            .split(',')
            .any(|m| m == "bpf");
        let jit_enabled = read_trim("/proc/sys/net/core/bpf_jit_enable") != "0";
        let unprivileged_bpf_disabled =
            read_trim("/proc/sys/kernel/unprivileged_bpf_disabled") != "0";
        let bpfd_socket =
            std::env::var("MACHINA_BPFD_SOCK").unwrap_or_else(|_| BPFD_SOCKET.to_string());
        let bpfd_present = std::path::Path::new(&bpfd_socket).exists();

        let mut notes = Vec::new();
        if !btf {
            notes.push(
                "kernel BTF (/sys/kernel/btf/vmlinux) missing — process telemetry is limited"
                    .into(),
            );
        }
        if !tcx {
            notes.push(format!(
                "kernel {kernel} predates TCX (6.6); tc hooks use clsact"
            ));
        }
        if !bpfd_present {
            notes.push(format!("machina-bpfd not running ({bpfd_socket} absent)"));
        }

        let mut s = BpfProbeSummary {
            available: btf || bpffs,
            kernel,
            btf,
            bpffs,
            tcx,
            lsm_bpf,
            jit_enabled,
            unprivileged_bpf_disabled,
            bpfd_socket,
            bpfd_present,
            notes,
            ..Default::default()
        };
        if let Some(bpftool) = find_bpftool() {
            s.program_count = count_prog_lines(&bpftool, &["prog", "show"]);
            s.map_count = count_prog_lines(&bpftool, &["map", "show"]);
            s.cgroup_program_count =
                count_prog_lines(&bpftool, &["prog", "show", "type", "cgroup_skb"])
                    + count_prog_lines(&bpftool, &["prog", "show", "type", "cgroup_sock_addr"]);
            s.tracepoint_count =
                count_prog_lines(&bpftool, &["prog", "show", "type", "tracepoint"]);
            if s.program_count == 0 {
                s.notes
                    .push("bpftool listed no programs (needs CAP_SYS_ADMIN)".into());
            }
            s.bpftool_path = bpftool;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kernel_versions() {
        assert_eq!(kernel_version("6.8.0-45-generic"), (6, 8));
        assert_eq!(kernel_version("7.0.1"), (7, 0));
        assert_eq!(kernel_version(""), (0, 0));
        assert!(kernel_version("6.10.2") >= (6, 6));
    }
}
