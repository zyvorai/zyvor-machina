// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use tracing::warn;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::sys;

use crate::libvirt::guest_agent::guest_ipv4_from_virsh;
use crate::state::{DiskInfo, FilesystemInfo, InterfaceInfo, VmDetails, VmInfo};
use crate::xml;
use crate::LibvirtError;

// libvirt domain state constants
const VIR_DOMAIN_NOSTATE: u32 = 0;
const VIR_DOMAIN_RUNNING: u32 = 1;
const VIR_DOMAIN_BLOCKED: u32 = 2;
const VIR_DOMAIN_PAUSED: u32 = 3;
const VIR_DOMAIN_SHUTDOWN: u32 = 4;
const VIR_DOMAIN_SHUTOFF: u32 = 5;
const VIR_DOMAIN_CRASHED: u32 = 6;
const VIR_DOMAIN_PMSUSPENDED: u32 = 7;

pub(crate) fn state_to_string(state: u32) -> String {
    match state {
        VIR_DOMAIN_NOSTATE => "no state".to_string(),
        VIR_DOMAIN_RUNNING => "running".to_string(),
        VIR_DOMAIN_BLOCKED => "blocked".to_string(),
        VIR_DOMAIN_PAUSED => "paused".to_string(),
        VIR_DOMAIN_SHUTDOWN => "shutting down".to_string(),
        VIR_DOMAIN_SHUTOFF => "shutoff".to_string(),
        VIR_DOMAIN_CRASHED => "crashed".to_string(),
        VIR_DOMAIN_PMSUSPENDED => "suspended".to_string(),
        _ => format!("unknown ({state})"),
    }
}

pub fn lookup_domain(conn: &Connect, name: &str) -> Result<Domain, LibvirtError> {
    Domain::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("VM '{name}' not found: {e}")))
}

/// Live XML concatenated with the persistent (inactive) definition.
///
/// Any device that cannot be hot-plugged — a SATA CD-ROM, a virtio-serial
/// controller — is attached to the persistent config only while the guest runs,
/// so it is absent from the live XML. Code that asks "does this device exist?"
/// must consult both, or it will fail to find a device it just added and then
/// try to add it again. Use this for existence checks, never for parsing a
/// single authoritative value.
pub fn domain_xml_live_and_config(domain: &Domain) -> String {
    let live = domain.get_xml_desc(0).unwrap_or_default();
    let inactive = domain
        .get_xml_desc(virt::sys::VIR_DOMAIN_XML_INACTIVE)
        .unwrap_or_default();
    if inactive.is_empty() || inactive == live {
        live
    } else {
        format!("{live}\n{inactive}")
    }
}

/// Poll the domain's LIVE xml (re-fetched each iteration) until `is_gone` reports the
/// device has actually left it, or `timeout` elapses.
///
/// `detach_device_flags()` returning `Ok` only means libvirt accepted and queued the
/// unplug request — for hot-pluggable devices (disk, NIC, PCI hostdev, TPM, vsock, ...)
/// the request is asynchronous and needs the GUEST OS to release the device before it
/// actually disappears from the live domain. A domain with no guest OS (or one whose
/// driver doesn't support hot-unplug) never acknowledges it, so the device stays
/// live-resident indefinitely even though the call "succeeded". Blindly returning success
/// in that case is actively misleading — the caller (and anything built on top, like a
/// snapshot taken afterward) reasonably believes the device is gone when it is not.
///
/// This does NOT change the outcome of the detach call itself (the persistent config is
/// already updated by the time this runs) — it only gives callers an honest signal about
/// whether the LIVE domain converged within a short, bounded wait, so they can report
/// "removed" vs. "removal pending — guest hasn't released the device" instead of a flat,
/// sometimes-false "done".
pub fn wait_until_absent_from_live<F>(
    domain: &Domain,
    timeout: std::time::Duration,
    mut is_gone: F,
) -> bool
where
    F: FnMut(&str) -> bool,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let live_xml = domain.get_xml_desc(0).unwrap_or_default();
        if is_gone(&live_xml) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Generic bounded poll: retry `check` every 250ms until it returns true or `timeout`
/// elapses. Domain-agnostic — the caller's closure captures whatever it needs to
/// re-check (a fresh `get_info()`, `get_xml_desc()`, etc.).
pub fn poll_until<F>(timeout: std::time::Duration, mut check: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if check() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn first_guest_ipv4(_conn: &Connect, name: &str) -> Option<String> {
    // Avoid libvirt FFI interface_addresses — qemu driver can SIGSEGV on legacy guests.
    guest_ipv4_from_virsh(name)
}

pub fn list_vms(conn: &Connect) -> Result<Vec<VmInfo>, LibvirtError> {
    let domains = conn
        .list_all_domains(0)
        .map_err(LibvirtError::map_op("Failed to list domains"))?;

    let mut vms = Vec::new();
    for domain in domains {
        let name = domain
            .get_name()
            .map_err(LibvirtError::map_op("Failed to get domain name"))?;

        let info = domain
            .get_info()
            .map_err(LibvirtError::map_op("Failed to get domain info"))?;

        let state = state_to_string(info.state);
        let guest_ips = if state == "running" {
            super::guest_agent::guest_addresses_from_virsh(&name)
        } else {
            Vec::new()
        };
        vms.push(VmInfo {
            name,
            state,
            vcpus: info.nr_virt_cpu,
            memory_mb: info.memory / 1024,
            libvirt_connection: None,
            guest_ip: guest_ips.iter().find(|a| !a.contains(':')).cloned(),
            guest_ips,
            backend: None,
            fluxvm_backend: None,
        });
    }

    Ok(vms)
}

pub fn get_vm_details(conn: &Connect, name: &str) -> Result<VmDetails, LibvirtError> {
    let domain = lookup_domain(conn, name)?;

    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get domain info"))?;

    let uuid = domain
        .get_uuid_string()
        .map_err(LibvirtError::map_op("Failed to get UUID"))?;

    let xml_str = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))?;

    let autostart = domain.get_autostart().unwrap_or(false);
    let persistent = domain.is_persistent().unwrap_or(false);

    let (os_type, arch) = parse_os_info(&xml_str);
    let mut interfaces = parse_interfaces(&xml_str);
    enrich_interface_ips(conn, name, &mut interfaces);
    let mut disks = parse_disks(&xml_str);
    enrich_disk_block_info(&domain, &mut disks);
    let filesystems = parse_filesystems(&xml_str);
    let guest_ip = first_guest_ipv4(conn, name);

    Ok(VmDetails {
        name: name.to_string(),
        uuid,
        state: state_to_string(info.state),
        vcpus: info.nr_virt_cpu,
        memory_mb: info.memory / 1024,
        os_type,
        arch,
        autostart,
        persistent,
        interfaces,
        disks,
        filesystems,
        libvirt_connection: None,
        guest_ip,
        backend: None,
        fluxvm_backend: None,
    })
}

fn parse_filesystems(xml_str: &str) -> Vec<FilesystemInfo> {
    let mut out = Vec::new();
    for block in crate::xml::split_blocks(xml_str, "filesystem") {
        // Only surface the common host-dir share case.
        let fstype = crate::xml::extract_attr(&block, "filesystem", "type").unwrap_or_default();
        if fstype != "mount" {
            continue;
        }
        let src = crate::xml::extract_attr(&block, "source", "dir").unwrap_or_default();
        let tag = crate::xml::extract_attr(&block, "target", "dir").unwrap_or_default();
        if src.is_empty() && tag.is_empty() {
            continue;
        }
        let accessmode =
            crate::xml::extract_attr(&block, "filesystem", "accessmode").unwrap_or_default();
        let driver = crate::xml::extract_attr(&block, "driver", "type").unwrap_or_default();
        let xattr = crate::xml::extract_attr(&block, "binary", "xattr")
            .map(|v| v.eq_ignore_ascii_case("on") || v == "1" || v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);
        out.push(FilesystemInfo {
            source: src,
            mount_tag: tag,
            driver,
            accessmode,
            xattr,
        });
    }
    out
}

pub fn get_vm_xml(conn: &Connect, name: &str) -> Result<String, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get XML"))
}

fn domain_action(
    conn: &Connect,
    name: &str,
    action: &str,
    f: impl FnOnce(&Domain) -> Result<(), virt::error::Error>,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    f(&domain).map_err(|e| LibvirtError::Operation(format!("Failed to {action} VM '{name}': {e}")))
}

/// File-backed `disk` sources in domain XML that are not regular files on the host (missing or not a file).
pub fn missing_file_disk_paths(conn: &Connect, name: &str) -> Result<Vec<String>, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let missing: Vec<String> = collect_disk_paths(&xml)
        .into_iter()
        .filter(|p| !Path::new(p).is_file())
        .collect();
    Ok(missing)
}

pub fn start_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let xml = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;

    let missing: Vec<String> = collect_disk_paths(&xml)
        .into_iter()
        .filter(|p| !Path::new(p).is_file())
        .collect();
    if !missing.is_empty() {
        return Err(LibvirtError::Invalid(format!(
            "Cannot start VM '{name}': disk image file(s) missing on host (create the image or fix paths before start): {}",
            missing.join(", ")
        )));
    }

    for net_name in crate::libvirt::net_xml::network_names_from_domain_xml(&xml) {
        crate::libvirt::network::ensure_network_active(conn, &net_name)?;
    }

    domain
        .create()
        .map(|_| ())
        .map_err(|e| LibvirtError::Operation(format!("Failed to start VM '{name}': {e}")))
}

pub fn stop_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    domain_action(conn, name, "stop", |d| d.destroy().map(|_| ()))
}

pub fn shutdown_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    shutdown_vm_mode(conn, name, PowerMode::Default)
}

pub fn reboot_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    reboot_vm_mode(conn, name, PowerMode::Default)
}

/// How libvirt should ask the guest to shut down or reboot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PowerMode {
    #[default]
    Default,
    /// `virsh shutdown --mode=agent` / guest-initiated clean shutdown.
    GuestAgent,
}

impl PowerMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "agent" | "guest" | "guest_agent" => Self::GuestAgent,
            _ => Self::Default,
        }
    }
}

pub fn shutdown_vm_mode(conn: &Connect, name: &str, mode: PowerMode) -> Result<(), LibvirtError> {
    domain_action(conn, name, "shutdown", |d| match mode {
        PowerMode::Default => d.shutdown().map(|_| ()),
        PowerMode::GuestAgent => d
            .shutdown_flags(sys::VIR_DOMAIN_SHUTDOWN_GUEST_AGENT)
            .map(|_| ()),
    })
}

pub fn reboot_vm_mode(conn: &Connect, name: &str, mode: PowerMode) -> Result<(), LibvirtError> {
    domain_action(conn, name, "reboot", |d| {
        let flags = match mode {
            PowerMode::Default => sys::VIR_DOMAIN_REBOOT_DEFAULT,
            PowerMode::GuestAgent => sys::VIR_DOMAIN_REBOOT_GUEST_AGENT,
        };
        d.reboot(flags).map(|_| ())
    })
}

/// Force reboot (libvirt `Reset`) — immediate reset without guest cooperation.
pub fn reset_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    domain_action(conn, name, "reset", |d| d.reset().map(|_| ()))
}

pub fn pause_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    domain_action(conn, name, "pause", |d| d.suspend().map(|_| ()))
}

pub fn resume_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(|e| LibvirtError::Operation(format!("Failed to query VM '{name}': {e}")))?;
    // Idempotent: resume on an already-running domain is a no-op (libvirt otherwise
    // returns VIR_ERR_OPERATION_INVALID and the UI sticky-banners it forever).
    if info.state == VIR_DOMAIN_RUNNING || info.state == VIR_DOMAIN_BLOCKED {
        return Ok(());
    }
    if info.state != VIR_DOMAIN_PAUSED && info.state != VIR_DOMAIN_PMSUSPENDED {
        return Err(LibvirtError::Invalid(format!(
            "Cannot resume VM '{name}': domain is {} (expected paused or suspended)",
            state_to_string(info.state)
        )));
    }
    domain
        .resume()
        .map(|_| ())
        .map_err(|e| LibvirtError::Operation(format!("Failed to resume VM '{name}': {e}")))
}

/// Optional `virDomainUndefineFlags` bits when removing a persistent domain definition.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UndefineOptions {
    #[serde(default)]
    pub managed_save: bool,
    #[serde(default)]
    pub snapshots_metadata: bool,
    #[serde(default)]
    pub nvram: bool,
    #[serde(default)]
    pub keep_nvram: bool,
    #[serde(default)]
    pub checkpoints_metadata: bool,
    #[serde(default)]
    pub tpm: bool,
    #[serde(default)]
    pub keep_tpm: bool,
    /// Delete backing disk image files (only `device='disk'` sources) after undefining.
    #[serde(default)]
    pub delete_disks: bool,
}

impl UndefineOptions {
    /// Build libvirt undefine flags; returns error on contradictory options.
    pub fn to_libvirt_flags(&self) -> Result<sys::virDomainUndefineFlagsValues, LibvirtError> {
        if self.nvram && self.keep_nvram {
            return Err(LibvirtError::Invalid(
                "undefine: nvram and keep_nvram are mutually exclusive".into(),
            ));
        }
        if self.tpm && self.keep_tpm {
            return Err(LibvirtError::Invalid(
                "undefine: tpm and keep_tpm are mutually exclusive".into(),
            ));
        }
        let mut f: u32 = 0;
        if self.managed_save {
            f |= sys::VIR_DOMAIN_UNDEFINE_MANAGED_SAVE;
        }
        if self.snapshots_metadata {
            f |= sys::VIR_DOMAIN_UNDEFINE_SNAPSHOTS_METADATA;
        }
        if self.nvram {
            f |= sys::VIR_DOMAIN_UNDEFINE_NVRAM;
        }
        if self.keep_nvram {
            f |= sys::VIR_DOMAIN_UNDEFINE_KEEP_NVRAM;
        }
        if self.checkpoints_metadata {
            f |= sys::VIR_DOMAIN_UNDEFINE_CHECKPOINTS_METADATA;
        }
        if self.tpm {
            f |= sys::VIR_DOMAIN_UNDEFINE_TPM;
        }
        if self.keep_tpm {
            f |= sys::VIR_DOMAIN_UNDEFINE_KEEP_TPM;
        }
        Ok(f as sys::virDomainUndefineFlagsValues)
    }
}

/// True when libvirt rejected undefine because a UEFI NVRAM file is still attached.
pub fn error_suggests_nvram_undefine(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("nvram") && (m.contains("undefine") || m.contains("cannot remove domain"))
}

/// Undefine persistent domain XML.
///
/// Always adds safe auto-cleanup base flags so that VMs with snapshot metadata, managed-save
/// images, or checkpoint metadata don't silently block deletion. User-supplied flags are OR'd
/// on top. If `undefine_flags` still fails (e.g. NVRAM file already gone), we retry without
/// the optional file-removal flags so the domain record is always dropped.
fn undefine_persistent(domain: &Domain, name: &str, user_flags: u32) -> Result<(), LibvirtError> {
    // These are always safe: remove snapshot/checkpoint/managed-save metadata on delete.
    let base: u32 = sys::VIR_DOMAIN_UNDEFINE_SNAPSHOTS_METADATA
        | sys::VIR_DOMAIN_UNDEFINE_MANAGED_SAVE
        | sys::VIR_DOMAIN_UNDEFINE_CHECKPOINTS_METADATA;
    let flags = user_flags | base;
    let nvram_flag = sys::VIR_DOMAIN_UNDEFINE_NVRAM;
    let nvram_requested = (user_flags & nvram_flag) != 0;

    match domain.undefine_flags(flags as sys::virDomainUndefineFlagsValues) {
        Ok(()) => Ok(()),
        Err(e) => {
            let first = format!("{e}");
            if !nvram_requested && error_suggests_nvram_undefine(&first) {
                let nvram_flags = flags | nvram_flag;
                warn!(
                    "undefine for VM '{name}' requires NVRAM removal — retrying with VIR_DOMAIN_UNDEFINE_NVRAM"
                );
                return domain
                    .undefine_flags(nvram_flags as sys::virDomainUndefineFlagsValues)
                    .map_err(|e2| {
                        LibvirtError::Operation(format!(
                            "Failed to delete VM '{name}' with NVRAM (first: {first}; nvram retry: {e2})"
                        ))
                    });
            }
            warn!(
                "undefine_flags failed for VM '{name}' (flags={flags:#x}), retrying with base flags only: {first}"
            );
            // Retry with just the safe base flags (drops user NVRAM/TPM file-removal bits
            // that may fail if those files no longer exist).
            domain
                .undefine_flags(base as sys::virDomainUndefineFlagsValues)
                .or_else(|_| domain.undefine())
                .map_err(|e2| {
                    LibvirtError::Operation(format!(
                        "Failed to delete VM '{name}' (first: {first}; retry: {e2})"
                    ))
                })
        }
    }
}

pub fn delete_vm(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    delete_vm_with_options(conn, name, &UndefineOptions::default()).map(|_leaked| ())
}

/// Collect file-backed disk paths (`device='disk'`) from domain XML.
/// CDROMs and non-file sources are intentionally excluded.
pub(crate) fn collect_disk_paths(xml: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for block in xml::split_blocks(xml, "disk") {
        let device = xml::extract_attr(&block, "disk", "device").unwrap_or_default();
        if device != "disk" {
            continue;
        }
        let disk_type = xml::extract_attr(&block, "disk", "type").unwrap_or_default();
        if disk_type != "file" {
            continue;
        }
        if let Some(file) = xml::extract_attr(&block, "source", "file") {
            if !file.is_empty() {
                paths.push(file);
            }
        }
    }
    paths
}

/// Target `dev` of every network-backed (`type='network'`, e.g. Ceph/RBD) data
/// disk in a domain XML — the counterpart to [`collect_disk_paths`], which only
/// ever sees `type='file'` sources.
pub(crate) fn collect_network_disk_targets(xml: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for block in xml::split_blocks(xml, "disk") {
        if xml::extract_attr(&block, "disk", "device").as_deref() != Some("disk") {
            continue;
        }
        if xml::extract_attr(&block, "disk", "type").as_deref() != Some("network") {
            continue;
        }
        targets.push(xml::extract_attr(&block, "target", "dev").unwrap_or_default());
    }
    targets
}

/// Stop (if needed) and undefine a VM, optionally passing `virDomainUndefineFlags` bits.
///
/// Returns the target names of any network-backed (e.g. Ceph/RBD) disks that
/// `delete_disks: true` could **not** remove: `collect_disk_paths` only ever
/// sees `type='file'` sources, so an RBD disk was previously neither unlinked
/// (correct — it isn't a local file) nor reported (not correct — the caller got
/// a bare "deleted" success and had no idea the backing volume was still
/// there). Recognizing and reporting it doesn't itself free the volume: that
/// needs a storage-backend call (e.g. to Atlas) this function doesn't make.
pub fn delete_vm_with_options(
    conn: &Connect,
    name: &str,
    opts: &UndefineOptions,
) -> Result<Vec<String>, LibvirtError> {
    let flags_u = opts.to_libvirt_flags()?;
    let flags: u32 = flags_u;
    let domain = match lookup_domain(conn, name) {
        Ok(d) => d,
        Err(LibvirtError::NotFound(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    // Collect disk paths before we undefine (XML is gone after).
    let (disk_paths, leaked_network_disks): (Vec<String>, Vec<String>) = if opts.delete_disks {
        let xml = domain.get_xml_desc(0).unwrap_or_default();
        let leaked = collect_network_disk_targets(&xml);
        if !leaked.is_empty() {
            tracing::warn!(
                "delete_disks=true for VM '{name}' but target(s) {} are network-backed \
                 (e.g. Ceph/RBD) — not a local file, so nothing was unlinked; the backing \
                 volume still exists and needs manual/out-of-band cleanup",
                leaked.join(", ")
            );
        }
        (collect_disk_paths(&xml), leaked)
    } else {
        (Vec::new(), Vec::new())
    };

    let info = domain
        .get_info()
        .map_err(|e| LibvirtError::Operation(format!("Failed to get VM '{name}' info: {e}")))?;
    let persistent = domain.is_persistent().unwrap_or(true);

    if info.state == VIR_DOMAIN_RUNNING || info.state == VIR_DOMAIN_PAUSED {
        domain
            .destroy()
            .map_err(|e| LibvirtError::Operation(format!("Failed to stop VM '{name}': {e}")))?;

        if !persistent {
            delete_disk_files(&disk_paths, name);
            return Ok(leaked_network_disks);
        }

        let domain = match lookup_domain(conn, name) {
            Ok(d) => d,
            Err(LibvirtError::NotFound(_)) => return Ok(leaked_network_disks),
            Err(e) => return Err(e),
        };
        undefine_persistent(&domain, name, flags)?;
        delete_disk_files(&disk_paths, name);
        return Ok(leaked_network_disks);
    }

    undefine_persistent(&domain, name, flags)?;
    delete_disk_files(&disk_paths, name);
    Ok(leaked_network_disks)
}

/// Delete disk image files on the host filesystem, logging but not propagating individual errors.
fn delete_disk_files(paths: &[String], vm_name: &str) {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => tracing::info!("Deleted disk image for VM '{vm_name}': {path}"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!("Disk image already gone for VM '{vm_name}': {path}");
            }
            Err(e) => {
                tracing::warn!("Could not delete disk image for VM '{vm_name}' at {path}: {e}");
            }
        }
    }
}

/// Replace persistent domain XML (virsh define). The `<name>` in XML must match `name`.
pub fn replace_domain_xml(conn: &Connect, name: &str, xml: &str) -> Result<(), LibvirtError> {
    // The domain name is a `<name>` child element, not a `name=` attribute — the
    // old extract_attr always returned None, so the guard never fired and a body
    // with a different <name> would define/overwrite a DIFFERENT domain.
    if let Some(xml_name) = crate::xml::extract_text(xml, "name") {
        if xml_name.trim() != name {
            return Err(LibvirtError::Invalid(format!(
                "XML domain name '{}' does not match '{name}'",
                xml_name.trim()
            )));
        }
    }
    virt::domain::Domain::define_xml(conn, xml).map_err(|e| {
        LibvirtError::Operation(format!("Failed to update domain XML for '{name}': {e}"))
    })?;
    Ok(())
}

pub fn set_autostart(conn: &Connect, name: &str, autostart: bool) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .set_autostart(autostart)
        .map_err(LibvirtError::map_op("Failed to set autostart"))?;
    Ok(())
}

pub fn rename_vm(conn: &Connect, name: &str, new_name: &str) -> Result<(), LibvirtError> {
    crate::validate::validate_name(new_name)?;

    let domain = lookup_domain(conn, name)?;

    // VM must be shutoff to rename
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get VM info"))?;

    if info.state != VIR_DOMAIN_SHUTOFF {
        return Err(LibvirtError::Operation(
            "VM must be shutoff to rename".to_string(),
        ));
    }

    domain
        .rename(new_name, 0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to rename VM '{name}': {e}")))?;

    Ok(())
}

/// Inject Non-Maskable Interrupt (debug hung guests). Requires running or paused domain.
pub fn inject_nmi(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    // Unlike every other name-taking function in this module (rename/clone/etc.),
    // this one shells out to `virsh inject-nmi <name>` with `name` as the final
    // bare argv element and no `--` separator. Validate up front so a caller that
    // reaches this without going through an already-validated VM record (e.g. a
    // future direct agent/gRPC entry point) can't smuggle a virsh flag in via a
    // domain name starting with '-'.
    crate::validate::validate_name(name)?;
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("Failed to get VM info"))?;
    if info.state != VIR_DOMAIN_RUNNING && info.state != VIR_DOMAIN_PAUSED {
        return Err(LibvirtError::Operation(
            "VM must be running or paused to inject NMI".into(),
        ));
    }
    let uri = conn
        .get_uri()
        .map_err(LibvirtError::map_op("Failed to get libvirt URI"))?;
    let out = std::process::Command::new("virsh")
        .args(["-c", &uri, "inject-nmi", name])
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh inject-nmi: {e}")))?;
    if !out.status.success() {
        return Err(LibvirtError::Operation(format!(
            "virsh inject-nmi failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

/// Tail the hypervisor QEMU log for a domain (libvirt default path).
pub fn read_qemu_log(name: &str, lines: usize) -> Result<(String, String), LibvirtError> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(LibvirtError::Invalid("Invalid VM name".into()));
    }
    let log_path = format!("/var/log/libvirt/qemu/{name}.log");
    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => {
            let all: Vec<&str> = c.lines().collect();
            let start = all.len().saturating_sub(lines);
            all[start..].join("\n")
        }
        Err(e) => {
            return Err(LibvirtError::NotFound(format!(
                "QEMU log not readable at {log_path}: {e}"
            )));
        }
    };
    Ok((log_path, content))
}

// ── XML parsing helpers ─────────────────────────────────────────────────

fn parse_os_info(xml_str: &str) -> (String, String) {
    let os_type = xml::extract_text(xml_str, "type").unwrap_or_else(crate::unknown_string);
    let arch = xml::extract_attr(xml_str, "type", "arch").unwrap_or_else(crate::unknown_string);
    (os_type, arch)
}

pub(crate) fn parse_interfaces_for_diff(xml_str: &str) -> Vec<InterfaceInfo> {
    parse_interfaces(xml_str)
}

pub(crate) fn parse_disks_for_diff(xml_str: &str) -> Vec<DiskInfo> {
    parse_disks(xml_str)
}

pub(crate) fn parse_filesystems_for_diff(xml_str: &str) -> Vec<FilesystemInfo> {
    parse_filesystems(xml_str)
}

fn parse_interfaces(xml_str: &str) -> Vec<InterfaceInfo> {
    let mut interfaces = Vec::new();
    for iface_block in xml::split_blocks(xml_str, "interface") {
        let mac =
            xml::extract_attr(&iface_block, "mac", "address").unwrap_or_else(crate::unknown_string);
        let source = xml::extract_attr(&iface_block, "source", "network")
            .or_else(|| xml::extract_attr(&iface_block, "source", "bridge"))
            .unwrap_or_else(crate::unknown_string);
        let model =
            xml::extract_attr(&iface_block, "model", "type").unwrap_or_else(crate::unknown_string);
        interfaces.push(InterfaceInfo {
            mac_address: mac,
            source,
            model,
            ip: None,
        });
    }
    interfaces
}

fn enrich_interface_ips(conn: &Connect, name: &str, interfaces: &mut [InterfaceInfo]) {
    let Ok(rows) = super::guest_agent::get_guest_interfaces(conn, name) else {
        return;
    };
    let mut by_mac: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for row in rows {
        if row.ip_type != "ipv4" {
            continue;
        }
        let addr = row.address.split('/').next().unwrap_or("").trim();
        if addr.is_empty() || addr.starts_with("127.") {
            continue;
        }
        let mac = row.mac.to_ascii_lowercase().replace('-', ":");
        by_mac.entry(mac).or_insert_with(|| addr.to_string());
    }
    for iface in interfaces.iter_mut() {
        let mac = iface.mac_address.to_ascii_lowercase().replace('-', ":");
        if let Some(ip) = by_mac.get(&mac) {
            iface.ip = Some(ip.clone());
        }
    }
}

fn enrich_disk_block_info(domain: &virt::domain::Domain, disks: &mut [DiskInfo]) {
    for disk in disks.iter_mut() {
        if disk.target.is_empty() || disk.target == crate::unknown_string() {
            continue;
        }
        if let Ok(info) = domain.get_block_info(&disk.target, 0) {
            disk.capacity_bytes = Some(info.capacity);
            disk.allocation_bytes = Some(info.allocation);
            disk.physical_bytes = Some(info.physical);
            continue;
        }
        if disk.device == "disk"
            && !disk.source.is_empty()
            && disk.source != crate::unknown_string()
        {
            if let Ok(meta) = std::fs::metadata(&disk.source) {
                disk.physical_bytes = Some(meta.len());
            }
        }
    }
}

fn parse_disks(xml_str: &str) -> Vec<DiskInfo> {
    let mut disks = Vec::new();
    for disk_block in xml::split_blocks(xml_str, "disk") {
        let device =
            xml::extract_attr(&disk_block, "disk", "device").unwrap_or_else(|| "disk".to_string());
        let source = xml::extract_attr(&disk_block, "source", "file")
            .or_else(|| xml::extract_attr(&disk_block, "source", "dev"))
            .or_else(|| xml::extract_attr(&disk_block, "source", "volume"))
            .or_else(|| {
                // Network-backed (e.g. Ceph/RBD): `<source protocol='rbd' name='pool/image'>`
                // has none of the attributes above, so this fell through to
                // "unknown" — every API consumer (UI disk details, backup.sh's
                // disk-path collection) either showed a meaningless value or
                // silently skipped the disk entirely. `rbd:pool/image` is the
                // same source string qemu-img itself accepts.
                if xml::extract_attr(&disk_block, "source", "protocol").as_deref() == Some("rbd") {
                    xml::extract_attr(&disk_block, "source", "name")
                        .map(|name| format!("rbd:{name}"))
                } else {
                    None
                }
            })
            .unwrap_or_else(crate::unknown_string);
        let driver =
            xml::extract_attr(&disk_block, "driver", "type").unwrap_or_else(crate::unknown_string);
        let target =
            xml::extract_attr(&disk_block, "target", "dev").unwrap_or_else(crate::unknown_string);
        let bus = xml::extract_attr(&disk_block, "target", "bus").unwrap_or_default();
        let cache = xml::extract_attr(&disk_block, "driver", "cache").unwrap_or_default();
        let readonly = disk_block.contains("<readonly");
        let shareable = xml::extract_attr(&disk_block, "disk", "shareable")
            .map(|s| s == "yes")
            .unwrap_or(false);
        disks.push(DiskInfo {
            device,
            source,
            driver,
            target,
            bus,
            cache,
            readonly,
            shareable,
            capacity_bytes: None,
            allocation_bytes: None,
            physical_bytes: None,
        });
    }
    disks
}

#[cfg(test)]
mod tests {
    use super::UndefineOptions;

    #[test]
    fn undefine_options_rejects_nvram_conflict() {
        let o = UndefineOptions {
            nvram: true,
            keep_nvram: true,
            ..Default::default()
        };
        assert!(o.to_libvirt_flags().is_err());
    }

    #[test]
    fn error_suggests_nvram_undefine_detects_libvirt_message() {
        assert!(super::error_suggests_nvram_undefine(
            "cannot undefine domain with nvram"
        ));
        assert!(!super::error_suggests_nvram_undefine(
            "disk path mentions nvram-backup"
        ));
    }

    #[test]
    fn parse_disks_reports_an_rbd_source_as_an_rbd_uri() {
        // Before this, a network-backed disk had none of the file/dev/volume
        // attributes parse_disks looked for, so `source` fell back to
        // "unknown" — every API consumer (UI disk details, backup.sh's
        // disk-path collection) either showed a meaningless value or silently
        // skipped the disk entirely.
        let xml = r#"<domain><devices>
            <disk type='network' device='disk'>
              <source protocol='rbd' name='rbd-nvme-prod/csi-vol-abc'>
                <host name='10.43.1.1' port='6789'/>
              </source>
              <target dev='sda' bus='sata'/>
            </disk>
        </devices></domain>"#;
        let disks = super::parse_disks(xml);
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].source, "rbd:rbd-nvme-prod/csi-vol-abc");
    }

    #[test]
    fn parse_disks_still_prefers_file_source_when_present() {
        let xml = r#"<domain><devices>
            <disk type='file' device='disk'>
              <source file='/var/lib/libvirt/images/vm.qcow2'/>
              <target dev='vda' bus='virtio'/>
            </disk>
        </devices></domain>"#;
        let disks = super::parse_disks(xml);
        assert_eq!(disks[0].source, "/var/lib/libvirt/images/vm.qcow2");
    }
}
