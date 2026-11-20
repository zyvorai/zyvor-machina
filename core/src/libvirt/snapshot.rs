// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use tracing::warn;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::domain_snapshot::DomainSnapshot;
use virt::sys;

use super::domain::lookup_domain;
use super::storage;
use crate::state::SnapshotInfo;
use crate::state::{CreateSnapshotRequest, SnapshotDiskSpec};
use crate::xml;
use crate::LibvirtError;

pub fn list_snapshots(conn: &Connect, vm_name: &str) -> Result<Vec<SnapshotInfo>, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;

    let snaps = domain
        .list_all_snapshots(0)
        .map_err(LibvirtError::map_op("Failed to list snapshots"))?;

    let current = DomainSnapshot::current(&domain, 0)
        .ok()
        .and_then(|s| s.get_name().ok());

    let mut result = Vec::new();
    for snap in snaps {
        let name = snap
            .get_name()
            .map_err(LibvirtError::map_op("Failed to get snapshot name"))?;

        let xml_str = match snap.get_xml_desc(0) {
            Ok(x) => x,
            Err(e) => {
                warn!("Failed to get XML for snapshot '{}': {}", name, e);
                String::new()
            }
        };

        let creation_time = xml::extract_simple_text(&xml_str, "creationTime")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let state =
            xml::extract_simple_text(&xml_str, "state").unwrap_or_else(crate::unknown_string);
        let description = xml::extract_simple_text(&xml_str, "description").unwrap_or_default();

        let parent = extract_parent_name(&xml_str).unwrap_or_default();

        let is_current = current.as_deref() == Some(name.as_str());

        result.push(SnapshotInfo {
            name,
            vm_name: vm_name.to_string(),
            creation_time,
            state,
            description,
            parent,
            is_current,
        });
    }

    Ok(result)
}

pub fn list_all_snapshots(conn: &Connect) -> Result<Vec<SnapshotInfo>, LibvirtError> {
    let domains = conn
        .list_all_domains(0)
        .map_err(LibvirtError::map_op("Failed to list domains"))?;

    let mut all_snaps = Vec::new();
    for domain in domains {
        let name = domain.get_name().unwrap_or_default();
        match list_snapshots(conn, &name) {
            Ok(snaps) => all_snaps.extend(snaps),
            Err(e) => warn!("Failed to list snapshots for VM '{}': {}", name, e),
        }
    }

    Ok(all_snaps)
}

fn parse_domain_disks_for_snapshot(xml_str: &str) -> Vec<(String, Option<String>, Option<String>)> {
    // Returns (target_dev, source_file, source_dev) for device='disk'
    let mut out = Vec::new();
    for block in crate::xml::split_blocks(xml_str, "disk") {
        let device = crate::xml::extract_attr(&block, "disk", "device").unwrap_or_default();
        if device != "disk" {
            continue;
        }
        let target = crate::xml::extract_attr(&block, "target", "dev").unwrap_or_default();
        if target.is_empty() {
            continue;
        }
        let file = crate::xml::extract_attr(&block, "source", "file");
        let dev = crate::xml::extract_attr(&block, "source", "dev");
        out.push((target, file, dev));
    }
    out
}

fn normalize_mode(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn default_storage_mode(conn: &Connect) -> String {
    // Cockpit uses external snapshots when libvirt >= 9.9.0.
    match conn.get_lib_version() {
        Ok(v) if v >= 9_009_000 => "external".into(),
        _ => "internal".into(),
    }
}

fn validate_snapshot_file_output(conn: &Connect, path: &str) -> Result<(), LibvirtError> {
    if path.trim().is_empty() {
        return Ok(());
    }
    storage::assert_new_disk_output_parent_allowed(conn, path)?;
    Ok(())
}

fn join_dir_file(dir: &str, file: &str) -> String {
    let d = dir.trim_end_matches('/');
    format!("{d}/{file}")
}

fn safe_name_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub(crate) fn guest_fs_freeze(domain: &Domain) -> Result<i32, LibvirtError> {
    let ret = unsafe { sys::virDomainFSFreeze(domain.as_ptr(), std::ptr::null_mut(), 0, 0) };
    if ret < 0 {
        return Err(LibvirtError::Internal("guest fs freeze failed".into()));
    }
    Ok(ret)
}

fn guest_fs_thaw(domain: &Domain) {
    let _ = unsafe { sys::virDomainFSThaw(domain.as_ptr(), std::ptr::null_mut(), 0, 0) };
}

/// RAII guard that thaws the guest filesystems on drop unless disarmed. This pairs a
/// `guest_fs_freeze` with a guaranteed thaw on EVERY exit path — including the many
/// `?` early returns between freeze and snapshot creation. Without it, a failed
/// snapshot (bad path, validation error, unsupported storage) left the guest's
/// filesystems frozen indefinitely, stalling all guest I/O until a manual
/// `virsh domfsthaw`.
pub(crate) struct ThawGuard<'a> {
    pub(crate) domain: &'a Domain,
    pub(crate) armed: bool,
}

impl Drop for ThawGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            guest_fs_thaw(self.domain);
        }
    }
}

pub fn create_snapshot(
    conn: &Connect,
    vm_name: &str,
    req: &CreateSnapshotRequest,
) -> Result<(), LibvirtError> {
    if req.name.trim().is_empty() {
        return Err(LibvirtError::Invalid("snapshot name is required".into()));
    }
    let pre = super::cpu_memory::snapshot_precheck(conn, vm_name, req)?;
    if pre.blocked {
        return Err(LibvirtError::Operation(pre.message));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let is_active = domain.is_active().unwrap_or(false);
    // Armed only if the freeze actually succeeded; drops (thaws) on every return path.
    let mut thaw_guard = ThawGuard {
        domain: &domain,
        armed: false,
    };
    if req.quiesce && is_active {
        match guest_fs_freeze(&domain) {
            Ok(n) if n >= 0 => thaw_guard.armed = true,
            Ok(_) => warn!("guest fs freeze returned unexpected count for '{vm_name}'"),
            Err(e) => warn!("guest fs freeze skipped for '{vm_name}': {e}"),
        }
    }
    let dom_xml = domain.get_xml_desc(0).unwrap_or_default();
    let dom_disks = parse_domain_disks_for_snapshot(&dom_xml);

    let disk_only = req.disk_only;
    let storage_mode = {
        let m = normalize_mode(&req.storage_mode);
        if m.is_empty() || m == "auto" {
            default_storage_mode(conn)
        } else {
            m
        }
    };
    if storage_mode != "external" && storage_mode != "internal" {
        return Err(LibvirtError::Invalid(
            "storage_mode must be 'auto', 'external', or 'internal'".into(),
        ));
    }

    let mut flags: u32 = 0;
    if disk_only {
        flags |= sys::VIR_DOMAIN_SNAPSHOT_CREATE_DISK_ONLY;
    }
    if req.atomic {
        flags |= sys::VIR_DOMAIN_SNAPSHOT_CREATE_ATOMIC;
    }
    if req.reuse_external {
        flags |= sys::VIR_DOMAIN_SNAPSHOT_CREATE_REUSE_EXT;
    }

    // Memory settings (only meaningful when not disk-only)
    let mut memory_xml = String::new();
    if disk_only {
        memory_xml.push_str("\n  <memory snapshot='no'/>");
    } else {
        if !is_active {
            // Offline snapshots cannot include VM memory state.
            memory_xml.push_str("\n  <memory snapshot='no'/>");
        } else {
            let mm = normalize_mode(&req.memory_snapshot);
            let mm = if mm.is_empty() {
                // Cockpit-style: match storage mode
                storage_mode.clone()
            } else {
                mm
            };
            if mm != "internal" && mm != "external" {
                return Err(LibvirtError::Invalid(
                    "memory_snapshot must be 'internal' or 'external' (or empty for default)"
                        .into(),
                ));
            }
            if mm == "external" {
                let mut mem_file = req.memory_file.trim().to_string();
                if mem_file.is_empty() {
                    // Default under primary VM disk dir (pool target)
                    let base = super::storage::primary_vm_disk_base_dir(conn)
                        .unwrap_or_else(|| "/var/lib/libvirt/images".to_string());
                    let dir = if !req.external_memory_dir.trim().is_empty() {
                        req.external_memory_dir.trim().to_string()
                    } else {
                        base
                    };
                    let f = format!(
                        "{}-{}-mem.save",
                        safe_name_component(vm_name),
                        safe_name_component(req.name.trim())
                    );
                    mem_file = join_dir_file(&dir, &f);
                }
                validate_snapshot_file_output(conn, &mem_file)?;
                memory_xml.push_str(&format!(
                    "\n  <memory snapshot='external' file='{}'/>",
                    crate::xml::escape(&mem_file)
                ));
            } else {
                memory_xml.push_str("\n  <memory snapshot='internal'/>");
            }
        }
    }

    // Disk specs
    let mut disk_specs: Vec<SnapshotDiskSpec> = req.disks.clone();
    // Normalize empty snapshot strings to default by storage_mode
    for d in &mut disk_specs {
        if d.snapshot.trim().is_empty() {
            d.snapshot = storage_mode.clone();
        }
    }

    let mut disks_xml = String::new();
    if !disk_specs.is_empty() || storage_mode == "external" || disk_only {
        let mut lines: Vec<String> = Vec::new();
        // Build desired map: name -> spec
        let mut map: std::collections::HashMap<String, SnapshotDiskSpec> =
            std::collections::HashMap::new();
        for d in disk_specs {
            map.insert(d.name.trim().to_string(), d);
        }
        // For each domain disk, decide snapshot mode
        for (target, src_file, src_dev) in dom_disks.iter() {
            let spec = map.get(target);
            let mode = spec
                .map(|s| normalize_mode(&s.snapshot))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| storage_mode.clone());

            let mode = if mode == "yes" {
                "external".to_string()
            } else {
                mode
            };
            if mode != "no" && mode != "external" && mode != "internal" && mode != "manual" {
                return Err(LibvirtError::Invalid(format!(
                    "invalid disk snapshot mode for {target}: {mode} (use no|external|internal|manual)"
                )));
            }

            if mode == "no" {
                lines.push(format!(
                    "    <disk name='{}' snapshot='no'/>",
                    crate::xml::escape(target)
                ));
                continue;
            }

            if mode == "internal" && storage_mode == "internal" {
                // A network-backed disk (src_file and src_dev both absent —
                // e.g. `<source protocol='rbd' .../>`) has no qcow2 file for a
                // libvirt-internal snapshot to live in; Ceph has its own,
                // different native snapshot mechanism. Refuse cleanly instead
                // of letting libvirt fail with an opaque error deep in QEMU.
                if src_file.is_none() && src_dev.is_none() {
                    return Err(LibvirtError::Invalid(format!(
                        "disk '{target}' is network-backed (e.g. Ceph/RBD) — internal \
                         qcow2-style snapshots aren't supported for it; use the storage \
                         backend's own snapshot mechanism instead"
                    )));
                }
                lines.push(format!(
                    "    <disk name='{}' snapshot='internal'/>",
                    crate::xml::escape(target)
                ));
                continue;
            }

            if mode == "manual" {
                lines.push(format!(
                    "    <disk name='{}' snapshot='manual'/>",
                    crate::xml::escape(target)
                ));
                continue;
            }

            // external
            let mut file = spec.map(|s| s.file.trim().to_string()).unwrap_or_default();
            let driver = spec
                .and_then(|s| {
                    let t = s.driver.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                })
                .unwrap_or_else(|| "qcow2".to_string());

            if file.is_empty() && !req.external_disk_dir.trim().is_empty() {
                let f = format!(
                    "{}-{}-{}.qcow2",
                    safe_name_component(vm_name),
                    safe_name_component(req.name.trim()),
                    safe_name_component(target)
                );
                file = join_dir_file(req.external_disk_dir.trim(), &f);
            }

            if file.is_empty() {
                // For file-backed disks libvirt can auto-generate; for block disks it cannot.
                if src_file.is_none() && src_dev.is_some() {
                    return Err(LibvirtError::Invalid(format!(
                        "disk '{target}' is a block device; external snapshots require an explicit file path"
                    )));
                }
                // Neither file= nor dev= present — this is a network-backed
                // source (e.g. `<source protocol='rbd' .../>`), not a plain
                // file disk. Omitting <source> here would let libvirt try to
                // auto-generate a snapshot target the same way it does for a
                // real file disk, which fails with a confusing, unrelated
                // libvirt/QEMU-level error instead of an actionable one.
                if src_file.is_none() && src_dev.is_none() {
                    return Err(LibvirtError::Invalid(format!(
                        "disk '{target}' is network-backed (e.g. Ceph/RBD); external \
                         snapshots require an explicit file path here too"
                    )));
                }
                // omit <source> to let libvirt generate
                lines.push(format!(
                    "    <disk name='{}' snapshot='external'><driver type='{}'/></disk>",
                    crate::xml::escape(target),
                    crate::xml::escape(&driver),
                ));
            } else {
                validate_snapshot_file_output(conn, &file)?;
                lines.push(format!(
                    "    <disk name='{}' snapshot='external'>\n      <driver type='{}'/>\n      <source file='{}'/>\n    </disk>",
                    crate::xml::escape(target),
                    crate::xml::escape(&driver),
                    crate::xml::escape(&file),
                ));
            }
        }
        if !lines.is_empty() {
            disks_xml.push_str("\n  <disks>\n");
            disks_xml.push_str(&lines.join("\n"));
            disks_xml.push_str("\n  </disks>");
        }
    }

    let xml_str = format!(
        r#"<domainsnapshot>
  <name>{}</name>
  <description>{}</description>{}{} 
</domainsnapshot>"#,
        crate::xml::escape(req.name.trim()),
        crate::xml::escape(req.description.trim()),
        memory_xml,
        disks_xml,
    );

    DomainSnapshot::create_xml(&domain, &xml_str, flags)
        .map_err(LibvirtError::map_op("Failed to create snapshot"))?;
    // Success path: `thaw_guard` thaws on drop at function exit (same as every error
    // path), so no explicit thaw is needed here.
    Ok(())
}

pub fn delete_snapshot(conn: &Connect, vm_name: &str, snap_name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;

    let snap = DomainSnapshot::lookup_by_name(&domain, snap_name, 0)
        .map_err(|e| LibvirtError::NotFound(format!("Snapshot '{snap_name}' not found: {e}")))?;

    let flags: u32 = 0;
    // Libvirt may attempt block-commit when deleting external snapshots, which often fails on active domains
    // with "write lock" errors. Cockpit typically avoids destructive/merging deletes on running guests.
    if domain.is_active().unwrap_or(false) {
        let xml = snap.get_xml_desc(0).unwrap_or_default();
        let looks_external =
            xml.contains("snapshot='external'") || xml.contains("snapshot=\"external\"");
        if looks_external {
            // On a running guest the only option libvirt allows is METADATA_ONLY, which
            // drops tracking but leaves the overlay qcow2 files unmerged on disk — a
            // silent space leak and on-disk/reported chain divergence. Refuse and steer
            // the caller to stop the VM so the delete can actually merge the overlay.
            return Err(LibvirtError::Invalid(format!(
                "Cannot delete external snapshot '{snap_name}' while VM '{vm_name}' is \
                 running (would orphan overlay files). Stop the VM and retry."
            )));
        }
    }

    snap.delete(flags)
        .map_err(LibvirtError::map_op("Failed to delete snapshot"))?;

    Ok(())
}

pub fn revert_snapshot(conn: &Connect, vm_name: &str, snap_name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;

    let snap = DomainSnapshot::lookup_by_name(&domain, snap_name, 0)
        .map_err(|e| LibvirtError::NotFound(format!("Snapshot '{snap_name}' not found: {e}")))?;

    snap.revert(0)
        .map_err(LibvirtError::map_op("Failed to revert snapshot"))?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotActionPrecheck {
    pub ok: bool,
    pub blocked: bool,
    pub message: String,
    pub vm_running: bool,
    pub external_snapshot: bool,
    pub has_vfio_hostdev: bool,
}

pub fn snapshot_action_precheck(
    conn: &Connect,
    vm_name: &str,
    snap_name: &str,
    action: &str,
) -> Result<SnapshotActionPrecheck, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let running = domain.is_active().unwrap_or(false);
    let active_xml = domain.get_xml_desc(0).unwrap_or_default();
    let vfio = super::cpu_memory::has_vfio_hostdev(&active_xml);

    let snap = DomainSnapshot::lookup_by_name(&domain, snap_name, 0)
        .map_err(|e| LibvirtError::NotFound(format!("Snapshot '{snap_name}' not found: {e}")))?;
    let snap_xml = snap.get_xml_desc(0).unwrap_or_default();
    let external =
        snap_xml.contains("snapshot='external'") || snap_xml.contains("snapshot=\"external\"");

    let action = action.trim().to_ascii_lowercase();
    let (ok, blocked, message) = match action.as_str() {
        "delete" => {
            if running && external {
                (
                    true,
                    false,
                    "Running guest with external snapshot — libvirt will delete metadata only; disk chains may remain until merged offline."
                        .into(),
                )
            } else if running && vfio {
                (
                    false,
                    true,
                    "Deleting snapshots of running VMs with VFIO devices is not supported.".into(),
                )
            } else {
                (true, false, String::new())
            }
        }
        "revert" => {
            if running && vfio {
                (
                    false,
                    true,
                    "Reverting snapshots while VFIO passthrough devices are attached is not supported on a running guest."
                        .into(),
                )
            } else if running {
                (
                    true,
                    false,
                    "Reverting a running guest may briefly pause I/O — ensure the guest is quiesced or shut off for safest results."
                        .into(),
                )
            } else {
                (true, false, String::new())
            }
        }
        "clone" => {
            if running {
                (
                    true,
                    false,
                    "Cloning from a snapshot of a running guest copies disk state at snapshot time; the new VM starts from that point-in-time image."
                        .into(),
                )
            } else {
                (true, false, String::new())
            }
        }
        other => {
            return Err(LibvirtError::Invalid(format!(
                "unknown snapshot action precheck: {other}"
            )));
        }
    };

    Ok(SnapshotActionPrecheck {
        ok,
        blocked,
        message,
        vm_running: running,
        external_snapshot: external,
        has_vfio_hostdev: vfio,
    })
}

fn extract_parent_name(xml_str: &str) -> Option<String> {
    let blocks = xml::split_blocks(xml_str, "parent");
    blocks
        .first()
        .and_then(|block| xml::extract_simple_text(block, "name"))
}
