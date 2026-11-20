// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Distro-aware host insight: package updates, accounts, network counters, firewall summary.
//! Targets common families: Debian/Ubuntu (apt), Fedora/RHEL (dnf/microdnf/yum), Arch (pacman),
//! openSUSE (zypper). Probes are read-only; [`package_upgrade`], [`package_install`], and
//! [`package_remove`] are mutating (Linux). Uses `getent` where available so LDAP/NIS users appear.

use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use crate::LibvirtError;

// ── Binary resolution & subprocess budget (Linux probes only) ───

#[cfg(target_os = "linux")]
fn find_bin(name: &str) -> String {
    let candidates = [
        format!("/usr/bin/{name}"),
        format!("/usr/sbin/{name}"),
        format!("/sbin/{name}"),
        format!("/bin/{name}"),
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return c.clone();
        }
    }
    name.to_string()
}

// ── Package manager detection ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageUpdateItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<String>,
    #[serde(default)]
    pub security: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageUpdateCheck {
    /// `apt`, `dnf`, `microdnf`, `yum`, `apk`, `pacman`, `zypper`, or `unknown`
    pub backend: String,
    /// True if a probe command ran (may still have `pending: None` on parse failure).
    pub probed: bool,
    pub pending_count: Option<u32>,
    /// Human-readable one-line summary when available.
    pub summary: Option<String>,
    /// Extra context (timeouts, simulate-only, cache-only, etc.).
    pub hint: Option<String>,
    /// stderr or error text when probe failed.
    pub error: Option<String>,
    /// Linux: true when `/var/run/reboot-required` exists (Debian/Ubuntu family after many upgrades).
    #[serde(default)]
    pub reboot_required: bool,
    #[serde(default)]
    pub packages: Vec<PackageUpdateItem>,
}

/// Prefer Debian family before RPM so Ubuntu WSL with stray `dnf` still uses apt.
pub fn detect_package_backend() -> &'static str {
    if Path::new("/usr/bin/apt-get").exists() || Path::new("/bin/apt-get").exists() {
        return "apt";
    }
    if Path::new("/usr/bin/microdnf").exists() {
        return "microdnf";
    }
    if Path::new("/usr/bin/dnf").exists() {
        return "dnf";
    }
    if Path::new("/usr/bin/yum").exists() {
        return "yum";
    }
    // Alpine Linux (containers and minimal hosts)
    if Path::new("/sbin/apk").exists() || Path::new("/usr/sbin/apk").exists() {
        return "apk";
    }
    if Path::new("/usr/bin/pacman").exists() {
        return "pacman";
    }
    if Path::new("/usr/sbin/zypper").exists() || Path::new("/usr/bin/zypper").exists() {
        return "zypper";
    }
    "unknown"
}

#[cfg(target_os = "linux")]
const PROBE_BUDGET: Duration = Duration::from_secs(45);

#[cfg(target_os = "linux")]
fn run_with_budget(
    cmd: &mut Command,
    budget: Duration,
) -> Result<std::process::Output, LibvirtError> {
    let start = Instant::now();
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| LibvirtError::Operation(format!("spawn failed: {e}")))?;
    loop {
        if start.elapsed() > budget {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LibvirtError::Operation(
                "subprocess timed out (distro tools can be slow; try again)".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut out, &mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_end(&mut err, &mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(80)),
            Err(e) => {
                let _ = child.kill();
                return Err(LibvirtError::Operation(format!("wait failed: {e}")));
            }
        }
    }
}

/// Read-only update counts / summaries. Best-effort per distro; never runs `apt upgrade` / `dnf install`.
pub fn check_package_updates() -> Result<PackageUpdateCheck, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(PackageUpdateCheck {
            backend: "unknown".into(),
            probed: false,
            pending_count: None,
            summary: None,
            hint: Some("Only available on Linux hypervisors".into()),
            error: None,
            reboot_required: false,
            packages: Vec::new(),
        });
    }
    #[cfg(target_os = "linux")]
    {
        check_package_updates_linux()
    }
}

#[cfg(target_os = "linux")]
fn push_package_item(
    out: &mut PackageUpdateCheck,
    name: &str,
    current: Option<&str>,
    available: Option<&str>,
    security: bool,
) {
    if out.packages.len() >= 64 {
        return;
    }
    out.packages.push(PackageUpdateItem {
        name: name.to_string(),
        current: current.map(String::from),
        available: available.map(String::from),
        security,
    });
}

#[cfg(target_os = "linux")]
fn check_package_updates_linux() -> Result<PackageUpdateCheck, LibvirtError> {
    let backend = detect_package_backend();
    let mut out = PackageUpdateCheck {
        backend: backend.to_string(),
        probed: true,
        pending_count: None,
        summary: None,
        hint: None,
        error: None,
        reboot_required: false,
        packages: Vec::new(),
    };

    let r = match backend {
        "apt" => probe_apt_updates(&mut out),
        "dnf" => probe_dnf_updates("dnf", &mut out),
        "microdnf" => probe_dnf_updates("microdnf", &mut out),
        "yum" => probe_yum_updates(&mut out),
        "pacman" => probe_pacman_updates(&mut out),
        "apk" => probe_apk_updates(&mut out),
        "zypper" => probe_zypper_updates(&mut out),
        _ => {
            out.hint = Some(
                "No supported package manager detected (expected apt, dnf, yum, apk, pacman, or zypper)"
                    .into(),
            );
            Ok(())
        }
    };
    if let Err(e) = r {
        out.error = Some(e.to_string());
    }
    out.reboot_required = std::path::Path::new("/var/run/reboot-required").exists();
    Ok(out)
}

#[cfg(target_os = "linux")]
fn probe_apt_updates(out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    // Simulate only; pipe through `tail` so huge apt output cannot fill the pipe buffer.
    let ag = find_bin("apt-get");
    let mut cmd = Command::new(find_bin("sh"));
    cmd.arg("-c").arg(format!(
        "set -o pipefail; DEBIAN_FRONTEND=noninteractive {ag} -qq -s upgrade 2>&1 | tail -n 80"
    ));
    match run_with_budget(&mut cmd, PROBE_BUDGET) {
        Ok(output) => {
            let combined = String::from_utf8_lossy(&output.stdout);
            if !output.status.success() {
                out.error = Some(format!(
                    "apt-get -s upgrade failed: {}",
                    combined.trim().chars().take(500).collect::<String>()
                ));
                out.hint = Some(
                    "Debian/Ubuntu: needs readable apt lists; run `apt update` on the host if lists are stale."
                        .into(),
                );
                return Ok(());
            }
            let mut upgraded = None;
            for line in combined.lines().rev() {
                if line.contains(" upgraded") {
                    let head = line.split(" upgraded").next().unwrap_or("").trim();
                    if let Some(tok) = head.split_whitespace().last() {
                        if let Ok(n) = tok.parse::<u32>() {
                            upgraded = Some(n);
                            break;
                        }
                    }
                }
            }
            out.pending_count = upgraded;
            out.summary = upgraded.map(|n| format!("{n} packages would be upgraded (simulate)"));
            out.hint = Some(
                "Debian/Ubuntu: `apt-get -s upgrade` via sh+tail (no install performed)".into(),
            );
            for line in combined.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("Inst ") {
                    let name = rest.split_whitespace().next().unwrap_or("");
                    if !name.is_empty() {
                        let security = t.contains("-security") || t.contains("security");
                        push_package_item(out, name, None, None, security);
                    }
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_dnf_updates(bin: &str, out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    let exe = find_bin(bin);
    let mut cmd = Command::new(find_bin("sh"));
    cmd.arg("-c").arg(format!(
        "set -o pipefail; {exe} repoquery --upgrades --quiet --cacheonly 2>/dev/null | tail -n 4000 | wc -l"
    ));
    match run_with_budget(&mut cmd, PROBE_BUDGET) {
        Ok(output) => {
            let line = String::from_utf8_lossy(&output.stdout);
            let n: u32 = line.trim().parse().unwrap_or(0);
            out.pending_count = Some(n);
            out.summary = Some(format!(
                "{n} package(s) with newer versions (`{bin} repoquery --cacheonly`)"
            ));
            out.hint = Some(format!(
                "Fedora/RHEL-style: `{bin} repoquery` (tail+wc limits I/O). If 0 but updates exist, refresh metadata on the host."
            ));
            if !output.status.success() && n == 0 {
                let err = String::from_utf8_lossy(&output.stderr);
                if !err.trim().is_empty() {
                    out.error = Some(err.trim().chars().take(400).collect::<String>());
                }
            }
            let mut names_cmd = Command::new(find_bin("sh"));
            names_cmd.arg("-c").arg(format!(
                "set -o pipefail; {exe} repoquery --upgrades --qf '%{{name}}' --cacheonly 2>/dev/null | tail -n 64"
            ));
            if let Ok(names_out) = run_with_budget(&mut names_cmd, PROBE_BUDGET) {
                if names_out.status.success() {
                    for line in String::from_utf8_lossy(&names_out.stdout).lines() {
                        let name = line.trim();
                        if !name.is_empty() {
                            push_package_item(out, name, None, None, false);
                        }
                    }
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_yum_updates(out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    let mut cmd = Command::new(find_bin("yum"));
    cmd.args(["check-update", "-q"]);
    match run_with_budget(&mut cmd, PROBE_BUDGET) {
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            if code == 100 {
                out.pending_count = None;
                out.summary = Some(
                    "Updates available (yum check-update exit 100; count not enumerated)".into(),
                );
            } else if output.status.success() {
                out.pending_count = Some(0);
                out.summary = Some("No updates pending".into());
            } else {
                let err = String::from_utf8_lossy(&output.stderr);
                out.error = Some(err.trim().chars().take(400).collect::<String>());
            }
            out.hint =
                Some("RHEL/CentOS 7-style yum: exit 100 means at least one update exists".into());
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_apk_updates(out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    let apk = find_bin("apk");
    let mut cmd = Command::new(find_bin("sh"));
    cmd.arg("-c").arg(format!(
        "set -o pipefail; {apk} list -u 2>/dev/null | tail -n 4000 | wc -l"
    ));
    match run_with_budget(&mut cmd, Duration::from_secs(25)) {
        Ok(output) => {
            let n: u32 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            out.pending_count = Some(n);
            out.summary = Some(format!("{n} package(s) upgradable (`apk list -u`)"));
            out.hint = Some(
                "Alpine Linux: read-only count; run `apk upgrade` on the host to apply".into(),
            );
            if !output.status.success() && n == 0 {
                let e = String::from_utf8_lossy(&output.stderr);
                if !e.trim().is_empty() {
                    out.error = Some(e.trim().chars().take(400).collect::<String>());
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_pacman_updates(out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    let pm = find_bin("pacman");
    let mut cmd = Command::new(find_bin("sh"));
    cmd.arg("-c").arg(format!(
        "set -o pipefail; {pm} -Qu --color never 2>/dev/null | tail -n 4000 | wc -l"
    ));
    match run_with_budget(&mut cmd, Duration::from_secs(25)) {
        Ok(output) => {
            let n: u32 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
            out.pending_count = Some(n);
            out.summary = Some(format!("{n} package(s) pending (`pacman -Qu`, bounded)"));
            out.hint = Some("Arch Linux".into());
            if !output.status.success() && n == 0 {
                let e = String::from_utf8_lossy(&output.stderr);
                if !e.trim().is_empty() {
                    out.error = Some(e.trim().chars().take(400).collect::<String>());
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_zypper_updates(out: &mut PackageUpdateCheck) -> Result<(), LibvirtError> {
    let mut cmd = Command::new(find_bin("zypper"));
    cmd.args(["-n", "-q", "patch-check"]);
    match run_with_budget(&mut cmd, PROBE_BUDGET) {
        Ok(output) => {
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let mut pending = None;
            for line in text.lines() {
                if line.contains("patches needed") {
                    if let Some(tok) = line.split_whitespace().next() {
                        if let Ok(n) = tok.parse::<u32>() {
                            pending = Some(n);
                            break;
                        }
                    }
                }
            }
            out.pending_count = pending;
            out.summary = pending.map(|n| format!("{n} patches needed (zypper patch-check)"));
            out.hint = Some("openSUSE / SLE: counts from `zypper patch-check`".into());
            if !output.status.success() && pending.is_none() {
                let e = text.trim();
                if !e.is_empty() {
                    out.error = Some(e.chars().take(400).collect());
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    Ok(())
}

// ── /proc/net/dev ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetDevCounter {
    pub iface: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
}

/// Cumulative RX/TX since boot (kernel counters). Same semantics on all Linux distros.
pub fn list_net_dev_counters() -> Result<Vec<NetDevCounter>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        Ok(Vec::new())
    }
    #[cfg(target_os = "linux")]
    {
        parse_proc_net_dev()
    }
}

#[cfg(target_os = "linux")]
fn parse_proc_net_dev() -> Result<Vec<NetDevCounter>, LibvirtError> {
    let raw = std::fs::read_to_string("/proc/net/dev")
        .map_err(|e| LibvirtError::Operation(format!("read /proc/net/dev: {e}")))?;
    let mut out = Vec::new();
    for line in raw.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface.trim().to_string();
        if iface.is_empty() {
            continue;
        }
        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() < 16 {
            continue;
        }
        let rx_bytes: u64 = cols[0].parse().unwrap_or(0);
        let rx_packets: u64 = cols[1].parse().unwrap_or(0);
        let tx_bytes: u64 = cols[8].parse().unwrap_or(0);
        let tx_packets: u64 = cols[9].parse().unwrap_or(0);
        out.push(NetDevCounter {
            iface,
            rx_bytes,
            rx_packets,
            tx_bytes,
            tx_packets,
        });
    }
    Ok(out)
}

/// Instantaneous RX/TX derived from two `/proc/net/dev` samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetDevRate {
    pub iface: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub rx_packets_per_sec: f64,
    pub tx_packets_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetDevRatesResponse {
    /// Actual wait between samples (after clamping).
    pub sample_interval_ms: u64,
    pub interfaces: Vec<NetDevRate>,
}

/// Delta `/proc/net/dev` over `interval_ms` (clamped 50–5000). Non-Linux returns empty.
pub fn list_net_dev_rates(interval_ms: u64) -> Result<NetDevRatesResponse, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = interval_ms;
        Ok(NetDevRatesResponse {
            sample_interval_ms: 0,
            interfaces: Vec::new(),
        })
    }
    #[cfg(target_os = "linux")]
    {
        list_net_dev_rates_linux(interval_ms)
    }
}

#[cfg(target_os = "linux")]
fn list_net_dev_rates_linux(interval_ms: u64) -> Result<NetDevRatesResponse, LibvirtError> {
    let wait_ms = interval_ms.clamp(50, 5000);
    let first = parse_proc_net_dev()?;
    std::thread::sleep(std::time::Duration::from_millis(wait_ms));
    let second = parse_proc_net_dev()?;
    let dt = wait_ms as f64 / 1000.0;
    let m2: std::collections::HashMap<String, &NetDevCounter> =
        second.iter().map(|c| (c.iface.clone(), c)).collect();
    let mut interfaces = Vec::new();
    for a in &first {
        let Some(b) = m2.get(&a.iface) else { continue };
        if a.iface == "lo" {
            continue;
        }
        let drxb = b.rx_bytes.saturating_sub(a.rx_bytes) as f64 / dt;
        let dtxb = b.tx_bytes.saturating_sub(a.tx_bytes) as f64 / dt;
        let drxp = b.rx_packets.saturating_sub(a.rx_packets) as f64 / dt;
        let dtxp = b.tx_packets.saturating_sub(a.tx_packets) as f64 / dt;
        interfaces.push(NetDevRate {
            iface: a.iface.clone(),
            rx_bytes_per_sec: drxb.max(0.0),
            tx_bytes_per_sec: dtxb.max(0.0),
            rx_packets_per_sec: drxp.max(0.0),
            tx_packets_per_sec: dtxp.max(0.0),
        });
    }
    interfaces.sort_by(|x, y| x.iface.cmp(&y.iface));
    Ok(NetDevRatesResponse {
        sample_interval_ms: wait_ms,
        interfaces,
    })
}

// ── Passwd / group (getent with file fallback) ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswdEntry {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: String,
    pub shell: String,
    /// Debian/Ubuntu convention: uid < 1000 and not root; RHEL historically used < 500.
    pub system_account: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupEntry {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

#[cfg(target_os = "linux")]
fn read_passwd_source() -> Result<String, LibvirtError> {
    let out = Command::new("getent").arg("passwd").output();
    if let Ok(o) = out {
        if o.status.success() && !o.stdout.is_empty() {
            return Ok(String::from_utf8_lossy(&o.stdout).to_string());
        }
    }
    std::fs::read_to_string("/etc/passwd")
        .map_err(|e| LibvirtError::Operation(format!("passwd: {e}")))
}

#[cfg(target_os = "linux")]
fn read_group_source() -> Result<String, LibvirtError> {
    let out = Command::new("getent").arg("group").output();
    if let Ok(o) = out {
        if o.status.success() && !o.stdout.is_empty() {
            return Ok(String::from_utf8_lossy(&o.stdout).to_string());
        }
    }
    std::fs::read_to_string("/etc/group")
        .map_err(|e| LibvirtError::Operation(format!("group: {e}")))
}

/// NSS-aware passwd listing (LDAP/NIS if configured). `limit` capped at 500.
pub fn list_passwd_entries(limit: usize) -> Result<Vec<PasswdEntry>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = limit;
        return Ok(Vec::new());
    }
    #[cfg(target_os = "linux")]
    {
        let cap = limit.clamp(1, 500);
        let data = read_passwd_source()?;
        let mut rows = Vec::new();
        for line in data.lines() {
            if rows.len() >= cap {
                break;
            }
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 7 {
                continue;
            }
            let Ok(uid) = parts[2].parse::<u32>() else {
                continue;
            };
            let Ok(gid) = parts[3].parse::<u32>() else {
                continue;
            };
            let system_account = uid != 0 && uid < 1000;
            rows.push(PasswdEntry {
                username: parts[0].to_string(),
                uid,
                gid,
                gecos: parts[4].to_string(),
                home: parts[5].to_string(),
                shell: parts[6].to_string(),
                system_account,
            });
        }
        Ok(rows)
    }
}

/// NSS-aware group listing. `limit` capped at 500.
pub fn list_group_entries(limit: usize) -> Result<Vec<GroupEntry>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = limit;
        return Ok(Vec::new());
    }
    #[cfg(target_os = "linux")]
    {
        let cap = limit.clamp(1, 500);
        let data = read_group_source()?;
        let mut rows = Vec::new();
        for line in data.lines() {
            if rows.len() >= cap {
                break;
            }
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 4 {
                continue;
            }
            let Ok(gid) = parts[2].parse::<u32>() else {
                continue;
            };
            let members_str = parts.get(3).copied().unwrap_or("");
            let members = members_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            rows.push(GroupEntry {
                name: parts[0].to_string(),
                gid,
                members,
            });
        }
        Ok(rows)
    }
}

// ── Security / firewall summary (reuses libvirt host_network) ─────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSecuritySummary {
    pub network_backend: String,
    pub firewall_backend: String,
    pub ufw_status_line: Option<String>,
    pub firewalld_default_zone: Option<String>,
    pub selinux_mode: Option<String>,
}

pub fn host_security_summary() -> Result<HostSecuritySummary, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(HostSecuritySummary {
            network_backend: "unknown".into(),
            firewall_backend: "unknown".into(),
            ufw_status_line: None,
            firewalld_default_zone: None,
            selinux_mode: None,
        });
    }
    #[cfg(target_os = "linux")]
    {
        let (net, fw) = crate::libvirt::host_network::get_detected_backends();
        let mut ufw_status_line = None;
        if fw == "ufw" {
            if let Ok(o) = Command::new(find_bin("ufw")).arg("status").output() {
                if o.status.success() {
                    ufw_status_line = String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .map(|s| s.trim().to_string());
                }
            }
        }
        let mut firewalld_default_zone = None;
        if fw == "firewalld" {
            if let Ok(o) = Command::new(find_bin("firewall-cmd"))
                .arg("--get-default-zone")
                .output()
            {
                if o.status.success() {
                    firewalld_default_zone =
                        Some(String::from_utf8_lossy(&o.stdout).trim().to_string());
                }
            }
        }
        let selinux_mode = Command::new(find_bin("getenforce"))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(HostSecuritySummary {
            network_backend: net,
            firewall_backend: fw,
            ufw_status_line,
            firewalld_default_zone,
            selinux_mode,
        })
    }
}

// ── Package install / upgrade (mutating; Linux only) ───────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageActionResult {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
}

/// Single package name / version pin token (apt `pkg=ver`, dnf names, etc.).
pub fn validate_package_token(name: &str) -> Result<(), LibvirtError> {
    let s = name.trim();
    if s.is_empty() {
        return Err(LibvirtError::Invalid("package name is empty".into()));
    }
    if s.len() > 200 {
        return Err(LibvirtError::Invalid("package name is too long".into()));
    }
    // Bar a leading '-' so a token can't become a package-manager flag when passed
    // as a positional argv element (e.g. apt-get install --allow-downgrades).
    if s.starts_with('-') {
        return Err(LibvirtError::Invalid(
            "package name must not start with '-'".into(),
        ));
    }
    let ok = s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '+' | '.' | '-' | '_' | ':' | '=' | '*' | '~' | '@' | '%' | '[' | ']'
            )
    });
    if !ok {
        return Err(LibvirtError::Invalid(
            "package name contains unsupported characters".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
const PACKAGE_ACTION_BUDGET: Duration = Duration::from_secs(3600);

#[cfg(target_os = "linux")]
const MAX_PACKAGES_PER_REQUEST: usize = 32;

#[cfg(target_os = "linux")]
fn merge_action_result(
    command: &str,
    prior: PackageActionResult,
    mut cmd: Command,
) -> Result<PackageActionResult, LibvirtError> {
    let out = run_with_budget(&mut cmd, PACKAGE_ACTION_BUDGET)?;
    let ok = prior.ok && out.status.success();
    let stdout = format!("{}\n{}", prior.stdout, String::from_utf8_lossy(&out.stdout));
    let stderr = format!("{}\n{}", prior.stderr, String::from_utf8_lossy(&out.stderr));
    let exit_code = if out.status.success() {
        prior.exit_code
    } else {
        out.status.code().unwrap_or(-1)
    };
    Ok(PackageActionResult {
        command: format!("{} ; {}", prior.command, command),
        exit_code,
        stdout,
        stderr,
        ok,
    })
}

#[cfg(target_os = "linux")]
fn run_one_action(
    mut cmd: Command,
    command_display: String,
) -> Result<PackageActionResult, LibvirtError> {
    let out = run_with_budget(&mut cmd, PACKAGE_ACTION_BUDGET)?;
    let ok = out.status.success();
    Ok(PackageActionResult {
        command: command_display,
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        ok,
    })
}

/// Apply pending upgrades using the same backend ordering as [`detect_package_backend`].
pub fn package_upgrade() -> Result<PackageActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        Err(LibvirtError::Operation(
            "package upgrade is only supported on Linux".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        package_upgrade_linux()
    }
}

#[cfg(target_os = "linux")]
fn package_upgrade_linux() -> Result<PackageActionResult, LibvirtError> {
    let backend = detect_package_backend();
    match backend {
        "apt" => {
            let apt = find_bin("apt-get");
            let r1 = run_one_action(
                {
                    let mut c = Command::new(&apt);
                    c.arg("-qq")
                        .arg("update")
                        .env("DEBIAN_FRONTEND", "noninteractive");
                    c
                },
                format!("{apt} -qq update"),
            )?;
            if !r1.ok {
                return Ok(r1);
            }
            merge_action_result(&format!("{apt} -y -qq upgrade"), r1, {
                let mut c = Command::new(&apt);
                c.args(["-y", "-qq", "upgrade"])
                    .env("DEBIAN_FRONTEND", "noninteractive");
                c
            })
        }
        "microdnf" => {
            let exe = find_bin("microdnf");
            run_one_action(
                {
                    let mut c = Command::new(&exe);
                    c.args(["-y", "upgrade"]);
                    c
                },
                format!("{exe} -y upgrade"),
            )
        }
        "dnf" => {
            let exe = find_bin("dnf");
            run_one_action(
                {
                    let mut c = Command::new(&exe);
                    c.args(["-y", "upgrade"]);
                    c
                },
                format!("{exe} -y upgrade"),
            )
        }
        "yum" => {
            let exe = find_bin("yum");
            run_one_action(
                {
                    let mut c = Command::new(&exe);
                    c.args(["-y", "upgrade"]);
                    c
                },
                format!("{exe} -y upgrade"),
            )
        }
        "apk" => {
            let apk = find_bin("apk");
            let r1 = run_one_action(
                {
                    let mut c = Command::new(&apk);
                    c.args(["update", "--no-cache"]);
                    c
                },
                format!("{apk} update --no-cache"),
            )?;
            if !r1.ok {
                return Ok(r1);
            }
            merge_action_result(&format!("{apk} upgrade -U"), r1, {
                let mut c = Command::new(&apk);
                c.args(["upgrade", "-U"]);
                c
            })
        }
        "pacman" => {
            let pm = find_bin("pacman");
            run_one_action(
                {
                    let mut c = Command::new(&pm);
                    c.args(["-Syu", "--noconfirm"]);
                    c
                },
                format!("{pm} -Syu --noconfirm"),
            )
        }
        "zypper" => {
            let zp = find_bin("zypper");
            let r1 = run_one_action(
                {
                    let mut c = Command::new(&zp);
                    c.args(["-n", "ref"]);
                    c
                },
                format!("{zp} -n ref"),
            )?;
            if !r1.ok {
                return Ok(r1);
            }
            merge_action_result(&format!("{zp} -n up -y"), r1, {
                let mut c = Command::new(&zp);
                c.args(["-n", "up", "-y"]);
                c
            })
        }
        _ => Err(LibvirtError::Invalid(format!(
            "unsupported package backend for upgrade: {backend}"
        ))),
    }
}

/// Install one or more packages (same backend as [`detect_package_backend`]; max {MAX_PACKAGES_PER_REQUEST}).
pub fn package_install(packages: Vec<String>) -> Result<PackageActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = packages;
        Err(LibvirtError::Operation(
            "package install is only supported on Linux".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        package_install_linux(packages)
    }
}

#[cfg(target_os = "linux")]
fn package_install_linux(packages: Vec<String>) -> Result<PackageActionResult, LibvirtError> {
    if packages.is_empty() {
        return Err(LibvirtError::Invalid("no packages specified".into()));
    }
    if packages.len() > MAX_PACKAGES_PER_REQUEST {
        return Err(LibvirtError::Invalid(format!(
            "too many packages (max {MAX_PACKAGES_PER_REQUEST})"
        )));
    }
    for p in &packages {
        validate_package_token(p)?;
    }
    let backend = detect_package_backend();
    let pkgs: Vec<&str> = packages.iter().map(|s| s.trim()).collect();
    match backend {
        "apt" => {
            let apt = find_bin("apt-get");
            let r1 = run_one_action(
                {
                    let mut c = Command::new(&apt);
                    c.arg("-qq")
                        .arg("update")
                        .env("DEBIAN_FRONTEND", "noninteractive");
                    c
                },
                format!("{apt} -qq update"),
            )?;
            if !r1.ok {
                return Ok(r1);
            }
            let mut c = Command::new(&apt);
            c.arg("-y")
                .arg("-qq")
                .arg("install")
                .env("DEBIAN_FRONTEND", "noninteractive");
            for p in &pkgs {
                c.arg(p);
            }
            merge_action_result(&format!("{apt} -y -qq install {}", pkgs.join(" ")), r1, c)
        }
        "microdnf" => {
            let exe = find_bin("microdnf");
            let mut c = Command::new(&exe);
            c.arg("-y").arg("install");
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y install {}", pkgs.join(" ")))
        }
        "dnf" => {
            let exe = find_bin("dnf");
            let mut c = Command::new(&exe);
            c.args(["-y", "install"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y install {}", pkgs.join(" ")))
        }
        "yum" => {
            let exe = find_bin("yum");
            let mut c = Command::new(&exe);
            c.args(["-y", "install"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y install {}", pkgs.join(" ")))
        }
        "apk" => {
            let apk = find_bin("apk");
            let mut c = Command::new(&apk);
            c.args(["add", "--no-cache"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{apk} add --no-cache {}", pkgs.join(" ")))
        }
        "pacman" => {
            let pm = find_bin("pacman");
            let mut c = Command::new(&pm);
            c.args(["-S", "--noconfirm"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{pm} -S --noconfirm {}", pkgs.join(" ")))
        }
        "zypper" => {
            let zp = find_bin("zypper");
            let r1 = run_one_action(
                {
                    let mut c = Command::new(&zp);
                    c.args(["-n", "ref"]);
                    c
                },
                format!("{zp} -n ref"),
            )?;
            if !r1.ok {
                return Ok(r1);
            }
            let mut c = Command::new(&zp);
            c.args(["-n", "in", "-y"]);
            for p in &pkgs {
                c.arg(p);
            }
            merge_action_result(&format!("{zp} -n in -y {}", pkgs.join(" ")), r1, c)
        }
        _ => Err(LibvirtError::Invalid(format!(
            "unsupported package backend for install: {backend}"
        ))),
    }
}

/// Remove (uninstall) one or more packages (same backend as [`detect_package_backend`]; max 32 per request).
/// When `purge` is true, Debian/Ubuntu uses `apt-get remove --purge`; other backends ignore it.
pub fn package_remove(
    packages: Vec<String>,
    purge: bool,
) -> Result<PackageActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (packages, purge);
        Err(LibvirtError::Operation(
            "package remove is only supported on Linux".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        package_remove_linux(packages, purge)
    }
}

#[cfg(target_os = "linux")]
fn package_remove_linux(
    packages: Vec<String>,
    purge: bool,
) -> Result<PackageActionResult, LibvirtError> {
    if packages.is_empty() {
        return Err(LibvirtError::Invalid("no packages specified".into()));
    }
    if packages.len() > MAX_PACKAGES_PER_REQUEST {
        return Err(LibvirtError::Invalid(format!(
            "too many packages (max {MAX_PACKAGES_PER_REQUEST})"
        )));
    }
    for p in &packages {
        validate_package_token(p)?;
    }
    let backend = detect_package_backend();
    let pkgs: Vec<&str> = packages.iter().map(|s| s.trim()).collect();
    match backend {
        "apt" => {
            let apt = find_bin("apt-get");
            let mut c = Command::new(&apt);
            if purge {
                c.args(["-y", "-qq", "remove", "--purge"])
                    .env("DEBIAN_FRONTEND", "noninteractive");
            } else {
                c.args(["-y", "-qq", "remove"])
                    .env("DEBIAN_FRONTEND", "noninteractive");
            }
            for p in &pkgs {
                c.arg(p);
            }
            let label = if purge {
                format!("{apt} -y -qq remove --purge {}", pkgs.join(" "))
            } else {
                format!("{apt} -y -qq remove {}", pkgs.join(" "))
            };
            run_one_action(c, label)
        }
        "microdnf" => {
            let exe = find_bin("microdnf");
            let mut c = Command::new(&exe);
            c.args(["-y", "remove"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y remove {}", pkgs.join(" ")))
        }
        "dnf" => {
            let exe = find_bin("dnf");
            let mut c = Command::new(&exe);
            c.args(["-y", "remove"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y remove {}", pkgs.join(" ")))
        }
        "yum" => {
            let exe = find_bin("yum");
            let mut c = Command::new(&exe);
            c.args(["-y", "remove"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{exe} -y remove {}", pkgs.join(" ")))
        }
        "apk" => {
            let apk = find_bin("apk");
            let mut c = Command::new(&apk);
            c.arg("del");
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{apk} del {}", pkgs.join(" ")))
        }
        "pacman" => {
            let pm = find_bin("pacman");
            let mut c = Command::new(&pm);
            c.args(["-R", "--noconfirm"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{pm} -R --noconfirm {}", pkgs.join(" ")))
        }
        "zypper" => {
            let zp = find_bin("zypper");
            let mut c = Command::new(&zp);
            c.args(["-n", "rm", "-y"]);
            for p in &pkgs {
                c.arg(p);
            }
            run_one_action(c, format!("{zp} -n rm -y {}", pkgs.join(" ")))
        }
        _ => Err(LibvirtError::Invalid(format!(
            "unsupported package backend for remove: {backend}"
        ))),
    }
}

/// Simulate a full upgrade (no changes) and return combined tool output for operator review.
pub fn package_upgrade_preview() -> Result<PackageActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        Err(LibvirtError::Operation(
            "package upgrade preview is only supported on Linux".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        package_upgrade_preview_linux()
    }
}

#[cfg(target_os = "linux")]
fn package_upgrade_preview_linux() -> Result<PackageActionResult, LibvirtError> {
    let backend = detect_package_backend();
    match backend {
        "apt" => {
            let ag = find_bin("apt-get");
            let mut cmd = Command::new(find_bin("sh"));
            cmd.arg("-c").arg(format!(
                "set -o pipefail; DEBIAN_FRONTEND=noninteractive {ag} -qq -s upgrade 2>&1 | tail -n 4000"
            ));
            run_one_action(cmd, format!("{ag} -qq -s upgrade (simulate, tail)"))
        }
        "microdnf" | "dnf" => {
            let exe = find_bin(if backend == "microdnf" {
                "microdnf"
            } else {
                "dnf"
            });
            let mut cmd = Command::new(&exe);
            cmd.args(["upgrade", "--assumeno"]);
            run_one_action(cmd, format!("{exe} upgrade --assumeno"))
        }
        "yum" => {
            let exe = find_bin("yum");
            let mut cmd = Command::new(&exe);
            cmd.args(["upgrade", "--assumeno"]);
            run_one_action(cmd, format!("{exe} upgrade --assumeno"))
        }
        "apk" => {
            let apk = find_bin("apk");
            let mut cmd = Command::new(&apk);
            cmd.args(["upgrade", "--simulate"]);
            run_one_action(cmd, format!("{apk} upgrade --simulate"))
        }
        "pacman" => {
            let pm = find_bin("pacman");
            let mut cmd = Command::new(&pm);
            cmd.args(["-Sup", "--noconfirm"]);
            run_one_action(cmd, format!("{pm} -Sup --noconfirm"))
        }
        "zypper" => {
            let zp = find_bin("zypper");
            let mut cmd = Command::new(&zp);
            cmd.args(["-n", "up", "--dry-run", "-y"]);
            run_one_action(cmd, format!("{zp} -n up --dry-run -y"))
        }
        _ => Err(LibvirtError::Invalid(format!(
            "unsupported package backend for upgrade preview: {backend}"
        ))),
    }
}

/// Remove packages that were installed only as dependencies and are no longer needed (apt: `autoremove`).
pub fn package_autoremove() -> Result<PackageActionResult, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        Err(LibvirtError::Operation(
            "package autoremove is only supported on Linux".into(),
        ))
    }
    #[cfg(target_os = "linux")]
    {
        package_autoremove_linux()
    }
}

#[cfg(target_os = "linux")]
fn package_autoremove_linux() -> Result<PackageActionResult, LibvirtError> {
    if detect_package_backend() != "apt" {
        return Err(LibvirtError::Invalid(
            "autoremove is only implemented for apt".into(),
        ));
    }
    let apt = find_bin("apt-get");
    let mut c = Command::new(&apt);
    c.args(["-y", "-qq", "autoremove"])
        .env("DEBIAN_FRONTEND", "noninteractive");
    run_one_action(c, format!("{apt} -y -qq autoremove"))
}
