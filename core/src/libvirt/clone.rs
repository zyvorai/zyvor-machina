// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use virt::connect::Connect;
use virt::domain::Domain;

use super::create::find_disk_path;
use super::domain::lookup_domain;
use super::template_apply::materialize_from_base;
use crate::LibvirtError;

/// `linked` — qcow2 backing file; `full` — independent copy; `xml` — legacy shared-disk define (unsafe).
pub fn normalize_clone_disk_mode(mode: &str) -> &'static str {
    match mode.trim().to_lowercase().as_str() {
        "full" | "copy" => "copy",
        "xml" | "shared" => "xml",
        _ => "backing",
    }
}

/// Clone a VM with independent or linked disks. Returns the new domain UUID.
pub fn clone_vm_with_disk(
    conn: &Connect,
    source_name: &str,
    new_name: &str,
    disk_mode: &str,
) -> Result<String, LibvirtError> {
    crate::validate::validate_name(new_name)?;
    let mode = normalize_clone_disk_mode(disk_mode);

    if mode == "xml" {
        clone_vm_xml_only(conn, source_name, new_name)?;
    } else {
        let source = lookup_domain(conn, source_name)?;
        // A "backing" (linked) clone points the new disk's qcow2 backing file
        // straight at the source's own disk. If the source is running, that disk
        // is live and writable — the still-running source and the new clone would
        // both write through the *same* backing chain, corrupting whichever one
        // writes to a shared cluster second. Rather than guess at a safe
        // snapshot-then-link scheme here, require the source to be shut off for a
        // linked clone; a caller that needs to clone a live VM can pass
        // disk_mode='full' for an independent copy instead.
        if mode == "backing" && source.is_active().unwrap_or(true) {
            return Err(LibvirtError::Invalid(format!(
                "VM '{source_name}' is running — a linked clone would back the new disk onto \
                 the source's live, writable qcow2, corrupting both once each writes through \
                 the shared chain. Shut '{source_name}' down first, or pass disk_mode='full' \
                 to make an independent copy while it keeps running."
            )));
        }
        let xml = source
            .get_xml_desc(0)
            .map_err(LibvirtError::map_op("Failed to get XML"))?;
        // Clone EVERY file-backed data disk, not just the first. The primary disk keeps
        // the {new_name}.qcow2 name; secondaries get {new_name}-{target}.qcow2, and each
        // is repointed in the cloned XML. (Cloning only the first left a multi-disk VM's
        // other disks sharing the source's backing → corruption.)
        let disks = file_backed_disks_from_xml(&xml);
        if disks.is_empty() {
            return Err(LibvirtError::Operation(format!(
                "no disk path found for VM '{source_name}'"
            )));
        }
        // `file_backed_disks_from_xml` only ever collects `<source file=.../>`
        // disks; a network-backed one (`<source protocol='rbd' name=.../>`, no
        // `file=` attribute) is invisible to it and therefore to `disk_map`
        // below. Without this check, `repoint_disks` only rewrites paths that
        // ARE in the map, so an RBD disk gets copied into the new domain's XML
        // completely unchanged — the clone and the source end up pointing at
        // the *same* Ceph image. Starting both is silent, concurrent-write disk
        // corruption, not a loud failure. Refuse rather than risk that; cloning
        // network-backed storage needs its own volume-clone path (e.g. through
        // Atlas), which this function does not implement.
        if let Some(net_target) = first_network_backed_disk_target(&xml) {
            return Err(LibvirtError::Invalid(format!(
                "VM '{source_name}' has a network-backed disk at target '{net_target}' \
                 (e.g. Ceph/RBD) — cloning it would leave the clone and the source \
                 sharing the same storage. Clone is only supported for file-backed disks."
            )));
        }
        let primary_dest = find_disk_path(conn, new_name)?;
        let dest_dir = Path::new(&primary_dest)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/var/lib/libvirt/images".to_string());
        let mat_mode = if mode == "copy" { "copy" } else { "backing" };

        let mut disk_map: Vec<(String, String)> = Vec::new();
        let mut created: Vec<String> = Vec::new();
        // Roll back any disks already copied if a later one fails.
        let cleanup = |created: &[String]| {
            for c in created {
                let _ = std::fs::remove_file(c);
            }
        };
        for (i, (target, src)) in disks.iter().enumerate() {
            if !Path::new(src).is_file() {
                cleanup(&created);
                return Err(LibvirtError::Operation(format!(
                    "source disk not found: {src}"
                )));
            }
            let dest = if i == 0 {
                primary_dest.clone()
            } else {
                let suffix = if target.is_empty() {
                    format!("disk{i}")
                } else {
                    target.clone()
                };
                format!(
                    "{}/{}-{}.qcow2",
                    dest_dir.trim_end_matches('/'),
                    new_name,
                    suffix
                )
            };
            if Path::new(&dest).exists() {
                cleanup(&created);
                return Err(LibvirtError::Operation(format!(
                    "Refusing to overwrite existing disk: {dest}"
                )));
            }
            if let Err(e) = materialize_from_base(Path::new(src), Path::new(&dest), mat_mode) {
                cleanup(&created);
                return Err(e);
            }
            created.push(dest.clone());
            disk_map.push((src.clone(), dest));
        }
        if let Err(e) = define_cloned_domain(conn, &xml, new_name, &disk_map) {
            cleanup(&created);
            return Err(e);
        }
    }

    let dom = lookup_domain(conn, new_name)?;
    dom.get_uuid_string()
        .map_err(LibvirtError::map_op("Failed to get cloned VM UUID"))
}

/// Legacy XML-only clone (shared disk paths). Prefer [`clone_vm_with_disk`].
pub fn clone_vm(conn: &Connect, source_name: &str, new_name: &str) -> Result<(), LibvirtError> {
    clone_vm_xml_only(conn, source_name, new_name)
}

fn clone_vm_xml_only(
    conn: &Connect,
    source_name: &str,
    new_name: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(new_name)?;
    let source = lookup_domain(conn, source_name)?;
    let xml = source
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;
    let new_xml = replace_domain_name(&xml, new_name).ok_or_else(|| {
        LibvirtError::Operation(
            "Failed to replace VM name in XML: <name> element not found".to_string(),
        )
    })?;
    let new_xml = remove_xml_element(&new_xml, "uuid");
    let new_xml = randomize_mac_addresses(&new_xml);
    let new_xml = super::graphics_convert::ensure_graphics_present(&new_xml, "127.0.0.1");
    Domain::define_xml(conn, &new_xml)
        .map_err(LibvirtError::map_op("Failed to define cloned VM"))?;
    Ok(())
}

/// All file-backed `device='disk'` disks in a domain XML, as (target_dev, source_path).
/// cdrom / floppy and non-file disks are skipped so clone only copies real data disks.
fn file_backed_disks_from_xml(xml: &str) -> Vec<(String, String)> {
    let mut disks = Vec::new();
    for block in crate::xml::split_blocks(xml, "disk") {
        if crate::xml::extract_attr(&block, "disk", "device").as_deref() != Some("disk") {
            continue;
        }
        let src = crate::xml::extract_attr(&block, "source", "file").unwrap_or_default();
        if src.is_empty() {
            continue;
        }
        let target = crate::xml::extract_attr(&block, "target", "dev").unwrap_or_default();
        disks.push((target, src));
    }
    disks
}

/// The `target dev` of the first network-backed (`type='network'`, e.g. Ceph/RBD)
/// data disk in a domain XML, if any. Cdrom/floppy devices are excluded, same as
/// [`file_backed_disks_from_xml`].
fn first_network_backed_disk_target(xml: &str) -> Option<String> {
    crate::xml::split_blocks(xml, "disk")
        .into_iter()
        .find_map(|block| {
            if crate::xml::extract_attr(&block, "disk", "device").as_deref() != Some("disk") {
                return None;
            }
            if crate::xml::extract_attr(&block, "disk", "type").as_deref() != Some("network") {
                return None;
            }
            Some(crate::xml::extract_attr(&block, "target", "dev").unwrap_or_default())
        })
}

fn define_cloned_domain(
    conn: &Connect,
    source_xml: &str,
    new_name: &str,
    disk_map: &[(String, String)],
) -> Result<(), LibvirtError> {
    let new_xml = replace_domain_name(source_xml, new_name)
        .ok_or_else(|| LibvirtError::Operation("failed to replace domain name in XML".into()))?;
    let new_xml = remove_xml_element(&new_xml, "uuid");
    let new_xml = randomize_mac_addresses(&new_xml);
    let new_xml = repoint_disks(&new_xml, disk_map);
    let new_xml = super::graphics_convert::ensure_graphics_present(&new_xml, "127.0.0.1");
    Domain::define_xml(conn, &new_xml)
        .map_err(|e| LibvirtError::Operation(format!("define cloned VM: {e}")))?;
    Ok(())
}

/// Repoint each `<source file='OLD'/>` to its cloned copy per `disk_map` (old → new).
/// Only exact source-path matches are rewritten, so a cdrom/seed source (never in the
/// map) is left untouched.
fn repoint_disks(xml: &str, disk_map: &[(String, String)]) -> String {
    let mut out = String::new();
    for line in xml.lines() {
        let t = line.trim();
        let mapped = t
            .strip_prefix("<source file='")
            .and_then(|s| s.split('\'').next())
            .and_then(|old| disk_map.iter().find(|(o, _)| o == old));
        if let Some((_, new)) = mapped {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            out.push_str(&format!(
                "{indent}<source file='{}'/>\n",
                crate::xml::escape(new)
            ));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn replace_domain_name(xml: &str, new_name: &str) -> Option<String> {
    let start = xml.find("<name>")?;
    let end = xml.find("</name>")?;
    let before = &xml[..start];
    let after = &xml[end + "</name>".len()..];
    Some(format!(
        "{before}<name>{}</name>{after}",
        crate::xml::escape(new_name)
    ))
}

fn remove_xml_element(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let Some(start) = xml.find(&open) {
        if let Some(end_offset) = xml[start..].find(&close) {
            let end = start + end_offset + close.len();
            let after = &xml[end..];
            let trim_end = after.len() - after.trim_start().len();
            let mut result = xml[..start].to_string();
            result.push_str(&xml[end + trim_end..]);
            return result;
        }
    }
    xml.to_string()
}

fn randomize_mac_addresses(xml: &str) -> String {
    let mut result = String::new();
    let mut remaining = xml;
    let mut counter: u64 = 0;

    while let Some(pos) = remaining.find("<mac ") {
        result.push_str(&remaining[..pos]);
        let tag_start = &remaining[pos..];
        if let Some(end) = tag_start.find("/>") {
            let new_mac = generate_mac(counter);
            counter += 1;
            result.push_str(&format!("<mac address='{new_mac}'/>"));
            remaining = &remaining[pos + end + 2..];
        } else if let Some(end) = tag_start.find('>') {
            let new_mac = generate_mac(counter);
            counter += 1;
            result.push_str(&format!("<mac address='{new_mac}'/>"));
            remaining = &remaining[pos + end + 1..];
        } else {
            result.push_str(tag_start);
            remaining = "";
        }
    }
    result.push_str(remaining);
    result
}

fn generate_mac(counter: u64) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s1 = RandomState::new();
    let s2 = RandomState::new();
    let mut h1 = s1.build_hasher();
    let mut h2 = s2.build_hasher();
    h1.write_u64(counter);
    h2.write_u64(counter.wrapping_add(1));
    let hash1 = h1.finish();
    let hash2 = h2.finish();
    let b1 = hash1.to_le_bytes();
    let b2 = hash2.to_le_bytes();
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        b1[0] ^ b2[1],
        b1[2] ^ b2[3],
        b1[4] ^ b2[5]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED_DISK_VM: &str = r#"<domain>
      <devices>
        <disk type='file' device='disk'>
          <source file='/var/lib/libvirt/images/vm.qcow2'/>
          <target dev='vda' bus='virtio'/>
        </disk>
        <disk type='network' device='disk'>
          <source protocol='rbd' name='rbd-nvme-prod/csi-vol-abc'>
            <host name='10.43.1.1' port='6789'/>
          </source>
          <target dev='vdb' bus='virtio'/>
        </disk>
        <disk type='file' device='cdrom'>
          <source file='/var/lib/libvirt/images/isos/win.iso'/>
          <target dev='sda' bus='sata'/>
        </disk>
      </devices>
    </domain>"#;

    #[test]
    fn file_backed_disks_skip_the_network_backed_one() {
        // The bug this guards: an RBD disk has no `source file=`, so it was
        // silently invisible to disk_map/repoint_disks — the clone and source
        // would end up pointing at the same Ceph image.
        let disks = file_backed_disks_from_xml(MIXED_DISK_VM);
        assert_eq!(
            disks,
            vec![(
                "vda".to_string(),
                "/var/lib/libvirt/images/vm.qcow2".to_string()
            )]
        );
    }

    #[test]
    fn detects_the_network_backed_disk_that_would_be_silently_shared() {
        assert_eq!(
            first_network_backed_disk_target(MIXED_DISK_VM),
            Some("vdb".to_string())
        );
    }

    #[test]
    fn an_all_file_backed_vm_has_no_network_disk() {
        let xml = r#"<domain><devices>
            <disk type='file' device='disk'><source file='/a.qcow2'/><target dev='vda'/></disk>
        </devices></domain>"#;
        assert_eq!(first_network_backed_disk_target(xml), None);
    }
}
