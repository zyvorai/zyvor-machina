// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;
use virt::domain::Domain;

use super::domain::{lookup_domain, wait_until_absent_from_live};
use crate::state::AttachDiskRequest;
use crate::LibvirtError;

/// How long to wait for a hot-unplug to actually take effect on the live domain before
/// reporting it as still-pending. Real guests with a working driver typically release a
/// device within a few hundred ms of the request; this is generous without making a
/// synchronous HTTP call open-ended.
pub(crate) const DETACH_LIVE_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Outcome of a device detach that can require guest cooperation to complete live.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetachOutcome {
    /// True if the device was confirmed gone from the running domain's live XML within
    /// the wait window. False means the persistent config was updated (the device will be
    /// gone on next boot) but it's still present live — normally because the guest OS
    /// hasn't released it yet, or has no hot-unplug support at all (including a domain
    /// with no guest OS, e.g. a fresh/blank-disk VM).
    pub live_removed: bool,
}

pub fn get_domain_flags_pub(domain: &Domain) -> u32 {
    get_domain_flags(domain)
}

pub(crate) fn get_domain_flags(domain: &Domain) -> u32 {
    domain
        .get_info()
        .map(|info| {
            if info.state == 1
            /* VIR_DOMAIN_RUNNING */
            {
                virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG
            } else {
                virt::sys::VIR_DOMAIN_AFFECT_CONFIG
            }
        })
        .unwrap_or(virt::sys::VIR_DOMAIN_AFFECT_CONFIG)
}

const DISK_BUSES: &[&str] = &["virtio", "scsi", "sata", "ide"];

pub fn attach_disk(
    conn: &Connect,
    vm_name: &str,
    req: &AttachDiskRequest,
) -> Result<(), LibvirtError> {
    let source_path = std::path::Path::new(&req.source);
    if !source_path.is_absolute() {
        return Err(LibvirtError::Invalid(
            "Disk source path must be absolute".to_string(),
        ));
    }
    // Canonicalize to resolve symlinks and prevent path traversal
    let resolved = source_path
        .canonicalize()
        .map_err(|_| LibvirtError::Operation(format!("Disk source not found: {}", req.source)))?;
    if !resolved.is_file() {
        return Err(LibvirtError::Operation(format!(
            "Disk source is not a file: {}",
            req.source
        )));
    }
    // Confine to a configured pool so a caller can't attach an arbitrary host file/block
    // device into a guest. Check the CANONICAL path (symlinks already resolved).
    let resolved_str = resolved.to_string_lossy().into_owned();
    super::storage::assert_backup_source_within_pools(conn, &resolved_str)?;

    let bus = req.bus.trim();
    let bus = if bus.is_empty() { "virtio" } else { bus };
    if !DISK_BUSES.contains(&bus) {
        return Err(LibvirtError::Invalid(format!(
            "Invalid disk bus '{}'. Allowed: {}",
            bus,
            DISK_BUSES.join(", ")
        )));
    }
    let cache = req.cache.trim();
    let discard = req.discard.trim();
    let mut driver_attrs = vec![
        "name='qemu'".to_string(),
        format!("type='{}'", crate::xml::escape(&req.driver)),
    ];
    if !cache.is_empty() {
        driver_attrs.push(format!("cache='{}'", crate::xml::escape(cache)));
    }
    if !discard.is_empty() {
        driver_attrs.push(format!("discard='{}'", crate::xml::escape(discard)));
    }
    let driver_xml = format!("<driver {} />", driver_attrs.join(" "));
    let ro = if req.readonly { "\n  <readonly/>" } else { "" };
    let share = if req.shareable {
        " shareable='yes'"
    } else {
        ""
    };

    let domain = lookup_domain(conn, vm_name)?;

    let xml = format!(
        r#"<disk type='file' device='disk'{share}>
  {driver_xml}
  <source file='{source}'/>
  <target dev='{target}' bus='{bus}'/>{ro}
</disk>"#,
        share = share,
        driver_xml = driver_xml,
        source = crate::xml::escape(&resolved_str),
        target = crate::xml::escape(&req.target),
        bus = crate::xml::escape(bus),
        ro = ro,
    );

    let flags = get_domain_flags(&domain);
    domain
        .attach_device_flags(&xml, flags)
        .map_err(LibvirtError::map_op("Failed to attach disk"))?;
    Ok(())
}

pub fn detach_disk(
    conn: &Connect,
    vm_name: &str,
    target: &str,
) -> Result<DetachOutcome, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;

    // `detach_device_flags` matches the supplied XML against the domain's actual
    // device — a hardcoded `type='file'` here silently fails to match a
    // network-backed disk (e.g. an Atlas/RBD volume attached via
    // `agent::libvirt_ops::attach_disk`'s `type='network'` XML): the persistent
    // config sometimes still drops the entry while the LIVE domain keeps the
    // disk attached with no error surfaced, which is exactly the "detach
    // reported success but `virsh domblklist` still shows it" symptom this
    // fixes. Determine the real type from the domain's current XML instead of
    // assuming file-backed.
    let current_xml = domain.get_xml_desc(0).unwrap_or_default();
    let disk_type = disk_type_for_target(&current_xml, target);

    let xml = format!(
        r#"<disk type='{disk_type}' device='disk'>
  <target dev='{}'/>
</disk>"#,
        crate::xml::escape(target),
    );

    let flags = get_domain_flags(&domain);
    domain
        .detach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("Failed to detach disk '{target}': {e}")))?;

    // libvirt's synchronous return here only means the unplug request was queued — for a
    // running domain, actual removal is asynchronous and needs the guest to release the
    // device (see wait_until_absent_from_live's doc comment). Confirm within a bounded
    // window instead of blindly reporting success.
    let is_running = domain.is_active().unwrap_or(false);
    let live_removed = if is_running {
        wait_until_absent_from_live(&domain, DETACH_LIVE_WAIT, |live_xml| {
            !target_dev_present(live_xml, target)
        })
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

/// Does `<target dev='{target}' .../>` (either quote style) appear anywhere in `xml`?
/// The `type` of the disk whose target is `target` in a domain's XML, limited to the kinds libvirt
/// accepts in a detach (`file`, `block`, `network`, `volume`, `dir`); anything else, or no such disk,
/// is treated as `file`, as before. Matching the real type matters: a detach whose `type` differs
/// from the attached disk can report success while the live domain keeps the disk.
pub(crate) fn disk_type_for_target(xml: &str, target: &str) -> &'static str {
    for block in crate::xml::split_blocks(xml, "disk") {
        if crate::xml::extract_attr(&block, "target", "dev").as_deref() == Some(target) {
            return match crate::xml::extract_attr(&block, "disk", "type").as_deref() {
                Some("network") => "network",
                Some("block") => "block",
                Some("volume") => "volume",
                Some("dir") => "dir",
                _ => "file",
            };
        }
    }
    "file"
}

pub(crate) fn target_dev_present(xml: &str, target: &str) -> bool {
    xml.contains(&format!("target dev='{target}'"))
        || xml.contains(&format!("target dev=\"{target}\""))
}

/// Resize a block device attached to a VM (in GB).
/// Note: size_bytes = size_gb * 1024^3, max 10240 GB = ~11 TB, fits in u64.
pub fn resize_block_device(
    conn: &Connect,
    vm_name: &str,
    target: &str,
    size_gb: u64,
) -> Result<(), LibvirtError> {
    crate::validate::validate_disk_gb(size_gb)?;
    let domain = lookup_domain(conn, vm_name)?;
    let size_bytes = size_gb * 1024 * 1024 * 1024;
    domain.block_resize(target, size_bytes, 0).map_err(|e| {
        LibvirtError::Operation(format!(
            "Failed to resize disk '{target}' on '{}': {e}",
            vm_name
        ))
    })?;
    Ok(())
}

const ALLOWED_NIC_MODELS: &[&str] = &["virtio", "e1000", "e1000e", "rtl8139", "vmxnet3"];

/// Locally-administered, QEMU-prefixed MAC address for a newly attached NIC.
///
/// Attaching with `VIR_DOMAIN_AFFECT_LIVE | VIR_DOMAIN_AFFECT_CONFIG` applies the same
/// device XML to the live domain and the persistent config as two separate operations;
/// when the XML omits `<mac>`, libvirt auto-generates one independently for each,
/// so the live and offline definitions end up with *different* MACs. A later
/// `update_device_flags` (nic.tune) built from the live MAC then fails against the
/// config copy with "operation failed: no device matching mac address" even though the
/// live update succeeds. Generating the MAC ourselves keeps both copies identical.
fn random_nic_mac() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        rng.gen::<u8>(),
        rng.gen::<u8>(),
        rng.gen::<u8>()
    )
}

fn pci_slots_exhausted(err: &str) -> bool {
    let m = err.to_ascii_lowercase();
    m.contains("no more available pci slots") || m.contains("no more available pci slot")
}

/// Hot-add one spare `pcie-root-port` so q35 domains gain a free slot for the next
/// PCI device. Used when NIC (or other) attach fails with "No more available PCI slots"
/// on VMs that were defined without spare ports.
fn attach_spare_pcie_root_port(domain: &Domain, flags: u32) -> Result<(), LibvirtError> {
    let xml = "<controller type='pci' model='pcie-root-port'/>";
    domain
        .attach_device_flags(xml, flags)
        .map_err(LibvirtError::map_op(
            "Failed to add PCIe root port for hotplug",
        ))?;
    Ok(())
}

/// Attach a network interface to a VM.
pub fn attach_interface(
    conn: &Connect,
    vm_name: &str,
    network: &str,
    model: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(network)?;
    if !ALLOWED_NIC_MODELS.contains(&model) {
        return Err(LibvirtError::Invalid(format!(
            "Invalid NIC model '{}'. Allowed: {}",
            model,
            ALLOWED_NIC_MODELS.join(", ")
        )));
    }
    let domain = lookup_domain(conn, vm_name)?;

    let mac = random_nic_mac();
    let xml = format!(
        r#"<interface type='network'>
  <source network='{network}'/>
  <model type='{model}'/>
  <mac address='{mac}'/>
</interface>"#,
        network = crate::xml::escape(network),
        model = crate::xml::escape(model),
    );

    let flags = get_domain_flags(&domain);
    match domain.attach_device_flags(&xml, flags) {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if pci_slots_exhausted(&msg) {
                // Existing q35 VMs often lack spare root ports; add one and retry.
                attach_spare_pcie_root_port(&domain, flags)?;
                domain
                    .attach_device_flags(&xml, flags)
                    .map_err(LibvirtError::map_op("Failed to attach network interface"))?;
                Ok(())
            } else {
                Err(LibvirtError::map_op("Failed to attach network interface")(
                    e,
                ))
            }
        }
    }
}

/// Detach a network interface from a VM by MAC address.
pub fn detach_interface(
    conn: &Connect,
    vm_name: &str,
    mac: &str,
) -> Result<DetachOutcome, LibvirtError> {
    // Validate MAC address format (xx:xx:xx:xx:xx:xx)
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6
        || !parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(LibvirtError::Invalid(format!(
            "Invalid MAC address format: '{mac}'"
        )));
    }

    let domain = lookup_domain(conn, vm_name)?;

    // Locate the interface's <source network='...'/> (and, if present, its model) from the
    // running domain — libvirt's schema requires <source> for type='network' interfaces, so
    // passing just <mac address='...'/> fails XML validation. We deliberately do NOT reuse the
    // full live <interface> block wholesale: it also carries <alias name='netN'/> and
    // <address type='pci' .../>, which are runtime-only — never written to the domain's
    // persistent (offline) definition. detach_device_flags() runs against BOTH live and config
    // when the domain is active (see get_domain_flags), and matching an XML that includes
    // <alias>/<address> against the config side (which has neither) fails with "device not
    // found ... matching MAC address '...' and alias 'netN'" even though the interface is
    // right there. A minimal, source/model-only XML matches on both sides.
    let domain_xml = domain.get_xml_desc(0).map_err(LibvirtError::map_op(
        "Failed to get domain XML for NIC detach",
    ))?;
    let iface_block = extract_interface_xml_by_mac(&domain_xml, mac).ok_or_else(|| {
        LibvirtError::NotFound(format!(
            "Interface with MAC '{mac}' not found in domain XML"
        ))
    })?;
    let network = extract_attr(&iface_block, "source", "network").ok_or_else(|| {
        LibvirtError::Operation(format!(
            "Interface with MAC '{mac}' has no <source network='...'/> — cannot build detach XML"
        ))
    })?;
    let model = extract_attr(&iface_block, "model", "type");

    let mut xml = format!(
        "<interface type='network'>\n  <mac address='{}'/>\n  <source network='{}'/>",
        crate::xml::escape(mac),
        crate::xml::escape(&network),
    );
    if let Some(model) = &model {
        xml.push_str(&format!(
            "\n  <model type='{}'/>",
            crate::xml::escape(model)
        ));
    }
    xml.push_str("\n</interface>");

    let flags = get_domain_flags(&domain);
    domain
        .detach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("Failed to detach interface '{mac}': {e}")))?;

    // Same async-completion caveat as detach_disk: a successful call only means the unplug
    // request was queued, not that the guest has released the interface yet.
    let is_running = domain.is_active().unwrap_or(false);
    let live_removed = if is_running {
        wait_until_absent_from_live(&domain, DETACH_LIVE_WAIT, |live_xml| {
            extract_interface_xml_by_mac(live_xml, mac).is_none()
        })
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

/// Extract the `<interface>…</interface>` block containing the given MAC address.
fn extract_interface_xml_by_mac(domain_xml: &str, mac: &str) -> Option<String> {
    let mac_lower = mac.to_ascii_lowercase();
    let lower = domain_xml.to_ascii_lowercase();
    // Try both single-quote and double-quote attribute styles
    for q in [('\'', '\''), ('"', '"')] {
        let needle = format!("address={}{}{}", q.0, mac_lower, q.1);
        if let Some(mac_pos) = lower.find(&needle) {
            // Walk backwards to the opening <interface tag
            if let Some(iface_off) = lower[..mac_pos].rfind("<interface") {
                // Walk forwards to the closing </interface>
                let suffix = &lower[iface_off..];
                if let Some(close_off) = suffix.find("</interface>") {
                    let end = iface_off + close_off + "</interface>".len();
                    return Some(domain_xml[iface_off..end].to_string());
                }
            }
        }
    }
    None
}

/// Find `<tag ... attr='value' .../>` within `xml` and return `value` (single or double quotes).
fn extract_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let tag_start = xml.find(&format!("<{tag} "))?;
    let tag_end = xml[tag_start..].find('>').map(|i| tag_start + i)?;
    let tag_block = &xml[tag_start..tag_end];
    for q in ['\'', '"'] {
        let needle = format!("{attr}={q}");
        if let Some(pos) = tag_block.find(&needle) {
            let value_start = pos + needle.len();
            let value_end = tag_block[value_start..].find(q)? + value_start;
            return Some(tag_block[value_start..value_end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::disk_type_for_target;

    const XML: &str = "<domain><devices>\
<disk type='file' device='disk'><driver name='qemu' type='qcow2'/><source file='/var/lib/libvirt/images/a.qcow2'/><target dev='vda' bus='virtio'/></disk>\
<disk type='network' device='disk'><source protocol='rbd' name='pool/vol'/><target dev='vdb' bus='virtio'/></disk>\
<disk type='block' device='disk'><driver name='qemu' type='raw'/><source dev='/dev/disk/by-path/ip-10.0.0.20:3260-iscsi-iqn.x:d-lun-1'/><target dev='vdc' bus='virtio'/></disk>\
<disk type='file' device='cdrom'><source file='/iso/x.iso'/><target dev='sda' bus='sata'/></disk>\
</devices></domain>";

    #[test]
    fn detach_uses_the_real_disk_type() {
        assert_eq!(disk_type_for_target(XML, "vda"), "file");
        assert_eq!(disk_type_for_target(XML, "vdb"), "network");
        assert_eq!(disk_type_for_target(XML, "vdc"), "block");
        assert_eq!(disk_type_for_target(XML, "sda"), "file");
    }

    #[test]
    fn an_unknown_target_or_odd_type_falls_back_to_file() {
        assert_eq!(disk_type_for_target(XML, "vdz"), "file");
        let odd = "<domain><devices><disk type='nvme' device='disk'><target dev='vdd'/></disk></devices></domain>";
        assert_eq!(disk_type_for_target(odd, "vdd"), "file");
    }
}
