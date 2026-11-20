// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Disk / TMPDIR checks before running libguestfs `virt-builder` (avoids obscure failures mid-build).

use std::path::Path;

use crate::LibvirtError;

/// Free space available to unprivileged quota on `path`'s filesystem (`f_bavail` × `f_frsize`).
#[cfg(unix)]
pub fn filesystem_avail_bytes(path: &Path) -> Result<u64, LibvirtError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| LibvirtError::Invalid("path contains NUL byte".into()))?;
    // SAFETY: c is NUL-terminated; path is an existing directory we just canonicalized.
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut vfs) };
    if rc != 0 {
        return Err(LibvirtError::Operation(format!(
            "statvfs {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(vfs.f_bavail.saturating_mul(vfs.f_frsize))
}

#[cfg(not(unix))]
pub fn filesystem_avail_bytes(_path: &Path) -> Result<u64, LibvirtError> {
    Ok(u64::MAX)
}

/// Effective TMPDIR for libguestfs tools (same default as typical Unix).
pub fn effective_tmpdir() -> std::path::PathBuf {
    std::env::var_os("TMPDIR")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
}

/// Ensure enough free space on the output image parent FS and on TMPDIR.
///
/// `output_parent` should be the **canonical** directory that will hold the new qcow2.
#[cfg(unix)]
pub fn precheck_virt_builder_host_env(
    output_parent: &Path,
    min_parent_free: u64,
    min_tmp_free: u64,
) -> Result<(), LibvirtError> {
    let parent_avail = filesystem_avail_bytes(output_parent)?;
    if parent_avail < min_parent_free {
        return Err(LibvirtError::Invalid(format!(
            "Insufficient disk space for virt-builder output under {}: {} bytes free; configure [libvirt] virt_image_build_min_free_parent_bytes (currently need at least {} bytes)",
            output_parent.display(),
            parent_avail,
            min_parent_free
        )));
    }

    let tmpdir = effective_tmpdir();
    let tmp_meta = std::fs::metadata(&tmpdir).map_err(|e| {
        LibvirtError::Invalid(format!(
            "TMPDIR {} is not accessible (libguestfs needs a writable temp area): {e}",
            tmpdir.display()
        ))
    })?;
    if !tmp_meta.is_dir() {
        return Err(LibvirtError::Invalid(format!(
            "TMPDIR {} is not a directory",
            tmpdir.display()
        )));
    }
    let tmp_canon = tmpdir.canonicalize().map_err(|e| {
        LibvirtError::Invalid(format!(
            "TMPDIR {} could not be canonicalized: {e}",
            tmpdir.display()
        ))
    })?;
    let tmp_avail = filesystem_avail_bytes(&tmp_canon)?;
    if tmp_avail < min_tmp_free {
        return Err(LibvirtError::Invalid(format!(
            "Insufficient disk space on TMPDIR {} (resolved {}): {} bytes free; libguestfs needs scratch space — set TMPDIR to a larger volume or raise [libvirt] virt_image_build_min_free_tmp_bytes (need at least {} bytes)",
            tmpdir.display(),
            tmp_canon.display(),
            tmp_avail,
            min_tmp_free
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn precheck_virt_builder_host_env(
    _output_parent: &Path,
    _min_parent_free: u64,
    _min_tmp_free: u64,
) -> Result<(), LibvirtError> {
    Ok(())
}
