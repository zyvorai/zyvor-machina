// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Update existing disk / NIC / video devices (bus, cache, model, …).

use virt::connect::Connect;
use virt::domain::Domain;

use super::device::get_domain_flags;
use super::domain::lookup_domain;
use crate::xml::{self, split_blocks};
use crate::LibvirtError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiskTuneRequest {
    pub target: String,
    #[serde(default)]
    pub bus: Option<String>,
    #[serde(default)]
    pub cache: Option<String>,
    #[serde(default)]
    pub discard: Option<String>,
    #[serde(default)]
    pub readonly: Option<bool>,
    #[serde(default)]
    pub shareable: Option<bool>,
}

fn replace_driver_element(block: &str, new_driver: &str) -> String {
    if let Some(start) = block.find("<driver") {
        let rest = &block[start..];
        if let Some(off) = rest.find("/>") {
            let end = start + off + 2;
            return format!("{}{}{}", &block[..start], new_driver, &block[end..]);
        }
        if let Some(off) = rest.find("</driver>") {
            let end = start + off + "</driver>".len();
            return format!("{}{}{}", &block[..start], new_driver, &block[end..]);
        }
    }
    // Insert after first line (opening <disk ...>)
    if let Some(gt) = block.find('>') {
        format!("{}{}{}", &block[..=gt], new_driver, &block[gt + 1..])
    } else {
        format!("{new_driver}{block}")
    }
}

fn set_disk_readonly_shareable(
    mut block: String,
    readonly: Option<bool>,
    shareable: Option<bool>,
) -> String {
    if let Some(ro) = readonly {
        if ro {
            if !block.contains("<readonly") {
                if let Some(pos) = block.rfind("</disk>") {
                    block.insert_str(pos, "    <readonly/>\n");
                }
            }
        } else {
            while let Some(i) = block.find("<readonly") {
                if let Some(j) = block[i..].find("/>") {
                    block.replace_range(i..i + j + 2, "");
                } else if let Some(j) = block[i..].find("</readonly>") {
                    block.replace_range(i..i + j + "</readonly>".len(), "");
                } else {
                    break;
                }
            }
        }
    }
    if let Some(sh) = shareable {
        if sh {
            if !block.contains("shareable='") && !block.contains("shareable=\"") {
                if let Some(p) = block.find("<disk") {
                    if let Some(gt) = block[p..].find('>') {
                        let ins = p + gt;
                        block.insert_str(ins, " shareable='yes'");
                    }
                }
            }
        } else {
            block = block
                .replace(" shareable='yes'", "")
                .replace(" shareable=\"yes\"", "");
        }
    }
    block
}

fn target_prefix_for_bus(bus: &str) -> &'static str {
    match bus {
        "virtio" => "vd",
        "scsi" | "sata" | "ide" | "usb" => "sd",
        _ => "sd",
    }
}

/// Map `vda`/`sda`/`hda`/`xvda` → letter index used for the remapped target.
fn disk_index_letter(target: &str) -> char {
    let stripped = target
        .strip_prefix("xvd")
        .or_else(|| target.strip_prefix("vd"))
        .or_else(|| target.strip_prefix("sd"))
        .or_else(|| target.strip_prefix("hd"))
        .unwrap_or(target);
    stripped
        .chars()
        .next()
        .filter(|c| c.is_ascii_lowercase())
        .unwrap_or('a')
}

fn remap_target_dev(old: &str, bus: &str) -> String {
    format!("{}{}", target_prefix_for_bus(bus), disk_index_letter(old))
}

/// Prefer the same letter as `old` only when it is free; otherwise the first free
/// slot from `a`. Scanning from `a` keeps virtio↔scsi round-trips on `vda` when
/// the CD-ROM occupies `sda` (mid-state `sdb` → restore `vda`, not `vdb`).
fn pick_free_target_dev(old: &str, bus: &str, used: &[String]) -> String {
    let prefix = target_prefix_for_bus(bus);
    for letter in 'a'..='z' {
        let cand = format!("{prefix}{letter}");
        if cand == old || !used.iter().any(|u| u == &cand) {
            return cand;
        }
    }
    remap_target_dev(old, bus)
}

fn used_disk_targets(desc: &str) -> Vec<String> {
    split_blocks(desc, "disk")
        .into_iter()
        .filter_map(|b| xml::extract_attr(&b, "target", "dev"))
        .collect()
}

/// Drop `<address…/>` so libvirt can assign a bus-appropriate address on redefine.
fn strip_disk_address(block: &str) -> String {
    let mut out = block.to_string();
    while let Some(i) = out.find("<address") {
        if let Some(j) = out[i..].find("/>") {
            let mut end = i + j + 2;
            if end < out.len() && out.as_bytes()[end] == b'\n' {
                end += 1;
            }
            out.replace_range(i..end, "");
        } else if let Some(j) = out[i..].find("</address>") {
            let mut end = i + j + "</address>".len();
            if end < out.len() && out.as_bytes()[end] == b'\n' {
                end += 1;
            }
            out.replace_range(i..end, "");
        } else {
            break;
        }
    }
    out
}

fn set_target_bus_on_block(
    block: &str,
    target_dev: &str,
    bus: &str,
    used_targets: &[String],
) -> Result<(String, String), LibvirtError> {
    let new_dev = pick_free_target_dev(target_dev, bus, used_targets);
    let needle1 = format!("dev='{target_dev}'");
    let needle2 = format!("dev=\"{target_dev}\"");
    let idx = block
        .find(&needle1)
        .or_else(|| block.find(&needle2))
        .ok_or_else(|| {
            LibvirtError::NotFound(format!("target dev '{target_dev}' not in disk block"))
        })?;
    let before = &block[..idx];
    let tstart = before
        .rfind("<target")
        .ok_or_else(|| LibvirtError::Operation("target tag parse".into()))?;
    let tag_src = &block[tstart..];
    let tag_end_rel = tag_src
        .find("/>")
        .map(|i| i + 2)
        .or_else(|| tag_src.find("</target>").map(|i| i + "</target>".len()))
        .ok_or_else(|| LibvirtError::Operation("target end".into()))?;
    let tag_end_abs = tstart + tag_end_rel;
    // Rebuild a clean target — bus + remapped dev (virtio↔scsi needs vd*↔sd*).
    let new_tag = format!(
        "<target dev='{}' bus='{}'/>",
        xml::escape(&new_dev),
        xml::escape(bus)
    );
    let mut nb = format!("{}{}{}", &block[..tstart], new_tag, &block[tag_end_abs..]);
    nb = strip_disk_address(&nb);
    Ok((nb, new_dev))
}

fn replace_exact_once(haystack: &str, old: &str, new: &str) -> Result<String, LibvirtError> {
    let Some(pos) = haystack.find(old) else {
        return Err(LibvirtError::Operation(
            "device block not found in domain XML".into(),
        ));
    };
    Ok(format!(
        "{}{}{}",
        &haystack[..pos],
        new,
        &haystack[pos + old.len()..]
    ))
}

/// Update disk `<driver>` / `<target bus>` / readonly / shareable for `target` dev (e.g. vda).
///
/// Bus changes remap the target name (`vda`↔`sda`) and drop the PCI/`address` node so libvirt
/// can assign a bus-appropriate address. Those edits go through `define_xml` because
/// `updateDeviceFlags` rejects cross-bus address types.
pub fn update_disk_tune(
    conn: &Connect,
    vm_name: &str,
    tune: &DiskTuneRequest,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(vm_name)?;
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;

    let mut orig_block: Option<String> = None;
    let mut new_block: Option<String> = None;
    let mut bus_changed = false;
    let used = used_disk_targets(&desc);
    // Exclude the disk we are editing so remap may keep the same name when valid.
    let used_others: Vec<String> = used.into_iter().filter(|t| t != &tune.target).collect();
    for block in split_blocks(&desc, "disk") {
        if xml::extract_attr(&block, "target", "dev").as_deref() == Some(tune.target.as_str()) {
            let driver_type =
                xml::extract_attr(&block, "driver", "type").unwrap_or_else(|| "qcow2".to_string());
            let mut parts = vec![
                "name='qemu'".to_string(),
                format!("type='{}'", xml::escape(&driver_type)),
            ];
            // Preserve existing cache/discard when not being overridden so a bus-only
            // tune does not wipe them via a minimal driver rewrite.
            let existing_cache = xml::extract_attr(&block, "driver", "cache");
            let existing_discard = xml::extract_attr(&block, "driver", "discard");
            if let Some(ref c) = tune.cache {
                parts.push(format!("cache='{}'", xml::escape(c)));
            } else if let Some(c) = existing_cache {
                parts.push(format!("cache='{}'", xml::escape(&c)));
            }
            if let Some(ref d) = tune.discard {
                parts.push(format!("discard='{}'", xml::escape(d)));
            } else if let Some(d) = existing_discard {
                parts.push(format!("discard='{}'", xml::escape(&d)));
            }
            let new_driver = format!("<driver {} />", parts.join(" "));
            let mut nb = replace_driver_element(&block, &new_driver);
            if let Some(ref b) = tune.bus {
                let (rewritten, _new_dev) =
                    set_target_bus_on_block(&nb, &tune.target, b, &used_others)?;
                nb = rewritten;
                bus_changed = true;
            }
            nb = set_disk_readonly_shareable(nb, tune.readonly, tune.shareable);
            orig_block = Some(block);
            new_block = Some(nb);
            break;
        }
    }
    let frag = new_block
        .ok_or_else(|| LibvirtError::NotFound(format!("No disk with target '{}'", tune.target)))?;
    let orig = orig_block.expect("paired with new_block");

    if bus_changed {
        let new_xml = replace_exact_once(&desc, &orig, &frag)?;
        Domain::define_xml(conn, &new_xml)
            .map_err(|e| LibvirtError::Operation(format!("update_disk_tune define_xml: {e}")))?;
        return Ok(());
    }

    let flags = get_domain_flags(&domain);
    match domain.update_device_flags(&frag, flags) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Fall back to redefine when live/config device update is unsupported.
            let new_xml = replace_exact_once(&desc, &orig, &frag)?;
            Domain::define_xml(conn, &new_xml).map_err(|e2| {
                LibvirtError::Operation(format!(
                    "update_disk_tune: update_device_flags failed ({e}); define_xml also failed: {e2}"
                ))
            })?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NicTuneRequest {
    pub mac_address: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
}

pub fn update_nic_tune(
    conn: &Connect,
    vm_name: &str,
    tune: &NicTuneRequest,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let mac = tune.mac_address.to_ascii_lowercase();
    let mut found: Option<String> = None;
    for block in split_blocks(&desc, "interface") {
        let m = xml::extract_attr(&block, "mac", "address")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if m == mac {
            found = Some(block);
            break;
        }
    }
    let block = found
        .ok_or_else(|| LibvirtError::NotFound(format!("No NIC with MAC '{}'", tune.mac_address)))?;

    // Build a minimal <interface> fragment (type, mac, source, model only) rather than
    // reusing the live block wholesale. update_device_flags() runs against BOTH live and
    // config when the domain is active (see get_domain_flags), and the live block carries
    // <alias>/<address> that are runtime-only — a NIC attached moments earlier (see the
    // identical fix in detach_interface() above) has these on the live side but not the
    // persistent (offline) definition, so matching the full block against config fails
    // with "device not found ... matching MAC address" even though live update succeeds.
    let iface_type =
        xml::extract_attr(&block, "interface", "type").unwrap_or_else(|| "network".to_string());
    let model = tune
        .model
        .clone()
        .or_else(|| xml::extract_attr(&block, "model", "type"));
    let network = tune
        .network
        .clone()
        .or_else(|| xml::extract_attr(&block, "source", "network"));

    let mut frag = format!(
        "<interface type='{}'>\n  <mac address='{}'/>",
        xml::escape(&iface_type),
        xml::escape(&mac)
    );
    if iface_type == "network" {
        let net = network.ok_or_else(|| {
            LibvirtError::Operation("interface has no <source network='...'/>".into())
        })?;
        frag.push_str(&format!("\n  <source network='{}'/>", xml::escape(&net)));
    }
    if let Some(model) = model {
        frag.push_str(&format!("\n  <model type='{}'/>", xml::escape(&model)));
    }
    frag.push_str("\n</interface>");

    let flags = get_domain_flags(&domain);
    domain
        .update_device_flags(&frag, flags)
        .map_err(|e| LibvirtError::Operation(format!("update_nic_tune: {e}")))?;
    Ok(())
}

const VIDEO_MODELS: &[&str] = &["virtio", "qxl", "vga", "bochs", "cirrus", "none"];

/// Set the primary `<video><model type='…'/></video>`.
///
/// QEMU often rejects `updateDeviceFlags` for persistent video changes
/// (`operationunsupported`). Fall back to replacing the video block via `define_xml`.
pub fn set_video_model(conn: &Connect, vm_name: &str, model: &str) -> Result<(), LibvirtError> {
    if !VIDEO_MODELS.contains(&model) {
        return Err(LibvirtError::Invalid(format!(
            "video model must be one of: {}",
            VIDEO_MODELS.join(", ")
        )));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let blocks = split_blocks(&desc, "video");
    let Some(first) = blocks.first() else {
        return Err(LibvirtError::NotFound("No <video> in domain".into()));
    };
    let mut nb = first.clone();
    let keep_primary = nb.contains("primary='yes'") || nb.contains("primary=\"yes\"");
    let primary_attr = if keep_primary { " primary='yes'" } else { "" };
    if let Some(start) = nb.find("<model") {
        if let Some(rest) = nb[start..].find("/>") {
            let end = start + rest + 2;
            nb.replace_range(
                start..end,
                &format!(
                    "<model type='{}' heads='1'{}/>",
                    xml::escape(model),
                    primary_attr
                ),
            );
        } else if let Some(rest) = nb[start..].find("</model>") {
            let end = start + rest + "</model>".len();
            nb.replace_range(
                start..end,
                &format!(
                    "<model type='{}' heads='1'{}></model>",
                    xml::escape(model),
                    primary_attr
                ),
            );
        }
    }
    let flags = get_domain_flags(&domain);
    if domain.update_device_flags(&nb, flags).is_ok() {
        return Ok(());
    }
    let new_xml = replace_exact_once(&desc, first, &nb)?;
    Domain::define_xml(conn, &new_xml)
        .map_err(|e| LibvirtError::Operation(format!("set_video_model define_xml: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_virtio_to_scsi_target() {
        assert_eq!(remap_target_dev("vda", "scsi"), "sda");
        assert_eq!(remap_target_dev("vdb", "sata"), "sdb");
        assert_eq!(remap_target_dev("sda", "virtio"), "vda");
    }

    #[test]
    fn bus_change_strips_pci_address_and_renames() {
        let block = r#"<disk type='file' device='disk'>
    <driver name='qemu' type='qcow2' cache='none'/>
    <source file='/var/lib/libvirt/images/a.qcow2'/>
    <target dev='vda' bus='virtio'/>
    <address type='pci' domain='0x0000' bus='0x00' slot='0x05' function='0x0'/>
  </disk>"#;
        let (nb, new_dev) = set_target_bus_on_block(block, "vda", "scsi", &[]).unwrap();
        assert_eq!(new_dev, "sda");
        assert!(nb.contains("dev='sda'"));
        assert!(nb.contains("bus='scsi'"));
        assert!(!nb.contains("<address"));
    }

    #[test]
    fn bus_change_skips_occupied_sda() {
        let block = r#"<disk type='file' device='disk'>
    <target dev='vda' bus='virtio'/>
  </disk>"#;
        let used = vec!["sda".into()];
        let (_nb, new_dev) = set_target_bus_on_block(block, "vda", "scsi", &used).unwrap();
        assert_eq!(new_dev, "sdb");
    }

    #[test]
    fn bus_restore_prefers_vda_when_free() {
        let block = r#"<disk type='file' device='disk'>
    <target dev='sdb' bus='scsi'/>
  </disk>"#;
        let used = vec!["sda".into()];
        let (_nb, new_dev) = set_target_bus_on_block(block, "sdb", "virtio", &used).unwrap();
        assert_eq!(new_dev, "vda");
    }
}

/// Throughput limits for one disk; `0` removes a limit (libvirt's meaning), `None` leaves it alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiskIoTune {
    pub target: String,
    #[serde(default)]
    pub read_iops: Option<u64>,
    #[serde(default)]
    pub write_iops: Option<u64>,
    #[serde(default)]
    pub read_bps: Option<u64>,
    #[serde(default)]
    pub write_bps: Option<u64>,
}

/// `virsh blkdeviotune` arguments (after the domain name) for `t`.
pub fn iotune_args(vm_name: &str, t: &DiskIoTune) -> Result<Vec<String>, LibvirtError> {
    crate::validate::validate_name(vm_name)?;
    let dev_ok = (2..=8).contains(&t.target.len())
        && t.target.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && t.target.starts_with(|c: char| c.is_ascii_lowercase());
    if !dev_ok {
        return Err(LibvirtError::Invalid(format!("bad disk target '{}'", t.target)));
    }
    let mut a = vec!["blkdeviotune".to_string(), vm_name.to_string(), t.target.clone()];
    for (flag, v) in [
        ("--read-iops-sec", t.read_iops),
        ("--write-iops-sec", t.write_iops),
        ("--read-bytes-sec", t.read_bps),
        ("--write-bytes-sec", t.write_bps),
    ] {
        if let Some(v) = v {
            a.push(flag.into());
            a.push(v.to_string());
        }
    }
    if a.len() == 3 {
        return Err(LibvirtError::Invalid("no limit given".into()));
    }
    a.push("--current".into());
    Ok(a)
}

/// Apply I/O limits to a disk of a defined domain (live if running, persistent config otherwise).
pub fn set_disk_iotune(vm_name: &str, t: &DiskIoTune) -> Result<(), LibvirtError> {
    let args = iotune_args(vm_name, t)?;
    let out = std::process::Command::new("virsh")
        .args(&args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh blkdeviotune: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(LibvirtError::Operation(format!(
            "virsh blkdeviotune: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod iotune_tests {
    use super::*;

    #[test]
    fn builds_args_for_given_limits_only() {
        let t = DiskIoTune { target: "vdb".into(), read_iops: Some(500), write_bps: Some(0), ..Default::default() };
        let a = iotune_args("web-1", &t).unwrap();
        assert_eq!(
            a,
            ["blkdeviotune", "web-1", "vdb", "--read-iops-sec", "500", "--write-bytes-sec", "0", "--current"]
        );
    }

    #[test]
    fn rejects_empty_and_bad_targets() {
        let none = DiskIoTune { target: "vdb".into(), ..Default::default() };
        assert!(iotune_args("web-1", &none).is_err());
        let bad = DiskIoTune { target: "vdb; rm".into(), read_iops: Some(1), ..Default::default() };
        assert!(iotune_args("web-1", &bad).is_err());
        let ok = DiskIoTune { target: "vdb".into(), read_iops: Some(1), ..Default::default() };
        assert!(iotune_args("bad name;", &ok).is_err());
    }
}
