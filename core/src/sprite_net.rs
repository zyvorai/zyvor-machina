// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared helpers for disposable "sprite" microVM backends that supervise
//! their own VMM child process directly (`crate::cloud_hypervisor`,
//! `crate::firecracker`) rather than going through libvirtd. Kept separate
//! from `crate::libvirt::sprite` (which owns the shared golden-image
//! registry) since these concerns — per-instance run directories, opt-in
//! TAP/NAT egress, VMM binary discovery — apply identically regardless of
//! which non-libvirt VMM is in use.

use std::path::Path;
use std::process::Command;

use crate::LibvirtError;

/// Per-sprite runtime artifacts (disk copy/conversion, API socket, vsock
/// socket) for any directly-supervised VMM backend live under here,
/// namespaced by sprite id. Separate from
/// `crate::libvirt::sprite::SPRITE_IMAGES_DIR` (the shared, read-only
/// golden image registry) since these are per-instance and disposed of on
/// teardown.
pub const SPRITE_RUN_DIR: &str = "/var/lib/machina/sprite-run";

/// Bridge sprites-with-egress attach to — the host's pre-existing libvirt
/// "default" NAT network, not a new one this module sets up (see
/// `spec::SpriteCreateRequest::network_egress`'s doc comment for why:
/// reuses existing, already-tested NAT/DHCP infrastructure instead of
/// duplicating it, at the cost of sharing that network's posture with
/// regular VMs).
pub const EGRESS_BRIDGE: &str = "virbr0";

/// Linux interface names are capped at 15 characters (`IFNAMSIZ` is 16
/// including the nul terminator) — a full sprite UUID doesn't fit, so this
/// derives a short, still-namespaced name from it. `prefix` distinguishes
/// which backend owns the TAP (e.g. `"chv-"`, `"fc-"`) so two backends'
/// sprites can never collide on the same device name even with an
/// overlapping first-8-alphanumeric-chars sprite id.
pub fn tap_name_for(prefix: &str, sprite_id: &str) -> String {
    let short: String = sprite_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!("{prefix}{short}")
}

fn run_ip(args: &[&str]) -> Result<(), LibvirtError> {
    let out = Command::new("ip")
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("ip {args:?}: {e}")))?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "ip {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Create a TAP device and attach it to `EGRESS_BRIDGE`, matching the
/// libvirt "default" network's own NAT/DHCP setup — `ip tuntap add` +
/// `ip link set master` + `ip link set up`, cleaning up the TAP again if any
/// step after creation fails.
pub fn create_egress_tap(tap: &str) -> Result<(), LibvirtError> {
    run_ip(&["tuntap", "add", tap, "mode", "tap"])?;
    if let Err(e) = run_ip(&["link", "set", tap, "master", EGRESS_BRIDGE]) {
        delete_egress_tap(tap);
        return Err(e);
    }
    if let Err(e) = run_ip(&["link", "set", tap, "up"]) {
        delete_egress_tap(tap);
        return Err(e);
    }
    Ok(())
}

/// Best-effort: also detaches the TAP from its bridge, since deleting the
/// interface removes it from `EGRESS_BRIDGE` as a side effect.
pub fn delete_egress_tap(tap: &str) {
    let _ = Command::new("ip").args(["link", "del", tap]).output();
}

/// Resolve a VMM (or VMM control CLI) binary: try a fixed list of likely
/// install paths first, then fall back to `which`. Unlike
/// `translate::qemu::find_qemu_binary` (which silently falls back to a
/// guessed default path — reasonable for QEMU, which is all but guaranteed
/// present on any libvirt/KVM host), a missing Cloud Hypervisor/Firecracker
/// install is a real "not installed" condition: failing fast here means the
/// corresponding sprite backend rejects the request at boot time with a
/// clear error, the same posture as an unresolvable golden image.
pub(crate) fn find_vmm_binary(name: &str, candidates: &[&str]) -> Result<String, LibvirtError> {
    for path in candidates {
        if Path::new(path).is_file() {
            return Ok((*path).to_string());
        }
    }
    if let Ok(out) = Command::new("which").arg(name).output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Ok(p);
            }
        }
    }
    Err(LibvirtError::NotFound(format!(
        "{name} not found on PATH (install it to use this sprite backend)"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_vmm_binary_missing_is_not_found() {
        match find_vmm_binary("definitely-not-a-real-binary-xyz", &["/no/such/path"]) {
            Err(LibvirtError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn tap_name_for_uses_the_given_prefix_and_stays_within_ifnamsiz() {
        let name = tap_name_for("fc-", "f625d42b-8f9e-4713-ab52-d6c5ae23346e");
        assert!(name.starts_with("fc-"));
        assert!(name.len() <= 15, "tap name {name} exceeds IFNAMSIZ-1");
    }

    #[test]
    fn tap_name_for_distinguishes_backends_on_the_same_sprite_id() {
        let sprite_id = "abcdef1234567890";
        assert_ne!(
            tap_name_for("chv-", sprite_id),
            tap_name_for("fc-", sprite_id)
        );
    }
}
