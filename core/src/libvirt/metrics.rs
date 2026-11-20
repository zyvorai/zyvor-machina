// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::domain::lookup_domain;
use crate::host_linux_obs;
use crate::state::{VmBlockDeviceMetrics, VmMetrics, VmNetDeviceMetrics, VmVcpuMetrics};
use crate::LibvirtError;
use tracing::warn;
use virt::connect::Connect;
use virt::domain::Domain;

// libvirt virDomainMemoryStatTags — these were numerically wrong (AVAILABLE=6,
// ACTUAL_BALLOON=8, RSS=9), so the collector read USABLE/LAST_UPDATE and produced
// garbage per-VM memory_used/memory_pct. Real enum values:
const VIR_DOMAIN_MEMORY_STAT_UNUSED: u32 = 4;
const VIR_DOMAIN_MEMORY_STAT_AVAILABLE: u32 = 5;
const VIR_DOMAIN_MEMORY_STAT_ACTUAL_BALLOON: u32 = 6;
const VIR_DOMAIN_MEMORY_STAT_RSS: u32 = 7;

/// Compute (total_mb, used_mb, pct) from libvirt memory-stat values (all KiB).
/// used = available − unused (guest usable RAM minus free), falling back to RSS
/// then 0; total = balloon (allocation), falling back to the info memory.
fn compute_memory_mb(
    actual_balloon_kb: u64,
    available_kb: u64,
    unused_kb: u64,
    rss_kb: u64,
    info_memory_kb: u64,
) -> (u64, u64, f64) {
    let total_mb = if actual_balloon_kb > 0 {
        actual_balloon_kb / 1024
    } else {
        info_memory_kb / 1024
    };
    let used_mb = if available_kb > 0 && unused_kb > 0 {
        available_kb.saturating_sub(unused_kb) / 1024
    } else if rss_kb > 0 {
        rss_kb / 1024
    } else {
        0
    };
    // RSS includes QEMU process overhead (device emulation, page tables) beyond the guest's
    // balloon size, so the RSS fallback can report more "used" than the VM's total memory.
    // Clamp to total so used_mb and pct never contradict each other in the UI.
    let used_mb = if total_mb > 0 {
        used_mb.min(total_mb)
    } else {
        used_mb
    };
    let pct = if total_mb > 0 {
        (used_mb as f64 / total_mb as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    (total_mb, used_mb, pct)
}

#[cfg(test)]
mod memory_tests {
    use super::compute_memory_mb;

    #[test]
    fn used_is_available_minus_unused() {
        // 2 GiB balloon, guest sees 2 GiB usable, 512 MiB free → 1.5 GiB used.
        let (total, used, pct) =
            compute_memory_mb(2 * 1024 * 1024, 2 * 1024 * 1024, 512 * 1024, 0, 0);
        assert_eq!(total, 2048);
        assert_eq!(used, 1536);
        assert!((pct - 75.0).abs() < 0.1, "pct = {pct}");
    }

    #[test]
    fn falls_back_to_rss_then_info_memory() {
        // No available/unused → use RSS for used; no balloon → use info.memory.
        let (total, used, _pct) = compute_memory_mb(0, 0, 0, 800 * 1024, 4 * 1024 * 1024);
        assert_eq!(total, 4096);
        assert_eq!(used, 800);
    }

    #[test]
    fn rss_overshoot_clamps_used_to_total() {
        // 8 GiB balloon, but QEMU RSS (host-side, includes device emulation overhead)
        // reports 8260 MiB — used_mb must not exceed total_mb, and pct must match.
        let (total, used, pct) = compute_memory_mb(8192 * 1024, 0, 0, 8260 * 1024, 0);
        assert_eq!(total, 8192);
        assert_eq!(used, 8192);
        assert!((pct - 100.0).abs() < 0.1, "pct = {pct}");
    }

    #[test]
    fn pct_clamped_and_zero_safe() {
        assert_eq!(compute_memory_mb(0, 0, 0, 0, 0), (0, 0, 0.0));
        // used > total would clamp to 100%.
        let (_t, _u, pct) = compute_memory_mb(1024 * 1024, 2 * 1024 * 1024, 0, 0, 0);
        // available>0 but unused==0 → falls back (rss 0) → used 0 here.
        assert_eq!(pct, 0.0);
    }
}

pub fn domain_state_label(state: u32) -> &'static str {
    match state {
        0 => "nostate",
        1 => "running",
        2 => "blocked",
        3 => "paused",
        4 => "shutting down",
        5 => "shutoff",
        6 => "crashed",
        7 => "pmsuspended",
        _ => "unknown",
    }
}

pub fn get_vm_metrics(conn: &Connect, name: &str) -> Result<VmMetrics, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    collect_domain_metrics(&domain, name)
}

pub fn get_all_vm_metrics(conn: &Connect) -> Result<Vec<VmMetrics>, LibvirtError> {
    let domains = conn
        .list_all_domains(0)
        .map_err(LibvirtError::map_op("Failed to list domains"))?;

    let mut metrics = Vec::new();
    for domain in domains {
        let name = domain.get_name().unwrap_or_default();
        let info = match domain.get_info() {
            Ok(i) => i,
            Err(e) => {
                warn!("Failed to get info for VM '{}': {}", name, e);
                continue;
            }
        };

        let state = domain_state_label(info.state).to_string();
        let running = info.state == 1;

        if running {
            match collect_domain_metrics(&domain, &name) {
                Ok(m) => metrics.push(m),
                Err(e) => warn!("Failed to collect metrics for VM '{}': {}", name, e),
            }
        } else {
            metrics.push(stub_vm_metrics(&name, &state, info.nr_virt_cpu));
        }
    }

    Ok(metrics)
}

fn stub_vm_metrics(name: &str, state: &str, vcpus: u32) -> VmMetrics {
    VmMetrics {
        name: name.to_string(),
        state: state.to_string(),
        running: false,
        cpu_time_ns: 0,
        vcpus,
        memory_total_mb: 0,
        memory_used_mb: 0,
        memory_pct: 0.0,
        disk_rd_bytes: 0,
        disk_wr_bytes: 0,
        disk_rd_ops: 0,
        disk_wr_ops: 0,
        net_rx_bytes: 0,
        net_tx_bytes: 0,
        vcpus_detail: Vec::new(),
        disks: Vec::new(),
        nets: Vec::new(),
        cgroup: None,
        libvirt_connection: None,
    }
}

fn collect_domain_metrics(domain: &Domain, name: &str) -> Result<VmMetrics, LibvirtError> {
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get domain info"))?;

    let state = domain_state_label(info.state).to_string();
    let cpu_time_ns = info.cpu_time;

    // flags=0 — no special collection flags.
    let mem_stats = match domain.memory_stats(0) {
        Ok(stats) => stats,
        Err(e) => {
            warn!("Failed to get memory stats for VM '{}': {}", name, e);
            Vec::new()
        }
    };
    let mut actual_kb: u64 = 0;
    let mut available_kb: u64 = 0;
    let mut unused_kb: u64 = 0;
    let mut rss_kb: u64 = 0;

    for stat in &mem_stats {
        match stat.tag {
            // actual_kb = balloon (memory allocated to the guest) → total.
            // available_kb = memory the guest sees as usable; unused = free;
            // used = available − unused.
            VIR_DOMAIN_MEMORY_STAT_UNUSED => unused_kb = stat.val,
            VIR_DOMAIN_MEMORY_STAT_ACTUAL_BALLOON => actual_kb = stat.val,
            VIR_DOMAIN_MEMORY_STAT_AVAILABLE => available_kb = stat.val,
            VIR_DOMAIN_MEMORY_STAT_RSS => rss_kb = stat.val,
            _ => {}
        }
    }

    let (memory_total_mb, memory_used_mb, memory_pct) =
        compute_memory_mb(actual_kb, available_kb, unused_kb, rss_kb, info.memory);

    let (disks, disk_rd_bytes, disk_wr_bytes, disk_rd_ops, disk_wr_ops) =
        collect_block_stats(domain);
    let (nets, net_rx_bytes, net_tx_bytes) = collect_net_stats(domain);
    let vcpus_detail = collect_vcpu_stats(name);
    let cgroup = {
        let cg = host_linux_obs::read_vm_cgroup_v2(name);
        if cg.available {
            Some(cg)
        } else {
            None
        }
    };

    // Reliable host-side memory fallback. Guest balloon stats (available/unused) require the
    // virtio-balloon stats period to be enabled, and RSS isn't reported by every qemu — when
    // both are missing, memory_used reads 0 even for a busy VM, which silently breaks
    // threshold alerts, capacity/rightsizing, and the memory-based watchdog signal. The VM's
    // cgroup memory.current is always available on cgroup-v2 hosts, so fall back to it.
    let (memory_used_mb, memory_pct) = if memory_used_mb == 0 {
        let cg_used_mb = cgroup
            .as_ref()
            .and_then(|c| c.memory_current_bytes)
            .map(|b| b / 1024 / 1024)
            .unwrap_or(0);
        let pct = if memory_total_mb > 0 && cg_used_mb > 0 {
            (cg_used_mb as f64 / memory_total_mb as f64 * 100.0).min(100.0)
        } else {
            memory_pct
        };
        (cg_used_mb, pct)
    } else {
        (memory_used_mb, memory_pct)
    };

    Ok(VmMetrics {
        name: name.to_string(),
        state,
        running: true,
        cpu_time_ns,
        vcpus: info.nr_virt_cpu,
        memory_total_mb,
        memory_used_mb,
        memory_pct,
        disk_rd_bytes,
        disk_wr_bytes,
        disk_rd_ops,
        disk_wr_ops,
        net_rx_bytes,
        net_tx_bytes,
        vcpus_detail,
        disks,
        nets,
        cgroup,
        libvirt_connection: None,
    })
}

fn collect_block_stats(domain: &Domain) -> (Vec<VmBlockDeviceMetrics>, u64, u64, u64, u64) {
    let mut disks = Vec::new();
    let mut rd_total: u64 = 0;
    let mut wr_total: u64 = 0;
    let mut rd_ops: u64 = 0;
    let mut wr_ops: u64 = 0;

    if let Ok(xml) = domain.get_xml_desc(0) {
        for block in crate::xml::split_blocks(&xml, "disk") {
            if let Some(target) = crate::xml::extract_attr(&block, "target", "dev") {
                if let Ok(stats) = domain.get_block_stats(&target) {
                    let rd_bytes = stats.rd_bytes.max(0) as u64;
                    let wr_bytes = stats.wr_bytes.max(0) as u64;
                    let rd_o = stats.rd_req.max(0) as u64;
                    let wr_o = stats.wr_req.max(0) as u64;
                    rd_total += rd_bytes;
                    wr_total += wr_bytes;
                    rd_ops += rd_o;
                    wr_ops += wr_o;
                    disks.push(VmBlockDeviceMetrics {
                        device: target,
                        rd_bytes,
                        wr_bytes,
                        rd_ops: rd_o,
                        wr_ops: wr_o,
                    });
                }
            }
        }
    }

    disks.sort_by(|a, b| a.device.cmp(&b.device));
    (disks, rd_total, wr_total, rd_ops, wr_ops)
}

fn collect_net_stats(domain: &Domain) -> (Vec<VmNetDeviceMetrics>, u64, u64) {
    let mut nets = Vec::new();
    let mut rx_total: u64 = 0;
    let mut tx_total: u64 = 0;

    if let Ok(xml) = domain.get_xml_desc(0) {
        for block in crate::xml::split_blocks(&xml, "interface") {
            if let Some(target) = crate::xml::extract_attr(&block, "target", "dev") {
                if let Ok(stats) = domain.interface_stats(&target) {
                    let rx_bytes = stats.rx_bytes.max(0) as u64;
                    let tx_bytes = stats.tx_bytes.max(0) as u64;
                    let rx_packets = stats.rx_packets.max(0) as u64;
                    let tx_packets = stats.tx_packets.max(0) as u64;
                    rx_total += rx_bytes;
                    tx_total += tx_bytes;
                    nets.push(VmNetDeviceMetrics {
                        device: target,
                        rx_bytes,
                        tx_bytes,
                        rx_packets,
                        tx_packets,
                    });
                }
            }
        }
    }

    nets.sort_by(|a, b| a.device.cmp(&b.device));
    (nets, rx_total, tx_total)
}

pub fn collect_vcpu_stats(vm_name: &str) -> Vec<VmVcpuMetrics> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = vm_name;
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        collect_vcpu_stats_linux(vm_name)
    }
}

#[cfg(target_os = "linux")]
fn collect_vcpu_stats_linux(vm_name: &str) -> Vec<VmVcpuMetrics> {
    use std::process::Command;
    let Some(output) = Command::new("virsh")
        .args(["domstats", vm_name, "--vcpu"])
        .output()
        .ok()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut map: std::collections::BTreeMap<u32, VmVcpuMetrics> = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("vcpu.") {
            let Some((idx_s, keyval)) = rest.split_once('.') else {
                continue;
            };
            let Ok(vcpu) = idx_s.parse::<u32>() else {
                continue;
            };
            let entry = map.entry(vcpu).or_insert_with(|| VmVcpuMetrics {
                vcpu,
                cpu_time_ns: 0,
                state: String::new(),
            });
            if let Some((k, v)) = keyval.split_once('=') {
                match k {
                    "time" => entry.cpu_time_ns = v.parse().unwrap_or(0),
                    "state" => {
                        entry.state = match v {
                            "1" => "running",
                            "2" => "offline",
                            "3" => "idle",
                            "4" => "sleeping",
                            _ => v,
                        }
                        .to_string();
                    }
                    _ => {}
                }
            }
        }
    }
    map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::domain_state_label;

    #[test]
    fn domain_state_label_maps_running() {
        assert_eq!(domain_state_label(1), "running");
        assert_eq!(domain_state_label(5), "shutoff");
    }
}
