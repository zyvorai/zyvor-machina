// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use serde::{Deserialize, Serialize};
use virt::connect::Connect;
use virt::sys::VIR_DOMAIN_XML_INACTIVE;

use super::domain::lookup_domain;
use crate::build_precheck::filesystem_avail_bytes;
use crate::state::CreateSnapshotRequest;
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMemoryTopology {
    pub vcpus: u32,
    pub sockets: u32,
    pub cores: u32,
    pub threads: u32,
    pub current_memory_kib: u64,
    pub max_memory_kib: u64,
    pub state: String,
    pub has_vfio_hostdev: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPrecheck {
    pub ok: bool,
    pub blocked: bool,
    pub message: String,
    pub has_vfio_hostdev: bool,
    pub estimated_bytes: u64,
    pub available_bytes: Option<u64>,
}

pub fn has_vfio_hostdev(xml: &str) -> bool {
    let lower = xml.to_ascii_lowercase();
    if lower.contains("driver name='vfio'") || lower.contains("driver name=\"vfio\"") {
        return true;
    }
    for block in crate::xml::split_blocks(xml, "hostdev") {
        let typ = crate::xml::extract_attr(&block, "hostdev", "type").unwrap_or_default();
        if typ == "pci" || typ == "usb" {
            let inner = block.to_ascii_lowercase();
            if inner.contains("vfio") {
                return true;
            }
        }
    }
    false
}

fn parse_topology(xml: &str) -> (u32, u32, u32) {
    if let Some(block) = crate::xml::split_blocks(xml, "topology").into_iter().next() {
        let sockets = crate::xml::extract_attr(&block, "topology", "sockets")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let cores = crate::xml::extract_attr(&block, "topology", "cores")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let threads = crate::xml::extract_attr(&block, "topology", "threads")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        return (sockets.max(1), cores.max(1), threads.max(1));
    }
    (1, 1, 1)
}

fn memory_kib_from_xml(xml: &str, tag: &str) -> u64 {
    for block in crate::xml::split_blocks(xml, tag) {
        if let Some(v) = crate::xml::extract_text(&block, tag)
            .or_else(|| crate::xml::extract_attr(&block, tag, "value"))
            .and_then(|s| s.parse().ok())
        {
            return v;
        }
    }
    crate::xml::extract_text(xml, tag)
        .and_then(|s| s.parse().ok())
        .or_else(|| crate::xml::extract_attr(xml, tag, "value").and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

pub fn get_cpu_memory_topology(
    conn: &Connect,
    name: &str,
) -> Result<CpuMemoryTopology, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get domain info"))?;
    let active_xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get active XML"))?;
    let config_xml = domain
        .get_xml_desc(VIR_DOMAIN_XML_INACTIVE)
        .unwrap_or_else(|_| active_xml.clone());

    let (sockets, cores, threads) = parse_topology(&config_xml);
    let max_memory_kib = memory_kib_from_xml(&config_xml, "memory");
    let current_memory_kib = memory_kib_from_xml(&active_xml, "currentMemory");
    let current_memory_kib = if current_memory_kib > 0 {
        current_memory_kib
    } else {
        info.memory
    };
    let max_memory_kib = if max_memory_kib > 0 {
        max_memory_kib
    } else {
        info.max_mem
    };

    Ok(CpuMemoryTopology {
        vcpus: info.nr_virt_cpu,
        sockets,
        cores,
        threads,
        current_memory_kib,
        max_memory_kib,
        state: super::domain::state_to_string(info.state),
        has_vfio_hostdev: has_vfio_hostdev(&active_xml),
    })
}

pub fn set_cpu_topology(
    conn: &Connect,
    name: &str,
    sockets: u32,
    cores: u32,
    threads: u32,
) -> Result<(), LibvirtError> {
    if sockets == 0 || cores == 0 || threads == 0 {
        return Err(LibvirtError::Invalid(
            "sockets, cores, and threads must be >= 1".into(),
        ));
    }
    let vcpus = sockets
        .checked_mul(cores)
        .and_then(|v| v.checked_mul(threads))
        .ok_or_else(|| LibvirtError::Invalid("vCPU count overflow".into()))?;
    crate::validate::validate_vcpus(vcpus)?;

    let domain = lookup_domain(conn, name)?;
    let mut xml = domain
        .get_xml_desc(VIR_DOMAIN_XML_INACTIVE)
        .map_err(LibvirtError::map_op("Failed to get inactive XML"))?;

    xml = replace_or_insert_vcpu(&xml, vcpus);
    xml = replace_or_insert_topology(&xml, sockets, cores, threads);

    virt::domain::Domain::define_xml(conn, &xml).map_err(|e| {
        LibvirtError::Operation(format!("Failed to set CPU topology for '{name}': {e}"))
    })?;
    Ok(())
}

/// Rewrite `<vcpu>` so `current` is the boot/active count and the element body is the
/// maximum (with hotplug headroom). Domains created with max == current cannot grow live.
pub(crate) fn replace_or_insert_vcpu(xml: &str, vcpus: u32) -> String {
    let max = super::create::vcpu_max_for(vcpus).max(vcpus);
    let replacement = format!("<vcpu placement='static' current='{vcpus}'>{max}</vcpu>");
    if let Some(start) = xml.find("<vcpu") {
        if let Some(end) = xml[start..].find('>') {
            let close = start + end + 1; // index just past the first '>'
            let tag = &xml[start..close]; // "<vcpu .../>" or "<vcpu ...>"
            let after = &xml[close..];
            // Self-closing `<vcpu/>`: the '>' we found already consumed the '/', so the
            // old `after.starts_with("/>")` check was dead and fell through to inserting
            // a SECOND <vcpu> before <memory> (invalid XML). Detect self-close on the tag
            // itself and replace the whole element.
            if tag.trim_end().ends_with("/>") {
                return format!("{}{}{}", &xml[..start], replacement, after);
            }
            if let Some(tag_end) = after.find("</vcpu>") {
                let end_idx = close + tag_end + "</vcpu>".len();
                return format!("{}{}{}", &xml[..start], replacement, &xml[end_idx..]);
            }
        }
    }
    if let Some(idx) = xml.find("<memory") {
        return format!("{}\n  {}\n{}", &xml[..idx], replacement, &xml[idx..]);
    }
    xml.to_string()
}

/// Parse persistent-domain max vCPUs from inactive XML (`<vcpu>…</vcpu>` body, else `current`).
pub(crate) fn parse_vcpu_max_from_xml(xml: &str) -> Option<u32> {
    let start = xml.find("<vcpu")?;
    let gt = xml[start..].find('>')? + start;
    let tag = &xml[start..=gt];
    if tag.trim_end().ends_with("/>") {
        return crate::xml::extract_attr(tag, "vcpu", "current").and_then(|s| s.parse().ok());
    }
    let after = &xml[gt + 1..];
    let end = after.find("</vcpu>")?;
    let body = after[..end].trim();
    if let Ok(n) = body.parse::<u32>() {
        return Some(n);
    }
    crate::xml::extract_attr(tag, "vcpu", "current").and_then(|s| s.parse().ok())
}

fn replace_or_insert_topology(xml: &str, sockets: u32, cores: u32, threads: u32) -> String {
    let topo = format!("<topology sockets='{sockets}' cores='{cores}' threads='{threads}'/>");
    // Replace existing <topology .../> inside the <cpu> block.
    if let Some(start) = xml.find("<topology") {
        if let Some(end) = xml[start..].find("/>") {
            let end_idx = start + end + 2;
            return format!("{}{}{}", &xml[..start], topo, &xml[end_idx..]);
        }
    }
    if let Some(cpu_start) = xml.find("<cpu") {
        let cpu_substr = &xml[cpu_start..];
        // Self-closing <cpu ... /> — convert to open block with topology inside.
        if let Some(sc_off) = cpu_substr.find("/>") {
            if !cpu_substr[..sc_off].contains('>') {
                let attrs = cpu_substr[..sc_off].trim_start_matches("<cpu");
                let new_cpu = format!("<cpu{attrs}>\n  {topo}\n</cpu>");
                let after = &xml[cpu_start + sc_off + 2..];
                return format!("{}{}{}", &xml[..cpu_start], new_cpu, after);
            }
        }
        // Open <cpu ...>...</cpu> — insert topology right after the opening >.
        if let Some(cpu_gt) = cpu_substr.find('>') {
            let insert_at = cpu_start + cpu_gt + 1;
            return format!("{}\n  {}{}", &xml[..insert_at], topo, &xml[insert_at..]);
        }
    }
    if let Some(idx) = xml.find("<vcpu") {
        let cpu_block =
            format!("<cpu mode='host-passthrough' check='partial'>\n  {topo}\n</cpu>\n  ");
        return format!("{}{}{}", &xml[..idx], cpu_block, &xml[idx..]);
    }
    xml.to_string()
}

pub fn snapshot_precheck(
    conn: &Connect,
    vm_name: &str,
    req: &CreateSnapshotRequest,
) -> Result<SnapshotPrecheck, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let is_active = domain.is_active().unwrap_or(false);
    let active_xml = domain.get_xml_desc(0).unwrap_or_default();
    let vfio = has_vfio_hostdev(&active_xml);

    if is_active && vfio {
        return Ok(SnapshotPrecheck {
            ok: false,
            blocked: true,
            message: "Creating snapshots of VMs with VFIO devices is not supported while they are running."
                .into(),
            has_vfio_hostdev: true,
            estimated_bytes: 0,
            available_bytes: None,
        });
    }

    let storage_mode = {
        let m = req.storage_mode.trim().to_ascii_lowercase();
        if m.is_empty() || m == "auto" {
            super::storage::primary_vm_disk_base_dir(conn)
                .map(|_| "external".into())
                .unwrap_or_else(|| "internal".into())
        } else {
            m
        }
    };

    let mut estimated_bytes = 0u64;
    if storage_mode == "external" && is_active && !req.disk_only {
        let info = domain.get_info().ok();
        let mem_kib = info.map(|i| i.memory).unwrap_or(0);
        estimated_bytes = mem_kib.saturating_mul(1024);
    }

    if storage_mode == "external" {
        let base = if !req.external_disk_dir.trim().is_empty() {
            req.external_disk_dir.trim().to_string()
        } else if !req.external_memory_dir.trim().is_empty() {
            req.external_memory_dir.trim().to_string()
        } else {
            super::storage::primary_vm_disk_base_dir(conn)
                .unwrap_or_else(|| "/var/lib/libvirt/images".to_string())
        };
        let parent = Path::new(&base);
        if parent.is_absolute() {
            if let Ok(avail) = filesystem_avail_bytes(parent) {
                if estimated_bytes > 0 && avail < estimated_bytes {
                    return Ok(SnapshotPrecheck {
                        ok: false,
                        blocked: true,
                        message: format!(
                            "Insufficient free space under {} for external snapshot (need ~{} MiB, {} MiB available)",
                            base,
                            estimated_bytes / (1024 * 1024),
                            avail / (1024 * 1024),
                        ),
                        has_vfio_hostdev: vfio,
                        estimated_bytes,
                        available_bytes: Some(avail),
                    });
                }
                return Ok(SnapshotPrecheck {
                    ok: true,
                    blocked: false,
                    message: String::new(),
                    has_vfio_hostdev: vfio,
                    estimated_bytes,
                    available_bytes: Some(avail),
                });
            }
        }
    }

    Ok(SnapshotPrecheck {
        ok: true,
        blocked: false,
        message: String::new(),
        has_vfio_hostdev: vfio,
        estimated_bytes,
        available_bytes: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_vcpu_sets_current_and_headroom_max() {
        let xml = "<domain>\n  <vcpu placement='static'>2</vcpu>\n  <memory unit='KiB'>1</memory>\n</domain>";
        let out = replace_or_insert_vcpu(xml, 3);
        assert!(out.contains("current='3'"), "{out}");
        assert!(out.contains(">12</vcpu>"), "{out}");
        assert_eq!(parse_vcpu_max_from_xml(&out), Some(12));
    }

    #[test]
    fn parse_vcpu_max_plain_element() {
        let xml = "<vcpu placement='static'>2</vcpu>";
        assert_eq!(parse_vcpu_max_from_xml(xml), Some(2));
    }
}
