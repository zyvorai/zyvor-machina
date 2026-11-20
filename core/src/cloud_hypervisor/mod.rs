// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Cloud Hypervisor as an alternate microVM backend for disposable "sprite"
//! sandboxes (see `crate::libvirt::sprite` for the original libvirt/QEMU
//! backend). Cloud Hypervisor VMs are launched as plain child processes
//! supervised directly by `machina-daemon` — there is no libvirtd in this
//! path — and controlled via the `ch-remote` CLI against the VMM's
//! Unix-socket API, mirroring how `libvirt::template_apply` already shells
//! out to `qemu-img` rather than linking a client library.

pub mod sprite;

use std::path::Path;

use crate::sprite_net::find_vmm_binary;
use crate::LibvirtError;

/// Resolve the `cloud-hypervisor` VMM binary.
pub fn find_cloud_hypervisor_binary() -> Result<String, LibvirtError> {
    find_vmm_binary(
        "cloud-hypervisor",
        &[
            "/usr/bin/cloud-hypervisor",
            "/usr/local/bin/cloud-hypervisor",
        ],
    )
}

/// Resolve the `ch-remote` control CLI (ships alongside `cloud-hypervisor`).
pub fn find_ch_remote_binary() -> Result<String, LibvirtError> {
    find_vmm_binary(
        "ch-remote",
        &["/usr/bin/ch-remote", "/usr/local/bin/ch-remote"],
    )
}

/// Resolve the Cloud Hypervisor firmware image (`CLOUDHV.fd`, from the
/// `cloud-hypervisor/edk2` project). Cloud Hypervisor has no built-in BIOS
/// the way QEMU does — booting a disk image without either `--kernel` or
/// `--firmware` fails immediately (`the following required arguments were
/// not provided: --firmware <firmware>|--kernel <kernel>`), so this is
/// exactly as required as the VMM binary itself for the sprite boot path.
pub fn find_cloud_hypervisor_firmware() -> Result<String, LibvirtError> {
    let candidates = [
        "/usr/share/cloud-hypervisor/CLOUDHV.fd",
        "/usr/local/share/cloud-hypervisor/CLOUDHV.fd",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return Ok(path.to_string());
        }
    }
    Err(LibvirtError::NotFound(
        "Cloud Hypervisor firmware (CLOUDHV.fd) not found — install.sh's ensure_cloud_hypervisor \
         fetches it alongside the cloud-hypervisor binary"
            .into(),
    ))
}
