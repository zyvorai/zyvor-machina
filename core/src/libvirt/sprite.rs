// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Boot a disposable "sprite" microVM off a golden-image backing clone.
//!
//! Deliberately bypasses `create::create_vm()`/`template_apply::apply_saved_template()`
//! — those pull in mkosi/virt-builder auto-detection and unconditionally call
//! `guest_agent_provision::inject_guestkit_into_disk()`, both real latency
//! costs that don't belong on the "~1 second, throwaway" path. This module
//! composes the same low-level primitives (`template_apply::materialize_from_base`,
//! `create::find_disk_path`) directly instead.

use std::fs;
use std::path::{Path, PathBuf};

use virt::connect::Connect;
use virt::domain::Domain;

use crate::LibvirtError;

use super::create::find_disk_path;
use super::template_apply::materialize_from_base;

/// Golden images live in their own flat directory, keyed by filename (no
/// `.json` metadata wrapper like `VmTemplate`'s saved-template registry —
/// sprites take vcpus/memory from the request, not the image, so there's no
/// per-image sizing metadata to store).
const SPRITE_IMAGES_DIR: &str = "/var/lib/machina/sprite-images";

/// Resolve a `golden_image` registry key to its qcow2 path.
///
/// `key` is expected to already be validated by the caller (e.g.
/// `spec::SpriteCreateRequest::validate()`, which rejects anything but
/// `validate_name`'s alphanumeric/-/_/ charset) — this function
/// independently re-validates with `core`'s own `validate::validate_name`
/// before joining `key` into a filesystem path, the same defense-in-depth
/// pattern `template_apply::apply_saved_template` already uses for its own
/// `saved_template` key. `core` has no dependency on the `spec` crate (kept
/// that way deliberately — see `spec`'s own doc comments), so this can't
/// call the spec-layer validator directly even if it wanted to.
pub fn resolve_golden_image(key: &str) -> Result<PathBuf, LibvirtError> {
    crate::validate::validate_name(key)?;
    let path = PathBuf::from(SPRITE_IMAGES_DIR).join(format!("{key}.qcow2"));
    if !path.is_file() {
        return Err(LibvirtError::NotFound(format!(
            "golden image '{key}' not found (expected {})",
            path.display()
        )));
    }
    Ok(path)
}

/// List available `golden_image` registry keys (bare filenames, `.qcow2`
/// stripped) — lets a caller (e.g. the web UI's sprite-creation form) offer
/// a picker instead of requiring the operator to already know what's on
/// disk. An absent `SPRITE_IMAGES_DIR` is treated as "no images yet", not an
/// error — nothing has failed, the registry is just empty until an image is
/// dropped in.
pub fn list_golden_images() -> Result<Vec<String>, LibvirtError> {
    let entries = match fs::read_dir(SPRITE_IMAGES_DIR) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(LibvirtError::Operation(format!(
                "failed to read sprite images dir: {e}"
            )))
        }
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("qcow2"))
        .filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    Ok(names)
}

pub struct SpriteBootRequest<'a> {
    /// Libvirt domain name — pass `spec::sprite_domain_name(sprite_id)`, not
    /// the bare sprite id, so `virsh list` and the reaper can recognize
    /// sprite domains by the `"sprite-"` prefix.
    pub domain_name: &'a str,
    pub golden_image_path: &'a Path,
    pub vcpus: u32,
    pub memory_mb: u64,
    /// Attach a virtio NIC to the host's "default" NAT network (`virbr0`)
    /// instead of staying vsock-only. See `spec::SpriteCreateRequest`'s
    /// `network_egress` doc comment for the isolation posture this implies.
    pub network_egress: bool,
}

pub struct SpriteBootResult {
    /// Overlay qcow2 path — the reaper deletes this on teardown, never
    /// `golden_image_path` (see `domain::delete_vm_with_options` /
    /// `domain::collect_disk_paths`, which only ever read the *domain's own*
    /// `<source file>`, never a backing-file's internal header).
    pub disk_path: String,
    /// `None` if the running domain's XML couldn't be parsed for a `<cid
    /// address='N'/>` — the domain is still up either way, this only affects
    /// whether the caller gets a vsock CID to connect to.
    pub vsock_cid: Option<u32>,
}

/// Clone `req.golden_image_path` via a qcow2 backing-file overlay, then
/// transiently define+start it in one libvirt call (`Domain::create_xml`,
/// not persistent — no `virsh undefine` needed on teardown beyond what
/// `delete_vm_with_options` already does for a running domain).
pub fn boot_sprite(
    conn: &Connect,
    req: &SpriteBootRequest,
) -> Result<SpriteBootResult, LibvirtError> {
    let dest = find_disk_path(conn, req.domain_name)?;
    let dest_path = Path::new(&dest);
    if dest_path.exists() {
        return Err(LibvirtError::Operation(format!(
            "Refusing to overwrite existing disk: {dest}"
        )));
    }
    materialize_from_base(req.golden_image_path, dest_path, "backing")?;

    let xml = generate_sprite_domain_xml(
        req.domain_name,
        &dest,
        req.vcpus,
        req.memory_mb,
        req.network_egress,
    );

    // No VIR_DOMAIN_START_AUTODESTROY: the daemon's LibvirtManager holds one
    // shared, long-lived connection reused (and transparently reconnected on
    // poison, see connection.rs::with_slot) for every libvirt call, not a
    // dedicated per-sprite connection — tying domain lifetime to that shared
    // connection's open/close cycle would risk silently destroying a live
    // sprite on an unrelated reconnect. Teardown is owned entirely by the
    // daemon's own TTL reaper instead.
    let domain = Domain::create_xml(conn, &xml, 0).map_err(|e| {
        // Best-effort cleanup: the domain never came up, don't leak the overlay.
        let _ = fs::remove_file(&dest);
        LibvirtError::Operation(format!("failed to create sprite domain: {e}"))
    })?;

    let vsock_cid = domain
        .get_xml_desc(0)
        .ok()
        .and_then(|xml| crate::xml::extract_attr(&xml, "cid", "address"))
        .and_then(|s| s.parse::<u32>().ok());

    Ok(SpriteBootResult {
        disk_path: dest,
        vsock_cid,
    })
}

/// Minimal headless domain: no graphics/VNC/SPICE/tablet/USB-controller/QGA
/// devices (all present unconditionally in `create::generate_domain_xml`,
/// none of it belongs on a fast, headless sandbox path). No network
/// `<interface>` by default (vsock only, no DHCP negotiation on the boot
/// path, no guest IP at all) unless `network_egress` opts into one — see
/// `SpriteBootRequest::network_egress`'s doc comment. BIOS, not UEFI: one
/// less firmware-load/NVRAM step for a minimal boot.
fn generate_sprite_domain_xml(
    name: &str,
    disk_path: &str,
    vcpus: u32,
    memory_mb: u64,
    network_egress: bool,
) -> String {
    let memory_kib = memory_mb * 1024;
    let name_esc = crate::xml::escape(name);
    let disk_path_esc = crate::xml::escape(disk_path);
    let interface_xml = if network_egress {
        "\n    <interface type='network'>\n      <source network='default'/>\n      <model type='virtio'/>\n    </interface>"
    } else {
        ""
    };
    format!(
        r#"<domain type='kvm'>
  <name>{name_esc}</name>
  <memory unit='KiB'>{memory_kib}</memory>
  <currentMemory unit='KiB'>{memory_kib}</currentMemory>
  <vcpu placement='static'>{vcpus}</vcpu>
  <os>
    <type arch='x86_64' machine='q35'>hvm</type>
    <boot dev='hd'/>
  </os>
  <features>
    <acpi/>
    <apic/>
  </features>
  <cpu mode='host-passthrough' check='none'/>
  <clock offset='utc'/>
  <on_poweroff>destroy</on_poweroff>
  <on_reboot>destroy</on_reboot>
  <on_crash>destroy</on_crash>
  <devices>
    <disk type='file' device='disk'>
      <driver name='qemu' type='qcow2'/>
      <source file='{disk_path_esc}'/>
      <target dev='vda' bus='virtio'/>
    </disk>
    <vsock model='virtio'>
      <cid auto='yes'/>
    </vsock>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>{interface_xml}
  </devices>
</domain>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_golden_image_rejects_path_traversal() {
        assert!(resolve_golden_image("../../etc/passwd").is_err());
        assert!(resolve_golden_image("images/python-minimal").is_err());
    }

    #[test]
    fn resolve_golden_image_not_found_is_not_found_error() {
        match resolve_golden_image("definitely-does-not-exist-abc123") {
            Err(LibvirtError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn list_golden_images_succeeds_whether_or_not_the_dir_exists() {
        // A missing /var/lib/machina/sprite-images (most dev/CI hosts) means
        // "no images yet", not a failure — and on a host that already has
        // sprite images (e.g. a deployed daemon host this test happens to
        // run on), any non-empty listing is equally valid. Only the ok/err
        // outcome is asserted; the contents aren't test-environment-stable.
        let names = list_golden_images().expect("missing dir must not be an error");
        for name in &names {
            assert!(
                !name.contains('/'),
                "golden image name should be bare: {name}"
            );
        }
    }

    #[test]
    fn generated_xml_has_no_network_or_graphics_devices_by_default() {
        let xml = generate_sprite_domain_xml(
            "sprite-abc",
            "/var/lib/libvirt/images/sprite-abc.qcow2",
            1,
            512,
            false,
        );
        assert!(!xml.contains("<interface"));
        assert!(!xml.contains("<graphics"));
        assert!(!xml.contains("<video"));
        assert!(!xml.contains("<channel"));
        assert!(xml.contains("<vsock"));
        assert!(xml.contains("<cid auto='yes'/>"));
    }

    #[test]
    fn generated_xml_adds_default_network_interface_when_egress_requested() {
        let xml = generate_sprite_domain_xml(
            "sprite-abc",
            "/var/lib/libvirt/images/sprite-abc.qcow2",
            1,
            512,
            true,
        );
        assert!(xml.contains("<interface type='network'>"));
        assert!(xml.contains("<source network='default'/>"));
        // vsock stays present regardless — network_egress adds a NIC, it
        // doesn't replace the vsock channel.
        assert!(xml.contains("<vsock"));
    }

    #[test]
    fn generated_xml_escapes_disk_path() {
        let xml = generate_sprite_domain_xml("sprite-abc", "/tmp/weird'&path.qcow2", 1, 512, false);
        assert!(!xml.contains("weird'&path"));
    }
}
