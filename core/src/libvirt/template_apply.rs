// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Merge saved JSON templates and materialize golden-image disks (backing file or full copy).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use virt::connect::Connect;

use crate::state::{CreateVmRequest, VmTemplate};
use crate::LibvirtError;

const TEMPLATES_DIR: &str = "/var/lib/machina/templates";

/// If `req.saved_template` is set, load `/var/lib/machina/templates/{name}.json`, overwrite sizing/os fields,
/// and optionally create a new qcow2 from `base_image` (backing or copy).
pub fn apply_saved_template(conn: &Connect, req: &mut CreateVmRequest) -> Result<(), LibvirtError> {
    let key = req.saved_template.trim();
    if key.is_empty() {
        return Ok(());
    }
    crate::validate::validate_name(key)?;
    let path = format!("{TEMPLATES_DIR}/{key}.json");
    let raw = fs::read_to_string(&path)
        .map_err(|e| LibvirtError::NotFound(format!("Saved template '{key}': {e}")))?;
    let tmpl: VmTemplate = serde_json::from_str(&raw)
        .map_err(|e| LibvirtError::Operation(format!("Invalid template JSON for '{key}': {e}")))?;

    req.vcpus = tmpl.vcpus;
    req.memory_mb = tmpl.memory_mb;
    req.disk_gb = tmpl.disk_gb;
    req.os_variant = tmpl.os_variant.clone();

    if let Some(ref base) = tmpl.base_image {
        let base_path = Path::new(base);
        if !base_path.is_absolute() {
            return Err(LibvirtError::Invalid(
                "Template base_image must be an absolute path".into(),
            ));
        }
        let base_path = base_path
            .canonicalize()
            .map_err(|e| LibvirtError::Invalid(format!("Cannot resolve base_image: {e}")))?;
        if !base_path.is_file() {
            return Err(LibvirtError::Operation(format!(
                "Template base_image is not a file: {}",
                base_path.display()
            )));
        }
        let mode = effective_disk_mode(req, &tmpl);
        let dest = crate::libvirt::create::find_disk_path(conn, &req.name)?;
        if Path::new(&dest).exists() {
            return Err(LibvirtError::Operation(format!(
                "Refusing to overwrite existing disk: {dest}"
            )));
        }
        materialize_from_base(&base_path, Path::new(&dest), mode)?;
        req.existing_disk = dest;
    }

    Ok(())
}

fn effective_disk_mode(req: &CreateVmRequest, tmpl: &VmTemplate) -> &'static str {
    // Default to a full copy ("copy"): a COW linked clone ("backing") shares the
    // template base as a backing file, which produces a multi-level backing chain
    // once the VM is snapshotted — libvirt records the chain only one level deep, so
    // the template base is omitted from the VM's AppArmor profile and qemu is denied
    // reading it on restart. A full copy avoids that. "backing" stays available as an
    // explicit opt-in for callers that want thin/instant provisioning and won't rely
    // on snapshot+restart.
    let m = req.template_disk_mode.trim();
    if m == "copy" || m == "backing" {
        return if m == "copy" { "copy" } else { "backing" };
    }
    let m2 = tmpl.template_disk_mode.trim();
    if m2 == "backing" {
        "backing"
    } else {
        "copy"
    }
}

/// Create a new qcow2 from a source image (`backing` = linked clone, `copy` = full copy).
pub fn materialize_from_base(base: &Path, dest: &Path, mode: &str) -> Result<(), LibvirtError> {
    match mode {
        "copy" => {
            let out = Command::new("qemu-img")
                .args([
                    "convert",
                    "-O",
                    "qcow2",
                    "-c",
                    base.to_str().ok_or_else(|| {
                        LibvirtError::Invalid("base_image path is not valid UTF-8".into())
                    })?,
                    dest.to_str().ok_or_else(|| {
                        LibvirtError::Invalid("destination path is not valid UTF-8".into())
                    })?,
                ])
                .output()
                .map_err(|e| LibvirtError::Operation(format!("qemu-img convert: {e}")))?;
            if !out.status.success() {
                let _ = fs::remove_file(dest);
                return Err(LibvirtError::Operation(format!(
                    "qemu-img convert failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
        }
        _ => {
            let out = Command::new("qemu-img")
                .args([
                    "create",
                    "-f",
                    "qcow2",
                    "-F",
                    "qcow2",
                    "-b",
                    base.to_str().ok_or_else(|| {
                        LibvirtError::Invalid("base_image path is not valid UTF-8".into())
                    })?,
                    dest.to_str().ok_or_else(|| {
                        LibvirtError::Invalid("destination path is not valid UTF-8".into())
                    })?,
                ])
                .output()
                .map_err(|e| LibvirtError::Operation(format!("qemu-img create: {e}")))?;
            if !out.status.success() {
                let _ = fs::remove_file(dest);
                return Err(LibvirtError::Operation(format!(
                    "qemu-img create (backing) failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
        }
    }
    Ok(())
}

/// First boot disk path from domain XML (`device='disk'` with `file=` source).
pub fn primary_disk_path_from_xml(xml: &str) -> Option<PathBuf> {
    for block in crate::xml::split_blocks(xml, "disk") {
        let device = crate::xml::extract_attr(&block, "disk", "device").unwrap_or_default();
        if device != "disk" {
            continue;
        }
        if let Some(p) = crate::xml::extract_attr(&block, "source", "file") {
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    None
}
