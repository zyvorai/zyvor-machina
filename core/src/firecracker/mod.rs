// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Firecracker as a third microVM backend for disposable "sprite" sandboxes
//! (alongside `crate::libvirt::sprite`, the original libvirt/QEMU backend,
//! and `crate::cloud_hypervisor`). Firecracker VMs are launched as plain
//! child processes supervised directly by `machina-daemon` — same posture
//! as Cloud Hypervisor, no libvirtd in this path — and configured entirely
//! via its HTTP-over-Unix-socket control API (unlike Cloud Hypervisor,
//! which configures everything via CLI flags and boots immediately on
//! spawn). Each configuration step below shells `curl --unix-socket`,
//! mirroring how `core::cloud_hypervisor` already shells `ch-remote`
//! rather than linking an HTTP-over-UDS client — see that module's doc
//! comment for the same reasoning.

pub mod sprite;

use std::path::Path;

use crate::sprite_net::find_vmm_binary;
use crate::LibvirtError;

/// Resolve the `firecracker` VMM binary.
pub fn find_firecracker_binary() -> Result<String, LibvirtError> {
    find_vmm_binary(
        "firecracker",
        &["/usr/local/bin/firecracker", "/usr/bin/firecracker"],
    )
}

/// Resolve the shared `vmlinux` kernel image Firecracker boots directly
/// (no BIOS/UEFI, no bootloader, no reading the guest's own installed
/// kernel — unlike every other sprite backend). One kernel shared across
/// every Firecracker sprite on this host, mirroring
/// `cloud_hypervisor::find_cloud_hypervisor_firmware`'s single shared
/// `CLOUDHV.fd` — a golden image's own `/boot` contents go unused when
/// booted this way.
pub fn find_firecracker_kernel() -> Result<String, LibvirtError> {
    let candidates = [
        "/usr/local/share/firecracker/vmlinux",
        "/usr/share/firecracker/vmlinux",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return Ok(path.to_string());
        }
    }
    Err(LibvirtError::NotFound(
        "Firecracker kernel (vmlinux) not found — install.sh's ensure_firecracker fetches it \
         alongside the firecracker binary"
            .into(),
    ))
}
