// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Linux host observability: PSI, block I/O (`/proc/diskstats`), optional SMART.

use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

use crate::bpf_probe::BpfProbeSummary;
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PressureAvg {
    /// Some / full / total (microseconds of stall over a window).
    pub some: f64,
    pub full: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostPressureStats {
    pub cpu: PressureAvg,
    pub memory: PressureAvg,
    pub io: PressureAvg,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskIoStat {
    pub device: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ios: u64,
    pub write_ios: u64,
    pub read_ticks: u64,
    pub write_ticks: u64,
    pub io_in_progress: u64,
    pub time_in_queue: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartDiskHealth {
    pub device: String,
    pub passed: bool,
    pub summary: String,
    pub probed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CgroupV2Stats {
    pub unified_path: String,
    pub memory_current_bytes: Option<u64>,
    /// `max` file value; `"max"` in sysfs becomes None here.
    pub memory_max_bytes: Option<u64>,
    pub cpu_usage_usec: Option<u64>,
    pub available: bool,
}

/// QEMU/libvirt guest cgroup (from `machine.slice/machine-qemu`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmCgroupStats {
    pub vm_name: String,
    pub cgroup_path: String,
    pub memory_current_bytes: Option<u64>,
    pub memory_max_bytes: Option<u64>,
    pub cpu_usage_usec: Option<u64>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwmonTemp {
    pub sensor: String,
    pub label: String,
    pub temp_celsius: f64,
    pub critical_celsius: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxHostObservability {
    pub pressure: HostPressureStats,
    pub disk_io: Vec<DiskIoStat>,
    pub smart: Vec<SmartDiskHealth>,
    pub cgroup: CgroupV2Stats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thermal: Vec<HwmonTemp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vm_cgroups: Vec<VmCgroupStats>,
    pub bpf: BpfProbeSummary,
}

#[cfg(target_os = "linux")]
fn io_err(msg: impl Into<String>) -> LibvirtError {
    LibvirtError::Operation(msg.into())
}

/// Parse the two-line `/proc/pressure/*` (PSI) format into a PressureAvg.
///
///   some avg10=X avg60=Y avg300=Z total=N
///   full avg10=X avg60=Y avg300=Z total=N
///
/// The old parser looked for `some=`/`full=` tokens that don't exist (the token
/// is a bare `some`, then `avgN=`/`total=`) and only read the first line, so
/// some/full were always 0 — the host never showed any pressure. Report avg10
/// (most-recent 10s window, a %) for some/full and the some line's total counter.
fn parse_pressure(content: &str) -> PressureAvg {
    let mut out = PressureAvg::default();
    for line in content.lines() {
        let mut tokens = line.split_whitespace();
        let kind = tokens.next().unwrap_or("");
        let mut avg10 = 0.0f64;
        let mut total = 0.0f64;
        for t in tokens {
            if let Some(v) = t.strip_prefix("avg10=") {
                avg10 = v.parse().unwrap_or(0.0);
            } else if let Some(v) = t.strip_prefix("total=") {
                total = v.parse().unwrap_or(0.0);
            }
        }
        match kind {
            "some" => {
                out.some = avg10;
                out.total = total;
            }
            "full" => out.full = avg10,
            _ => {}
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn read_pressure(path: &str) -> PressureAvg {
    fs::read_to_string(path)
        .map(|c| parse_pressure(&c))
        .unwrap_or_default()
}

#[cfg(test)]
mod pressure_tests {
    use super::parse_pressure;

    #[test]
    fn parses_avg10_from_both_lines() {
        let sample = "some avg10=0.49 avg60=0.61 avg300=0.67 total=22668891778\n\
                      full avg10=0.10 avg60=0.05 avg300=0.02 total=42359297\n";
        let p = parse_pressure(sample);
        assert!((p.some - 0.49).abs() < 1e-9, "some avg10 = {}", p.some);
        assert!((p.full - 0.10).abs() < 1e-9, "full avg10 = {}", p.full);
        assert!(
            (p.total - 22668891778.0).abs() < 1.0,
            "some total = {}",
            p.total
        );
    }

    #[test]
    fn zero_pressure_is_zero_not_garbage() {
        let sample = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
                      full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        let p = parse_pressure(sample);
        assert_eq!(p.some, 0.0);
        assert_eq!(p.full, 0.0);
        assert_eq!(p.total, 0.0);
    }

    #[test]
    fn empty_or_malformed_input_defaults_to_zero() {
        let p = parse_pressure("");
        assert_eq!(p.some, 0.0);
        assert_eq!(p.full, 0.0);
    }
}

pub fn read_host_pressure() -> HostPressureStats {
    #[cfg(not(target_os = "linux"))]
    {
        return HostPressureStats::default();
    }
    #[cfg(target_os = "linux")]
    {
        let cpu = read_pressure("/proc/pressure/cpu");
        let memory = read_pressure("/proc/pressure/memory");
        let io = read_pressure("/proc/pressure/io");
        let available = Path::new("/proc/pressure/cpu").exists();
        HostPressureStats {
            cpu,
            memory,
            io,
            available,
        }
    }
}

pub fn read_diskstats() -> Result<Vec<DiskIoStat>, LibvirtError> {
    #[cfg(not(target_os = "linux"))]
    {
        return Ok(Vec::new());
    }
    #[cfg(target_os = "linux")]
    {
        let raw = fs::read_to_string("/proc/diskstats")
            .map_err(|e| io_err(format!("read /proc/diskstats: {e}")))?;
        let mut out = Vec::new();
        for line in raw.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            // https://www.kernel.org/doc/Documentation/ABI/testing/procfs-diskstats
            if cols.len() < 14 {
                continue;
            }
            let dev = format!("{}{}", cols[0], cols[1]);
            if cols[2].starts_with("loop") || cols[2].starts_with("ram") {
                continue;
            }
            let device = cols[2].to_string();
            let read_ios: u64 = cols[3].parse().unwrap_or(0);
            let read_merged: u64 = cols[4].parse().unwrap_or(0);
            let read_sectors: u64 = cols[5].parse().unwrap_or(0);
            let read_ticks: u64 = cols[6].parse().unwrap_or(0);
            let write_ios: u64 = cols[7].parse().unwrap_or(0);
            let write_merged: u64 = cols[8].parse().unwrap_or(0);
            let write_sectors: u64 = cols[9].parse().unwrap_or(0);
            let write_ticks: u64 = cols[10].parse().unwrap_or(0);
            let io_in_progress: u64 = cols[11].parse().unwrap_or(0);
            let time_in_queue: u64 = cols[12].parse().unwrap_or(0);
            let _ = (dev, read_merged, write_merged);
            out.push(DiskIoStat {
                device,
                read_bytes: read_sectors.saturating_mul(512),
                write_bytes: write_sectors.saturating_mul(512),
                read_ios,
                write_ios,
                read_ticks,
                write_ticks,
                io_in_progress,
                time_in_queue,
            });
        }
        out.sort_by(|a, b| a.device.cmp(&b.device));
        Ok(out)
    }
}

#[cfg(target_os = "linux")]
fn find_smartctl() -> Option<String> {
    for c in ["/usr/sbin/smartctl", "/sbin/smartctl", "/usr/bin/smartctl"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

pub fn probe_smart_health(devices: &[String]) -> Vec<SmartDiskHealth> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = devices;
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        let Some(smartctl) = find_smartctl() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dev in devices {
            let path = if dev.starts_with('/') {
                dev.clone()
            } else {
                format!("/dev/{dev}")
            };
            let output = Command::new(&smartctl)
                .args(["-H", "-n", "standby", &path])
                .output();
            let (passed, summary, probed) = match output {
                Ok(o) => {
                    let text = format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    let lower = text.to_ascii_lowercase();
                    let passed =
                        lower.contains("passed") || lower.contains("ok") || o.status.success();
                    let summary: String = text
                        .lines()
                        .find(|l| {
                            l.contains("PASSED")
                                || l.contains("FAILED")
                                || l.contains("SMART")
                                || l.contains("Health")
                        })
                        .unwrap_or(text.lines().next().unwrap_or(""))
                        .trim()
                        .chars()
                        .take(200)
                        .collect();
                    (passed, summary.to_string(), true)
                }
                Err(e) => (false, format!("smartctl failed: {e}"), false),
            };
            out.push(SmartDiskHealth {
                device: dev.clone(),
                passed,
                summary,
                probed,
            });
        }
        out
    }
}

#[cfg(target_os = "linux")]
fn read_cgroup_v2_at(base: &str, unified_path: &str) -> CgroupV2Stats {
    let read_u64_file = |rel: &str| -> Option<u64> {
        let p = format!("{base}/{rel}");
        let s = fs::read_to_string(&p).ok()?;
        let t = s.trim();
        if t == "max" {
            return None;
        }
        t.parse().ok()
    };
    let memory_current_bytes = read_u64_file("memory.current");
    let memory_max_bytes = read_u64_file("memory.max");
    let cpu_usage_usec = fs::read_to_string(format!("{base}/cpu.stat"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                let mut parts = l.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some("usage_usec"), Some(v)) => v.parse().ok(),
                    _ => None,
                }
            })
        });
    CgroupV2Stats {
        unified_path: unified_path.to_string(),
        memory_current_bytes,
        memory_max_bytes,
        cpu_usage_usec,
        available: true,
    }
}

#[cfg(target_os = "linux")]
fn read_cgroup_v2_self_linux() -> CgroupV2Stats {
    let mut unified_path = String::new();
    if let Ok(cg) = fs::read_to_string("/proc/self/cgroup") {
        for line in cg.lines() {
            let Some((rest, path)) = line.split_once("::") else {
                continue;
            };
            if rest == "0" && !path.is_empty() {
                unified_path = path.trim_start_matches('/').to_string();
                break;
            }
        }
    }
    if unified_path.is_empty() {
        return CgroupV2Stats::default();
    }
    let base = format!("/sys/fs/cgroup/{unified_path}");
    read_cgroup_v2_at(&base, &unified_path)
}

pub fn vm_name_from_libvirt_cgroup_dir(dir_name: &str) -> Option<String> {
    let stem = dir_name.strip_suffix(".libvirt-qemu")?;
    let (id_part, name) = stem.split_once('-')?;
    if id_part.chars().all(|c| c.is_ascii_digit()) && !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn libvirt_qemu_cgroup_roots() -> Vec<&'static str> {
    vec![
        "/sys/fs/cgroup/machine.slice/machine-qemu",
        "/sys/fs/cgroup/system.slice/machine-qemu",
    ]
}

#[cfg(target_os = "linux")]
pub fn list_vm_cgroup_stats() -> Vec<VmCgroupStats> {
    let mut out = Vec::new();
    for root in libvirt_qemu_cgroup_roots() {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let Some(vm_name) = vm_name_from_libvirt_cgroup_dir(&dir_name) else {
                continue;
            };
            let cgroup_path = format!(
                "{}/{}",
                root.trim_start_matches("/sys/fs/cgroup/"),
                dir_name
            );
            let base = entry.path();
            let stats = read_cgroup_v2_at(base.to_str().unwrap_or(""), &cgroup_path);
            out.push(VmCgroupStats {
                vm_name,
                cgroup_path,
                memory_current_bytes: stats.memory_current_bytes,
                memory_max_bytes: stats.memory_max_bytes,
                cpu_usage_usec: stats.cpu_usage_usec,
                available: stats.available,
            });
        }
    }
    out.sort_by(|a, b| a.vm_name.cmp(&b.vm_name));
    out.dedup_by(|a, b| a.vm_name == b.vm_name);
    out
}

#[cfg(not(target_os = "linux"))]
pub fn list_vm_cgroup_stats() -> Vec<VmCgroupStats> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn read_vm_cgroup_v2(vm_name: &str) -> VmCgroupStats {
    let suffix = format!("-{vm_name}.libvirt-qemu");
    for root in libvirt_qemu_cgroup_roots() {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir_name = entry.file_name().to_string_lossy().to_string();
            if !dir_name.ends_with(&suffix) {
                continue;
            }
            let cgroup_path = format!(
                "{}/{}",
                root.trim_start_matches("/sys/fs/cgroup/"),
                dir_name
            );
            let base = entry.path();
            let stats = read_cgroup_v2_at(base.to_str().unwrap_or(""), &cgroup_path);
            return VmCgroupStats {
                vm_name: vm_name.to_string(),
                cgroup_path,
                memory_current_bytes: stats.memory_current_bytes,
                memory_max_bytes: stats.memory_max_bytes,
                cpu_usage_usec: stats.cpu_usage_usec,
                available: stats.available,
            };
        }
    }
    VmCgroupStats {
        vm_name: vm_name.to_string(),
        ..Default::default()
    }
}

#[cfg(not(target_os = "linux"))]
pub fn read_vm_cgroup_v2(vm_name: &str) -> VmCgroupStats {
    VmCgroupStats {
        vm_name: vm_name.to_string(),
        ..Default::default()
    }
}

#[cfg(target_os = "linux")]
pub fn read_hwmon_temps() -> Vec<HwmonTemp> {
    let hwmon_root = Path::new("/sys/class/hwmon");
    let Ok(chips) = fs::read_dir(hwmon_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for chip in chips.flatten() {
        let chip_path = chip.path();
        let chip_id = chip.file_name().to_string_lossy().to_string();
        let chip_name = fs::read_to_string(chip_path.join("name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| chip_id.clone());
        let Ok(entries) = fs::read_dir(&chip_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            let Some(idx) = fname
                .strip_prefix("temp")
                .and_then(|r| r.strip_suffix("_input"))
            else {
                continue;
            };
            let raw: i64 = fs::read_to_string(entry.path())
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            if raw == 0 {
                continue;
            }
            let label_path = chip_path.join(format!("temp{idx}_label"));
            let label = fs::read_to_string(&label_path)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| format!("temp{idx}"));
            let crit = fs::read_to_string(chip_path.join(format!("temp{idx}_crit")))
                .ok()
                .and_then(|s| s.trim().parse::<i64>().ok())
                .map(|milli| milli as f64 / 1000.0);
            let sensor = format!("{chip_name}/{label}");
            out.push(HwmonTemp {
                sensor,
                label,
                temp_celsius: raw as f64 / 1000.0,
                critical_celsius: crit,
            });
        }
    }
    out.sort_by(|a, b| a.sensor.cmp(&b.sensor));
    out
}

#[cfg(not(target_os = "linux"))]
pub fn read_hwmon_temps() -> Vec<HwmonTemp> {
    Vec::new()
}

pub fn read_cgroup_v2_self() -> CgroupV2Stats {
    #[cfg(target_os = "linux")]
    {
        read_cgroup_v2_self_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        CgroupV2Stats::default()
    }
}

pub fn gather_linux_observability() -> Result<LinuxHostObservability, LibvirtError> {
    let pressure = read_host_pressure();
    let disk_io = read_diskstats()?;
    let cgroup = read_cgroup_v2_self();
    let thermal = read_hwmon_temps();
    let vm_cgroups = list_vm_cgroup_stats();
    let bpf = crate::bpf_probe::probe_bpf_summary();
    let block_devs: Vec<String> = disk_io
        .iter()
        .filter(|d| {
            !d.device.starts_with("dm")
                && !d.device.starts_with("md")
                && !d.device.starts_with("nbd")
                && !d.device.starts_with("loop")
                && !d.device.starts_with("sr")
                && !d.device.starts_with("ram")
                && !d.device.starts_with("zram")
                && d.read_bytes + d.write_bytes > 0
        })
        .take(8)
        .map(|d| d.device.clone())
        .collect();
    let smart = probe_smart_health(&block_devs);
    Ok(LinuxHostObservability {
        pressure,
        disk_io,
        smart,
        cgroup,
        thermal,
        vm_cgroups,
        bpf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_name_from_cgroup_dir_parses_libvirt_suffix() {
        assert_eq!(
            vm_name_from_libvirt_cgroup_dir("42-my-vm.libvirt-qemu").as_deref(),
            Some("my-vm")
        );
        assert!(vm_name_from_libvirt_cgroup_dir("bad").is_none());
    }
}
