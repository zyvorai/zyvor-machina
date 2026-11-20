// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Compare active vs persistent (inactive) domain XML — Cockpit Machines needs-shutdown parity.

use serde::{Deserialize, Serialize};
use virt::connect::Connect;
use virt::sys::VIR_DOMAIN_XML_INACTIVE;

use super::domain::{lookup_domain, state_to_string};
use crate::state::{DiskInfo, FilesystemInfo, InterfaceInfo};
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConfig {
    pub needs_shutdown: bool,
    pub state: String,
    pub persistent: bool,
    pub pending_changes: Vec<PendingChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    pub category: String,
    pub summary: String,
}

pub fn get_pending_config(conn: &Connect, name: &str) -> Result<PendingConfig, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get domain info"))?;
    let state = state_to_string(info.state);
    let persistent = domain.is_persistent().unwrap_or(false);

    if state != "running" && state != "paused" {
        return Ok(PendingConfig {
            needs_shutdown: false,
            state,
            persistent,
            pending_changes: vec![],
        });
    }

    let active_xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get active XML"))?;
    let inactive_xml = domain
        .get_xml_desc(VIR_DOMAIN_XML_INACTIVE)
        .map_err(LibvirtError::map_op("Failed to get inactive XML"))?;

    let changes = diff_configs(&active_xml, &inactive_xml);
    // Do not fall back to a coarse full-XML string compare: live dumps always include
    // runtime-only noise (domain id, seclabel labels, vnet targets, channel paths, pty
    // sources) that is not a real needs-shutdown config change.

    Ok(PendingConfig {
        needs_shutdown: !changes.is_empty(),
        state,
        persistent,
        pending_changes: changes,
    })
}

fn diff_configs(active: &str, inactive: &str) -> Vec<PendingChange> {
    let mut out = Vec::new();

    let active_current = vcpu_current(active);
    let inactive_current = vcpu_current(inactive);
    if active_current != inactive_current {
        out.push(PendingChange {
            category: "cpu".into(),
            summary: format!(
                "vCPU count changed (running: {}, persistent: {})",
                active_current, inactive_current
            ),
        });
    } else {
        let active_max = vcpu_max(active);
        let inactive_max = vcpu_max(inactive);
        if active_max != inactive_max {
            out.push(PendingChange {
                category: "cpu".into(),
                summary: format!(
                    "Max vCPUs changed (running max: {}, persistent max: {}) — reboot to enable live hotplug",
                    active_max, inactive_max
                ),
            });
        }
    }

    if memory_kib(active) != memory_kib(inactive) {
        out.push(PendingChange {
            category: "memory".into(),
            summary: format!(
                "Memory size changed (running: {} MiB, persistent: {} MiB)",
                memory_kib(active) / 1024,
                memory_kib(inactive) / 1024
            ),
        });
    }

    diff_disks(active, inactive, &mut out);
    diff_interfaces(active, inactive, &mut out);
    diff_filesystems(active, inactive, &mut out);
    diff_boot_order(active, inactive, &mut out);
    diff_watchdog(active, inactive, &mut out);
    diff_hostdevs(active, inactive, &mut out);
    diff_vsock(active, inactive, &mut out);

    out
}

fn vcpu_block(xml: &str) -> Option<String> {
    crate::xml::split_blocks(xml, "vcpu").into_iter().next()
}

fn vcpu_current(xml: &str) -> String {
    let Some(b) = vcpu_block(xml) else {
        return String::new();
    };
    let max = crate::xml::extract_text(&b, "vcpu").unwrap_or_default();
    let current = crate::xml::extract_attr(&b, "vcpu", "current").unwrap_or_else(|| max.clone());
    let placement = crate::xml::extract_attr(&b, "vcpu", "placement").unwrap_or_default();
    if placement.is_empty() {
        current
    } else {
        format!("{current} ({placement})")
    }
}

fn vcpu_max(xml: &str) -> String {
    let Some(b) = vcpu_block(xml) else {
        return String::new();
    };
    let max = crate::xml::extract_text(&b, "vcpu").unwrap_or_default();
    if max.is_empty() {
        crate::xml::extract_attr(&b, "vcpu", "current").unwrap_or_default()
    } else {
        max
    }
}

fn memory_kib(xml: &str) -> u64 {
    crate::xml::extract_text(xml, "memory")
        .and_then(|s| s.parse().ok())
        .or_else(|| crate::xml::extract_attr(xml, "memory", "value").and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn disk_fingerprint(d: &DiskInfo) -> String {
    let source = if d.source == "unknown" {
        ""
    } else {
        d.source.as_str()
    };
    // Empty CD-ROMs often differ only by whether libvirt wrote `type='raw'` on
    // the live vs persistent driver element — not a real needs-shutdown change.
    let driver = if d.device == "cdrom" && source.is_empty() {
        "raw"
    } else if d.driver == "unknown" || d.driver.is_empty() {
        ""
    } else {
        d.driver.as_str()
    };
    format!(
        "{}:{}:{}:{}:{}:{}",
        d.target, d.device, source, driver, d.cache, d.readonly
    )
}

fn diff_disks(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let active_disks = super::domain::parse_disks_for_diff(active);
    let inactive_disks = super::domain::parse_disks_for_diff(inactive);
    let active_map: std::collections::BTreeMap<_, _> = active_disks
        .iter()
        .map(|d| (d.target.clone(), disk_fingerprint(d)))
        .collect();
    let inactive_map: std::collections::BTreeMap<_, _> = inactive_disks
        .iter()
        .map(|d| (d.target.clone(), disk_fingerprint(d)))
        .collect();

    for (target, fp) in &inactive_map {
        match active_map.get(target) {
            Some(active_fp) if active_fp != fp => out.push(PendingChange {
                category: "disk".into(),
                summary: format!(
                    "Disk {target} settings differ between running and persistent config"
                ),
            }),
            None => out.push(PendingChange {
                category: "disk".into(),
                summary: format!(
                    "Disk {target} added in persistent config (not active until shutdown)"
                ),
            }),
            _ => {}
        }
    }
    for target in active_map.keys() {
        if !inactive_map.contains_key(target) {
            out.push(PendingChange {
                category: "disk".into(),
                summary: format!(
                    "Disk {target} removed in persistent config (still active until shutdown)"
                ),
            });
        }
    }
}

fn iface_fingerprint(i: &InterfaceInfo) -> String {
    format!("{}:{}:{}", i.mac_address, i.source, i.model)
}

fn diff_interfaces(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let active_ifaces = super::domain::parse_interfaces_for_diff(active);
    let inactive_ifaces = super::domain::parse_interfaces_for_diff(inactive);
    let active_map: std::collections::BTreeMap<_, _> = active_ifaces
        .iter()
        .map(|i| (i.mac_address.clone(), iface_fingerprint(i)))
        .collect();
    let inactive_map: std::collections::BTreeMap<_, _> = inactive_ifaces
        .iter()
        .map(|i| (i.mac_address.clone(), iface_fingerprint(i)))
        .collect();

    for (mac, fp) in &inactive_map {
        match active_map.get(mac) {
            Some(active_fp) if active_fp != fp => out.push(PendingChange {
                category: "network".into(),
                summary: format!("NIC {mac} settings differ between running and persistent config"),
            }),
            None => out.push(PendingChange {
                category: "network".into(),
                summary: format!("NIC {mac} added in persistent config"),
            }),
            _ => {}
        }
    }
    for mac in active_map.keys() {
        if !inactive_map.contains_key(mac) {
            out.push(PendingChange {
                category: "network".into(),
                summary: format!("NIC {mac} removed in persistent config"),
            });
        }
    }
}

fn fs_fingerprint(f: &FilesystemInfo) -> String {
    format!("{}:{}", f.source, f.mount_tag)
}

fn diff_filesystems(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let active_fs = super::domain::parse_filesystems_for_diff(active);
    let inactive_fs = super::domain::parse_filesystems_for_diff(inactive);
    let active_set: std::collections::BTreeSet<_> = active_fs.iter().map(fs_fingerprint).collect();
    let inactive_set: std::collections::BTreeSet<_> =
        inactive_fs.iter().map(fs_fingerprint).collect();
    if active_set != inactive_set {
        out.push(PendingChange {
            category: "filesystem".into(),
            summary: "Virtiofs / shared directory configuration changed".into(),
        });
    }
}

fn boot_devices(xml: &str) -> Vec<String> {
    crate::xml::split_blocks(xml, "boot")
        .into_iter()
        .filter_map(|b| crate::xml::extract_attr(&b, "boot", "dev"))
        .collect()
}

fn diff_boot_order(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    if boot_devices(active) != boot_devices(inactive) {
        out.push(PendingChange {
            category: "boot".into(),
            summary: "Boot device order changed".into(),
        });
    }
}

fn watchdog_action(xml: &str) -> Option<String> {
    crate::xml::split_blocks(xml, "watchdog")
        .into_iter()
        .next()
        .and_then(|b| crate::xml::extract_attr(&b, "watchdog", "action"))
}

fn diff_watchdog(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let active_wd = active.contains("<watchdog");
    let inactive_wd = inactive.contains("<watchdog");
    if active_wd != inactive_wd {
        out.push(PendingChange {
            category: "watchdog".into(),
            summary: if inactive_wd && !active_wd {
                "Watchdog added in persistent config".into()
            } else {
                "Watchdog removed in persistent config".into()
            },
        });
        return;
    }
    if active_wd && watchdog_action(active) != watchdog_action(inactive) {
        out.push(PendingChange {
            category: "watchdog".into(),
            summary: "Watchdog action changed".into(),
        });
    }
}

fn hostdev_fingerprints(xml: &str) -> Vec<String> {
    crate::xml::split_blocks(xml, "hostdev")
        .into_iter()
        .map(|b| {
            let typ = crate::xml::extract_attr(&b, "hostdev", "type").unwrap_or_default();
            let addr = crate::xml::extract_attr(&b, "address", "domain")
                .or_else(|| crate::xml::extract_attr(&b, "source", "dev"))
                .unwrap_or_default();
            format!("{typ}:{addr}")
        })
        .collect()
}

fn diff_hostdevs(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let a = hostdev_fingerprints(active);
    let i = hostdev_fingerprints(inactive);
    if a != i {
        out.push(PendingChange {
            category: "hostdev".into(),
            summary: "Host device passthrough configuration changed".into(),
        });
    }
}

fn diff_vsock(active: &str, inactive: &str, out: &mut Vec<PendingChange>) {
    let active_v = active.contains("<vsock");
    let inactive_v = inactive.contains("<vsock");
    if active_v != inactive_v {
        out.push(PendingChange {
            category: "vsock".into(),
            summary: if inactive_v && !active_v {
                "Vsock device will be added after shutdown".into()
            } else {
                "Vsock device will be removed after shutdown".into()
            },
        });
    }
}

/// Strip runtime-only XML before coarse string comparison (kept for tests / future use).
#[allow(dead_code)]
fn normalize_for_compare(xml: &str) -> String {
    let mut out = String::new();
    let mut skip_depth = 0i32;
    for line in xml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<currentMemory") {
            skip_depth = 1;
            continue;
        }
        if skip_depth > 0 {
            if trimmed.contains("</currentMemory>") {
                skip_depth = 0;
            }
            continue;
        }
        let mut l = line.to_string();
        if l.contains("<vcpu") {
            l = l.replace(" current='", " _current='");
            l = l.replace(" current=\"", " _current=\"");
        }
        if l.contains("<graphics") && (l.contains("port='") || l.contains("port=\"")) {
            continue;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DiskInfo;

    fn disk(target: &str, device: &str, source: &str, driver: &str) -> DiskInfo {
        DiskInfo {
            device: device.into(),
            source: source.into(),
            driver: driver.into(),
            target: target.into(),
            bus: "sata".into(),
            cache: String::new(),
            readonly: true,
            shareable: false,
            capacity_bytes: None,
            allocation_bytes: None,
            physical_bytes: None,
        }
    }

    #[test]
    fn empty_cdrom_driver_raw_vs_unknown_not_a_diff() {
        let live = disk("sdb", "cdrom", "unknown", "unknown");
        let cfg = disk("sdb", "cdrom", "", "raw");
        assert_eq!(disk_fingerprint(&live), disk_fingerprint(&cfg));
    }
}
