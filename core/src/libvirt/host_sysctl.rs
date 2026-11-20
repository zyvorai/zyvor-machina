// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Recommended sysctl values for hypervisor / high-concurrency networking.
//!
//! Adapted from common Linux tuning guides. Deprecated knobs from older snippets are omitted.
//! `rmem_max` / `wmem_max` are set **not less than** the default socket buffer sizes so defaults
//! do not exceed maxima.

use serde::Serialize;
use std::process::Command;

/// Drop-in filename for `/etc/sysctl.d/`.
pub const DROPIN_FILENAME: &str = "99-machina-host-net.conf";

#[derive(Debug, Clone, Serialize)]
pub struct SysctlTuningRow {
    pub key: String,
    pub recommended: String,
    pub current: Option<String>,
    pub current_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SysctlTuningResponse {
    pub dropin_path: String,
    pub recommended_conf: String,
    pub rows: Vec<SysctlTuningRow>,
    pub notes: Vec<String>,
}

/// (sysctl key, value as written in sysctl.conf — may contain spaces for multi-value keys)
const TUNING: &[(&str, &str)] = &[
    // Memory / fs
    ("fs.file-max", "2097152"),
    ("vm.swappiness", "10"),
    ("vm.dirty_ratio", "60"),
    ("vm.dirty_background_ratio", "2"),
    // TCP behaviour
    ("net.ipv4.tcp_synack_retries", "2"),
    ("net.ipv4.ip_local_port_range", "2000 65535"),
    ("net.ipv4.tcp_rfc1337", "1"),
    ("net.ipv4.tcp_fin_timeout", "15"),
    ("net.ipv4.tcp_keepalive_time", "300"),
    ("net.ipv4.tcp_keepalive_probes", "5"),
    ("net.ipv4.tcp_keepalive_intvl", "15"),
    // Core socket buffers (max >= default)
    ("net.core.rmem_default", "31457280"),
    ("net.core.rmem_max", "134217728"),
    ("net.core.wmem_default", "31457280"),
    ("net.core.wmem_max", "134217728"),
    ("net.core.somaxconn", "4096"),
    ("net.core.netdev_max_backlog", "65536"),
    ("net.core.optmem_max", "25165824"),
    // TCP/UDP memory (pages for tcp_mem / udp_mem)
    ("net.ipv4.tcp_mem", "65536 131072 262144"),
    ("net.ipv4.udp_mem", "65536 131072 262144"),
    ("net.ipv4.tcp_rmem", "8192 87380 16777216"),
    ("net.ipv4.udp_rmem_min", "16384"),
    ("net.ipv4.tcp_wmem", "8192 65536 16777216"),
    ("net.ipv4.udp_wmem_min", "16384"),
    ("net.ipv4.tcp_max_tw_buckets", "1440000"),
    ("net.ipv4.tcp_tw_reuse", "1"),
];

fn find_sysctl() -> String {
    for c in ["/usr/sbin/sysctl", "/sbin/sysctl", "/bin/sysctl"] {
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    "sysctl".to_string()
}

/// Read current runtime value for `key` via `sysctl -n`.
fn sysctl_read_n(key: &str) -> Result<String, String> {
    let out = Command::new(find_sysctl())
        .args(["-n", key])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "sysctl failed".to_string()
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .replace('\t', " "))
}

/// Build the recommended `/etc/sysctl.d/…` file contents.
pub fn recommended_sysctl_conf() -> String {
    let mut s = String::from(
        "# machina - host networking / concurrency sysctl (review before apply)\n\
         # Install: sudo install -m 644 … /etc/sysctl.d/",
    );
    s.push_str(DROPIN_FILENAME);
    s.push_str("\n# Load: sudo sysctl --system   or   sudo sysctl -p /etc/sysctl.d/");
    s.push_str(DROPIN_FILENAME);
    s.push_str("\n\n");

    s.push_str("### Memory / VFS ###\n\n");
    for (k, v) in &TUNING[..4] {
        s.push_str(&format!("{k} = {v}\n"));
    }
    s.push_str("\n### TCP / general IPv4 ###\n\n");
    for (k, v) in &TUNING[4..11] {
        s.push_str(&format!("{k} = {v}\n"));
    }
    s.push_str("\n### Socket and throughput tuning ###\n\n");
    for (k, v) in &TUNING[11..] {
        s.push_str(&format!("{k} = {v}\n"));
    }
    s.push('\n');
    s
}

fn tuning_notes() -> Vec<String> {
    vec![
        "Match \"no\" only means the live value differs from this generic snippet; many hosts are already tuned and may legitimately be better for your workload.".to_string(),
        "net.core rmem/wmem maxima in the snippet are set so max >= default (some published guides had this reversed).".to_string(),
        "net.ipv4.tcp_mem and net.ipv4.udp_mem are in units of memory pages (typically 4096 bytes); adjust for very small or very large RAM if needed.".to_string(),
        "After installing the drop-in, run: sudo sysctl --system".to_string(),
    ]
}

/// JSON payload for the Host Networking UI: recommended file text plus per-key current values.
pub fn sysctl_tuning_report() -> SysctlTuningResponse {
    let dropin_path = format!("/etc/sysctl.d/{DROPIN_FILENAME}");
    let recommended_conf = recommended_sysctl_conf();
    let mut rows = Vec::with_capacity(TUNING.len());

    for (key, recommended) in TUNING {
        match sysctl_read_n(key) {
            Ok(current) => rows.push(SysctlTuningRow {
                key: (*key).to_string(),
                recommended: (*recommended).to_string(),
                current: Some(current),
                current_error: None,
            }),
            Err(e) => rows.push(SysctlTuningRow {
                key: (*key).to_string(),
                recommended: (*recommended).to_string(),
                current: None,
                current_error: Some(e),
            }),
        }
    }

    SysctlTuningResponse {
        dropin_path,
        recommended_conf,
        rows,
        notes: tuning_notes(),
    }
}
