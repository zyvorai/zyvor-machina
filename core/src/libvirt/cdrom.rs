// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use super::device::{target_dev_present, DetachOutcome, DETACH_LIVE_WAIT};
use super::domain::{lookup_domain, wait_until_absent_from_live};
use crate::LibvirtError;

#[allow(clippy::too_many_lines)]
/// Insert (or swap) CD-ROM media.
///
/// `target` may be empty, in which case a free target is chosen for the domain's
/// bus — the caller almost never has a reason to care which one it is.
pub fn insert_cdrom(
    conn: &Connect,
    name: &str,
    iso_path: &str,
    target: &str,
) -> Result<CdromInsertOutcome, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let conn_ref = conn;

    // Validate ISO path: must be absolute and resolve to a real path (no symlink escapes)
    let path = std::path::Path::new(iso_path);
    if !path.is_absolute() {
        return Err(LibvirtError::Invalid(
            "ISO path must be absolute".to_string(),
        ));
    }
    if !path.exists() {
        return Err(LibvirtError::Operation(format!(
            "ISO file not found: {iso_path}"
        )));
    }
    let resolved = path
        .canonicalize()
        .map_err(|_| LibvirtError::Operation(format!("Failed to resolve ISO path: {iso_path}")))?;
    if !resolved.is_file() {
        return Err(LibvirtError::Operation(format!(
            "ISO path is not a file: {iso_path}"
        )));
    }
    // Unlike the browser-upload path (confined to `iso_upload_dir` by
    // construction), this endpoint takes a caller-supplied absolute path
    // directly. Without this check any actor with VM-write access could mount
    // an arbitrary host file — `/etc/shadow`, an SSH private key, anything
    // qemu's user can read — read-only into a guest as "CD-ROM media". Same
    // allow-list used for disk-image browse/delete and backup restore sources.
    super::storage::assert_backup_source_within_pools(conn_ref, iso_path)?;

    let flags = get_update_flags(&domain);

    let vm_xml = domain.get_xml_desc(0).unwrap_or_default();
    let default_bus = detect_best_bus(&vm_xml);
    // An empty target means "wherever it fits" — see pick_free_cdrom_target.
    //
    // The picker must see the persistent config as well as the live domain: a
    // SATA CD-ROM attached to a running guest lands in config only, so the live
    // XML alone under-reports what is taken and we would pick a target libvirt
    // then rejects with "target sdb already exists".
    let combined_xml = super::domain::domain_xml_live_and_config(&domain);
    let target: String = if target.trim().is_empty() {
        pick_free_cdrom_target(&combined_xml, default_bus)?
    } else {
        target.trim().to_string()
    };
    let target = target.as_str();

    // Check if a cdrom device already exists at this target. Must use the
    // combined live+config view, same as the target-picker above: a SATA
    // CD-ROM staged on a running guest lands in config only, so checking the
    // live XML alone would report "no CD-ROM here" for a drive that
    // demonstrably exists — sending this down the "attach new device" branch,
    // which libvirt then rejects as already existing, instead of updating it.
    let (has_cdrom, existing_bus) = find_cdrom_device(&combined_xml, target);

    if has_cdrom {
        // Update existing cdrom device — use same bus type
        let bus = existing_bus.unwrap_or_else(|| "sata".to_string());
        let xml = format!(
            r#"<disk type='file' device='cdrom'>
  <driver name='qemu' type='raw'/>
  <source file='{}'/>
  <target dev='{}' bus='{}'/>
  <readonly/>
</disk>"#,
            crate::xml::escape(iso_path),
            crate::xml::escape(target),
            crate::xml::escape(&bus),
        );
        domain
            .update_device_flags(&xml, flags)
            .map_err(|e| LibvirtError::Operation(format!("Failed to update CD-ROM: {e}")))?;
        let live = flags & virt::sys::VIR_DOMAIN_AFFECT_LIVE != 0;
        Ok(CdromInsertOutcome {
            target: target.to_string(),
            bus,
            live,
            requires_restart: false,
        })
    } else {
        // No cdrom exists — attach new device. Detect bus type from VM.
        let bus = detect_best_bus(&vm_xml);
        let xml = format!(
            r#"<disk type='file' device='cdrom'>
  <driver name='qemu' type='raw'/>
  <source file='{}'/>
  <target dev='{}' bus='{}'/>
  <readonly/>
</disk>"#,
            crate::xml::escape(iso_path),
            crate::xml::escape(target),
            bus,
        );

        // For shutoff VMs, we can redefine with the cdrom; for running VMs, use attach
        let info = domain.get_info().ok();
        let is_running = info.as_ref().map(|i| i.state == 1).unwrap_or(false);

        if is_running {
            // Try live+config first. SATA can't be hotplugged — fall back to config-only
            // so the drive appears on next boot without failing the whole operation.
            let live_result = domain.attach_device_flags(
                &xml,
                virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG,
            );
            if live_result.is_err() {
                domain
                    .attach_device_flags(&xml, virt::sys::VIR_DOMAIN_AFFECT_CONFIG)
                    .map_err(|e| {
                        LibvirtError::Operation(format!(
                            "Failed to attach CD-ROM (stop the VM to hot-attach SATA): {e}"
                        ))
                    })?;
                // Staged only — the guest cannot see this media until it reboots.
                return Ok(CdromInsertOutcome {
                    target: target.to_string(),
                    bus: bus.to_string(),
                    live: false,
                    requires_restart: true,
                });
            }
            Ok(CdromInsertOutcome {
                target: target.to_string(),
                bus: bus.to_string(),
                live: true,
                requires_restart: false,
            })
        } else {
            // For shutoff VMs — insert cdrom into XML definition
            let new_xml = insert_cdrom_into_xml(&vm_xml, &xml);
            virt::domain::Domain::define_xml(conn_ref, &new_xml).map_err(|e| {
                LibvirtError::Operation(format!("Failed to define VM with CD-ROM: {e}"))
            })?;
            Ok(CdromInsertOutcome {
                target: target.to_string(),
                bus: bus.to_string(),
                // A stopped guest sees the media the moment it starts.
                live: false,
                requires_restart: false,
            })
        }
    }
}

pub fn eject_cdrom(conn: &Connect, name: &str, target: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    // Both views: a CD-ROM staged on a running guest is config-only, and looking
    // at the live XML alone reported "no CD-ROM at target" for a drive that
    // demonstrably existed.
    let vm_xml = super::domain::domain_xml_live_and_config(&domain);
    let (has_cdrom, existing_bus) = find_cdrom_device(&vm_xml, target);

    if !has_cdrom {
        return Err(LibvirtError::NotFound(format!(
            "No CD-ROM device at target '{target}'"
        )));
    }

    let bus = existing_bus.unwrap_or_else(|| "sata".to_string());
    let xml = format!(
        r#"<disk type='file' device='cdrom'>
  <target dev='{}' bus='{}'/>
  <readonly/>
</disk>"#,
        crate::xml::escape(target),
        crate::xml::escape(&bus),
    );

    let flags = get_update_flags(&domain);
    // Same LIVE+CONFIG-then-CONFIG-only fallback as `detach_cdrom`: a drive
    // staged only in the persistent config (SATA on a running guest) has
    // nothing to eject live, so the combined-flags call fails outright without
    // this — the caller could stage an insert but never cancel it before reboot.
    if domain.update_device_flags(&xml, flags).is_err() {
        domain
            .update_device_flags(&xml, virt::sys::VIR_DOMAIN_AFFECT_CONFIG)
            .map_err(|e| LibvirtError::Operation(format!("Failed to eject CD-ROM: {e}")))?;
    }

    Ok(())
}

/// Remove the CD-ROM *drive* entirely, not just its media.
///
/// `eject_cdrom` only blanks the media and leaves the device behind, so a
/// mistakenly-added drive could previously only be removed with `virsh
/// detach-disk` on the hypervisor.
pub fn detach_cdrom(
    conn: &Connect,
    name: &str,
    target: &str,
) -> Result<DetachOutcome, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let vm_xml = super::domain::domain_xml_live_and_config(&domain);
    let (has_cdrom, existing_bus) = find_cdrom_device(&vm_xml, target);
    if !has_cdrom {
        return Err(LibvirtError::NotFound(format!(
            "No CD-ROM device at target '{target}'"
        )));
    }
    let bus = existing_bus.unwrap_or_else(|| "sata".to_string());
    let xml = format!(
        r#"<disk type='file' device='cdrom'>
  <target dev='{}' bus='{}'/>
  <readonly/>
</disk>"#,
        crate::xml::escape(target),
        crate::xml::escape(&bus),
    );
    // Live+config where possible; SATA cannot hot-detach, so fall back to config
    // so the drive is gone on next boot rather than failing outright.
    let both = virt::sys::VIR_DOMAIN_AFFECT_LIVE | virt::sys::VIR_DOMAIN_AFFECT_CONFIG;
    let live_attempted = domain.detach_device_flags(&xml, both).is_ok();
    if !live_attempted {
        domain
            .detach_device_flags(&xml, virt::sys::VIR_DOMAIN_AFFECT_CONFIG)
            .map_err(|e| {
                LibvirtError::Operation(format!("Failed to detach CD-ROM at {target}: {e}"))
            })?;
    }

    // As with detach_disk: libvirt's synchronous return only means the unplug
    // request was queued. A drive that fell back to CONFIG-only never had a
    // live change attempted; one that took the combined LIVE|CONFIG path still
    // needs the guest to actually release it before it's really gone. Confirm
    // within a bounded window instead of blindly reporting success.
    let live_removed = if !live_attempted {
        false
    } else if domain.is_active().unwrap_or(false) {
        wait_until_absent_from_live(&domain, DETACH_LIVE_WAIT, |live_xml| {
            !target_dev_present(live_xml, target)
        })
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

/// Names of VMs that currently have `iso_path` mounted as CD-ROM media,
/// paired with whether each is running.
///
/// Overwriting an ISO file in place (browser re-upload / re-download with
/// `overwrite=true`) swaps the directory entry via `rename`, but QEMU keeps
/// its own open file descriptor to the *old* inode — a running guest with
/// this media mounted keeps reading the old bytes until the drive is
/// ejected and reinserted, while every other API consumer already sees the
/// new file. This exists so callers can warn about that instead of the
/// caller finding out by getting stale data from the guest.
pub fn vms_with_iso_mounted(conn: &Connect, iso_path: &str) -> Vec<(String, bool)> {
    let Ok(domains) = conn.list_all_domains(0) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for domain in domains {
        let Ok(name) = domain.get_name() else {
            continue;
        };
        let xml = super::domain::domain_xml_live_and_config(&domain);
        let mounted = crate::xml::split_blocks(&xml, "disk").iter().any(|block| {
            crate::xml::extract_attr(block, "disk", "device").as_deref() == Some("cdrom")
                && crate::xml::extract_attr(block, "source", "file").as_deref() == Some(iso_path)
        });
        if mounted {
            let running = domain.get_info().map(|i| i.state == 1).unwrap_or(false);
            hits.push((name, running));
        }
    }
    hits
}

/// Find a cdrom device at the given target, return (exists, bus_type).
fn find_cdrom_device(xml: &str, target: &str) -> (bool, Option<String>) {
    for block in crate::xml::split_blocks(xml, "disk") {
        let device = crate::xml::extract_attr(&block, "disk", "device").unwrap_or_default();
        if device == "cdrom" {
            let dev = crate::xml::extract_attr(&block, "target", "dev").unwrap_or_default();
            if dev == target || target.is_empty() {
                let bus = crate::xml::extract_attr(&block, "target", "bus");
                return (true, bus);
            }
        }
    }
    (false, None)
}

/// Detect the best bus type for a new cdrom based on VM's existing controllers.
/// All target names already claimed by a disk or CD-ROM in this domain.
fn used_targets(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("target dev=") {
        rest = &rest[i + "target dev=".len()..];
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '\'' && quote != '"' {
            continue;
        }
        if let Some(end) = rest[1..].find(quote) {
            out.push(rest[1..=end].to_string());
            rest = &rest[end + 1..];
        }
    }
    out
}

/// Pick a free CD-ROM target for `bus`, avoiding every device already attached.
///
/// The old fixed default of `sda` collided with the root disk on essentially
/// every SATA guest — the common case for Windows — and libvirt rejected the
/// attach with "target sda already exists".
pub fn pick_free_cdrom_target(vm_xml: &str, bus: &str) -> Result<String, LibvirtError> {
    let prefix = match bus {
        "ide" => "hd",
        "virtio" => "vd",
        // sata and scsi both present as sd*
        _ => "sd",
    };
    let used = used_targets(vm_xml);
    for suffix in b'a'..=b'z' {
        let candidate = format!("{prefix}{}", suffix as char);
        if !used.iter().any(|u| u == &candidate) {
            return Ok(candidate);
        }
    }
    Err(LibvirtError::Operation(format!(
        "no free {prefix}* target available for a CD-ROM on this VM"
    )))
}

/// Where the media actually landed. A SATA CD-ROM cannot be hot-attached, so a
/// running guest may only get it on next boot — the caller must be able to say
/// so rather than reporting a bare success the operator cannot see in the guest.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CdromInsertOutcome {
    pub target: String,
    pub bus: String,
    /// True when the media is visible to the running guest right now.
    pub live: bool,
    /// True when the guest must be restarted before the media appears.
    pub requires_restart: bool,
}

fn detect_best_bus(xml: &str) -> &'static str {
    // Check for SATA controller
    if xml.contains("type='sata'") || xml.contains("type=\"sata\"") {
        return "sata";
    }
    // Check for SCSI controller
    if xml.contains("type='scsi'") || xml.contains("type=\"scsi\"") {
        return "scsi";
    }
    // Check for IDE controller (legacy)
    if xml.contains("type='ide'") || xml.contains("type=\"ide\"") {
        return "ide";
    }
    // Default: SATA works on q35 machines (most modern VMs)
    "sata"
}

/// Insert a cdrom disk XML into the VM's devices section.
fn insert_cdrom_into_xml(vm_xml: &str, cdrom_xml: &str) -> String {
    // Insert before </devices>
    if let Some(pos) = vm_xml.rfind("</devices>") {
        let mut result = vm_xml[..pos].to_string();
        result.push_str("    ");
        result.push_str(cdrom_xml);
        result.push('\n');
        result.push_str("  ");
        result.push_str(&vm_xml[pos..]);
        result
    } else {
        vm_xml.to_string()
    }
}

fn get_update_flags(domain: &virt::domain::Domain) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS_SATA_VM: &str = r#"<domain>
      <devices>
        <disk type='file' device='disk'>
          <source file='/var/lib/libvirt/images/win10.qcow2'/>
          <target dev='sda' bus='sata'/>
        </disk>
        <controller type='sata' index='0'/>
        <interface type='network'><target dev='vnet3'/></interface>
      </devices>
    </domain>"#;

    #[test]
    fn skips_the_root_disk_instead_of_colliding_on_sda() {
        // The bug: a fixed "sda" default made libvirt reject every attach with
        // "target sda already exists" on SATA guests — i.e. most Windows VMs.
        let t = pick_free_cdrom_target(WINDOWS_SATA_VM, "sata").unwrap();
        assert_eq!(t, "sdb");
    }

    #[test]
    fn skips_every_target_already_in_use() {
        let xml = r#"<domain><devices>
            <disk device='disk'><target dev='sda' bus='sata'/></disk>
            <disk device='cdrom'><target dev='sdb' bus='sata'/></disk>
            <disk device='cdrom'><target dev='sdc' bus='sata'/></disk>
        </devices></domain>"#;
        assert_eq!(pick_free_cdrom_target(xml, "sata").unwrap(), "sdd");
    }

    #[test]
    fn uses_the_right_prefix_per_bus() {
        let empty = "<domain><devices/></domain>";
        assert_eq!(pick_free_cdrom_target(empty, "sata").unwrap(), "sda");
        assert_eq!(pick_free_cdrom_target(empty, "ide").unwrap(), "hda");
        assert_eq!(pick_free_cdrom_target(empty, "virtio").unwrap(), "vda");
        // The NIC's <target dev='vnet3'/> must not be mistaken for a disk target.
        assert_eq!(
            pick_free_cdrom_target(WINDOWS_SATA_VM, "virtio").unwrap(),
            "vda"
        );
    }

    #[test]
    fn used_targets_reads_both_quote_styles() {
        let xml = r#"<disk><target dev='sda'/></disk><disk><target dev="sdb"/></disk>"#;
        let used = used_targets(xml);
        assert!(used.contains(&"sda".to_string()));
        assert!(used.contains(&"sdb".to_string()));
    }
}

#[cfg(test)]
mod staged_device_tests {
    use super::*;

    #[test]
    fn avoids_a_target_that_exists_only_in_the_persistent_config() {
        // Live XML shows just the root disk, because a SATA CD-ROM attached to a
        // running guest lands in config only. Picking from the live view alone
        // chose sdb — which libvirt then rejected as already existing.
        let live = r#"<domain><devices>
            <disk device='disk'><target dev='sda' bus='sata'/></disk>
        </devices></domain>"#;
        let inactive = r#"<domain><devices>
            <disk device='disk'><target dev='sda' bus='sata'/></disk>
            <disk device='cdrom'><target dev='sdb' bus='sata'/></disk>
        </devices></domain>"#;
        assert_eq!(pick_free_cdrom_target(live, "sata").unwrap(), "sdb");
        let combined = format!("{live}\n{inactive}");
        assert_eq!(pick_free_cdrom_target(&combined, "sata").unwrap(), "sdc");
    }
}
