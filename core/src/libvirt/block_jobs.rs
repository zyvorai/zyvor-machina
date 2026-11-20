// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Block layer jobs (`virDomainBlockCommit`, `virDomainBlockPull`, job info/abort) via libvirt.
//! Used for snapshot backing-chain maintenance (flatten / merge).

use std::ffi::CString;
use std::mem;

use virt::connect::Connect;
use virt::sys;

use super::domain::lookup_domain;
use crate::LibvirtError;

fn virt_err(op: &str, e: virt::error::Error) -> LibvirtError {
    LibvirtError::Operation(format!("{op}: {e}"))
}

/// Result of `virDomainGetBlockJobInfo` when a job is active on `disk`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockJobInfo {
    /// `VIR_DOMAIN_BLOCK_JOB_TYPE_*` (pull, commit, copy, …).
    pub job_type: i32,
    pub bandwidth: u64,
    pub cur: u64,
    pub end: u64,
}

/// `virDomainBlockCommit` — merge backing files into `base` (see libvirt docs for `base`/`top`).
///
/// `disk` is the guest disk identifier (e.g. `vda` or volume path) as accepted by libvirt.
pub fn block_commit(
    conn: &Connect,
    vm_name: &str,
    disk: &str,
    base: Option<&str>,
    top: Option<&str>,
    bandwidth: u64,
    flags: u32,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let disk_c =
        CString::new(disk).map_err(|_| LibvirtError::Invalid("disk: invalid C string".into()))?;
    let base_c = base
        .map(CString::new)
        .transpose()
        .map_err(|_| LibvirtError::Invalid("base: invalid C string".into()))?;
    let top_c = top
        .map(CString::new)
        .transpose()
        .map_err(|_| LibvirtError::Invalid("top: invalid C string".into()))?;

    let base_ptr = base_c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
    let top_ptr = top_c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());

    let ret = unsafe {
        sys::virDomainBlockCommit(
            domain.as_ptr(),
            disk_c.as_ptr(),
            base_ptr,
            top_ptr,
            bandwidth as libc::c_ulong,
            flags,
        )
    };
    if ret == -1 {
        return Err(virt_err(
            "virDomainBlockCommit",
            virt::error::Error::last_error(),
        ));
    }
    Ok(())
}

/// `virDomainBlockPull` — pull data from backing image into the active overlay.
pub fn block_pull(
    conn: &Connect,
    vm_name: &str,
    disk: &str,
    bandwidth: u64,
    flags: u32,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let disk_c =
        CString::new(disk).map_err(|_| LibvirtError::Invalid("disk: invalid C string".into()))?;
    let ret = unsafe {
        sys::virDomainBlockPull(
            domain.as_ptr(),
            disk_c.as_ptr(),
            bandwidth as libc::c_ulong,
            flags,
        )
    };
    if ret == -1 {
        return Err(virt_err(
            "virDomainBlockPull",
            virt::error::Error::last_error(),
        ));
    }
    Ok(())
}

/// Returns `Ok(None)` when no block job is active on `disk`.
pub fn block_job_info(
    conn: &Connect,
    vm_name: &str,
    disk: &str,
    flags: u32,
) -> Result<Option<BlockJobInfo>, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let disk_c =
        CString::new(disk).map_err(|_| LibvirtError::Invalid("disk: invalid C string".into()))?;
    let mut info = mem::MaybeUninit::<sys::virDomainBlockJobInfo>::uninit();
    let ret = unsafe {
        sys::virDomainGetBlockJobInfo(domain.as_ptr(), disk_c.as_ptr(), info.as_mut_ptr(), flags)
    };
    if ret < 0 {
        return Err(virt_err(
            "virDomainGetBlockJobInfo",
            virt::error::Error::last_error(),
        ));
    }
    if ret == 0 {
        return Ok(None);
    }
    let info = unsafe { info.assume_init() };
    Ok(Some(BlockJobInfo {
        job_type: info.type_,
        bandwidth: info.bandwidth,
        cur: info.cur,
        end: info.end,
    }))
}

/// Abort an active block job on `disk` (`VIR_DOMAIN_BLOCK_JOB_ABORT_*` in `flags`).
pub fn block_job_abort(
    conn: &Connect,
    vm_name: &str,
    disk: &str,
    flags: u32,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let disk_c =
        CString::new(disk).map_err(|_| LibvirtError::Invalid("disk: invalid C string".into()))?;
    let ret = unsafe { sys::virDomainBlockJobAbort(domain.as_ptr(), disk_c.as_ptr(), flags) };
    if ret == -1 {
        return Err(virt_err(
            "virDomainBlockJobAbort",
            virt::error::Error::last_error(),
        ));
    }
    Ok(())
}

/// OR together `VIR_DOMAIN_BLOCK_COMMIT_*` bits from common options.
pub fn block_commit_flags(
    shallow: bool,
    delete: bool,
    active: bool,
    relative: bool,
    bandwidth_bytes: bool,
) -> u32 {
    let mut f = 0u32;
    if shallow {
        f |= sys::VIR_DOMAIN_BLOCK_COMMIT_SHALLOW;
    }
    if delete {
        f |= sys::VIR_DOMAIN_BLOCK_COMMIT_DELETE;
    }
    if active {
        f |= sys::VIR_DOMAIN_BLOCK_COMMIT_ACTIVE;
    }
    if relative {
        f |= sys::VIR_DOMAIN_BLOCK_COMMIT_RELATIVE;
    }
    if bandwidth_bytes {
        f |= sys::VIR_DOMAIN_BLOCK_COMMIT_BANDWIDTH_BYTES;
    }
    f
}

pub fn block_pull_flags(bandwidth_bytes: bool) -> u32 {
    if bandwidth_bytes {
        sys::VIR_DOMAIN_BLOCK_PULL_BANDWIDTH_BYTES
    } else {
        0
    }
}

pub fn block_job_abort_flags(r#async: bool, pivot: bool) -> u32 {
    let mut f = 0u32;
    if r#async {
        f |= sys::VIR_DOMAIN_BLOCK_JOB_ABORT_ASYNC;
    }
    if pivot {
        f |= sys::VIR_DOMAIN_BLOCK_JOB_ABORT_PIVOT;
    }
    f
}

pub fn block_job_info_flags(bandwidth_bytes: bool) -> u32 {
    if bandwidth_bytes {
        sys::VIR_DOMAIN_BLOCK_JOB_INFO_BANDWIDTH_BYTES
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::block_commit_flags;
    use virt::sys;

    #[test]
    fn block_commit_flags_combines_bits() {
        let f = block_commit_flags(true, true, false, false, true);
        assert!(f & sys::VIR_DOMAIN_BLOCK_COMMIT_SHALLOW != 0);
        assert!(f & sys::VIR_DOMAIN_BLOCK_COMMIT_DELETE != 0);
        assert!(f & sys::VIR_DOMAIN_BLOCK_COMMIT_BANDWIDTH_BYTES != 0);
    }
}
