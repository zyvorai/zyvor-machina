// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared validation for virt-image-build (sync POST and async jobs).

use machina_core::build_precheck;
use machina_core::config::MachinaConfig;
use machina_core::libvirt::storage;
use machina_core::validate::{
    validate_virt_builder_hostname, validate_virt_builder_os, validate_virt_builder_password_file,
    validate_virt_builder_ssh_pubkey_line,
};
use machina_core::LibvirtError;
use virt::connect::Connect;
use virt_image_build::BuildDiskRequest;

const VIB_LIST_MAX: usize = 64;
const VIB_STR_MAX: usize = 2048;

fn validate_vib_string_list(items: &[String], label: &str) -> Result<(), LibvirtError> {
    if items.len() > VIB_LIST_MAX {
        return Err(LibvirtError::Invalid(format!(
            "{label}: at most {VIB_LIST_MAX} entries"
        )));
    }
    for s in items {
        if s.len() > VIB_STR_MAX {
            return Err(LibvirtError::Invalid(format!(
                "{label}: each entry must be at most {VIB_STR_MAX} characters"
            )));
        }
    }
    Ok(())
}

fn validate_vib_install_list(s: &str) -> Result<(), LibvirtError> {
    if s.len() > 8192 {
        return Err(LibvirtError::Invalid("install string is too long".into()));
    }
    let mut n = 0usize;
    for pkg in s.split(',') {
        let p = pkg.trim();
        if p.is_empty() {
            continue;
        }
        n += 1;
        if n > 512 {
            return Err(LibvirtError::Invalid(
                "install: too many package names".into(),
            ));
        }
        if p.len() > 128 {
            return Err(LibvirtError::Invalid(format!(
                "install: package name too long: {p}"
            )));
        }
    }
    Ok(())
}

fn validate_optional_host_file(
    path: &str,
    label: &str,
    max_bytes: u64,
) -> Result<(), LibvirtError> {
    let p = path.trim();
    if p.is_empty() {
        return Ok(());
    }
    let pb = std::path::Path::new(p);
    if !pb.is_absolute() {
        return Err(LibvirtError::Invalid(format!(
            "{label} must be an absolute path"
        )));
    }
    let meta = std::fs::metadata(pb).map_err(|e| LibvirtError::Invalid(format!("{label}: {e}")))?;
    if !meta.is_file() {
        return Err(LibvirtError::Invalid(format!(
            "{label} must be a regular file"
        )));
    }
    if meta.len() > max_bytes {
        return Err(LibvirtError::Invalid(format!(
            "{label} must be at most {max_bytes} bytes"
        )));
    }
    Ok(())
}

/// Full request validation that requires a libvirt connection (output directory policy).
pub fn validate_virt_image_build(
    conn: &Connect,
    req: &BuildDiskRequest,
) -> Result<(), LibvirtError> {
    let out_path = req.output.trim();
    if out_path.is_empty() {
        return Err(LibvirtError::Invalid("output is required".into()));
    }
    storage::assert_new_disk_output_parent_allowed(conn, out_path)?;
    let cfg = MachinaConfig::load();
    let pb = std::path::Path::new(out_path);
    let parent = pb
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| LibvirtError::Invalid("output has no parent directory".into()))?;
    let parent_canon = parent.canonicalize().map_err(|e| {
        LibvirtError::Invalid(format!(
            "output parent directory does not exist or is inaccessible: {e}"
        ))
    })?;
    build_precheck::precheck_virt_builder_host_env(
        &parent_canon,
        cfg.libvirt.virt_image_build_min_free_parent_bytes,
        cfg.libvirt.virt_image_build_min_free_tmp_bytes,
    )?;
    validate_virt_builder_os(req.os.trim())?;
    validate_virt_builder_hostname(req.hostname.trim())?;

    if let Some(ref f) = req.root_password_file {
        if !f.trim().is_empty() {
            validate_virt_builder_password_file(f.trim())?;
        }
    }
    if let Some(ref li) = req.root_password_inline {
        if li.len() > 4096 {
            return Err(LibvirtError::Invalid(
                "root_password_inline must be at most 4096 characters".into(),
            ));
        }
    }

    if let Some(ref f) = req.ssh_pubkey_file {
        if !f.trim().is_empty() {
            validate_optional_host_file(f.trim(), "ssh_pubkey_file", 65_536)?;
        }
    }
    if let Some(ref li) = req.ssh_pubkey_inline {
        if !li.trim().is_empty() {
            validate_virt_builder_ssh_pubkey_line(li.trim())?;
        }
    }

    validate_vib_string_list(&req.run_command, "run_command")?;
    validate_vib_string_list(&req.copy_in, "copy_in")?;
    validate_vib_string_list(&req.extra_virt_builder_args, "extra_virt_builder_args")?;
    validate_vib_install_list(&req.install)?;

    if let Some(ref fb) = req.firstboot_script {
        validate_optional_host_file(fb, "firstboot_script", 256 * 1024)?;
    }

    let fmt = req.format.trim();
    if fmt.is_empty() || fmt.len() > 32 {
        return Err(LibvirtError::Invalid("invalid disk format".into()));
    }
    if !fmt.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(LibvirtError::Invalid(
            "format: use letters, digits, hyphen only".into(),
        ));
    }

    let sz = req.size.trim();
    if sz.is_empty() || sz.len() > 32 {
        return Err(LibvirtError::Invalid("invalid size".into()));
    }

    Ok(())
}
