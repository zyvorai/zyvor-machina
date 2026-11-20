// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional devices: TPM 2.0 (emulated), watchdog, sound, extra serial PTY.

use virt::connect::Connect;

use super::device::{get_domain_flags, DetachOutcome, DETACH_LIVE_WAIT};
use super::domain::{lookup_domain, wait_until_absent_from_live};
use crate::xml::{self, split_blocks, strip_runtime_only_attrs};
use crate::LibvirtError;

fn tpm_present(xml: &str) -> bool {
    xml.contains("<tpm") || xml.contains("<tpm ")
}

/// Attach an emulated TPM 2.0 (CRB) if none exists.
pub fn attach_tpm_emulator(conn: &Connect, vm_name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    if tpm_present(&desc) {
        return Err(LibvirtError::Invalid("VM already has a TPM device".into()));
    }
    let tpm_xml = r#"<tpm model="tpm-crb">
  <backend type="emulator" version="2.0"/>
</tpm>"#;
    let flags = get_domain_flags(&domain);
    domain
        .attach_device_flags(tpm_xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("attach TPM: {e}")))?;
    Ok(())
}

/// Remove the first TPM device from the domain XML.
pub fn detach_tpm(conn: &Connect, vm_name: &str) -> Result<DetachOutcome, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let blocks = split_blocks(&desc, "tpm");
    let Some(first) = blocks.first() else {
        return Err(LibvirtError::NotFound(format!("No TPM on VM '{vm_name}'")));
    };
    // Strip <alias>/<address> — runtime-only, never in the persistent config; matching
    // them on the CONFIG side of a combined live+config detach fails with a false
    // "device not found" (see strip_runtime_only_attrs doc comment / the same bug fixed
    // for NIC detach in device.rs).
    let xml = strip_runtime_only_attrs(first);
    let flags = get_domain_flags(&domain);
    domain
        .detach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("detach TPM: {e}")))?;

    let is_running = domain.is_active().unwrap_or(false);
    let live_removed = if is_running {
        wait_until_absent_from_live(&domain, DETACH_LIVE_WAIT, |live_xml| !tpm_present(live_xml))
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

const WATCHDOG_MODELS: &[&str] = &["i6300esb", "ib700", "diag288"];
const WATCHDOG_ACTIONS: &[&str] = &["reset", "shutdown", "poweroff", "pause", "none", "dump"];

/// Attach a watchdog device (`model` + `action` when guest hangs).
pub fn attach_watchdog(
    conn: &Connect,
    vm_name: &str,
    model: &str,
    action: &str,
) -> Result<(), LibvirtError> {
    if !WATCHDOG_MODELS.contains(&model) {
        return Err(LibvirtError::Invalid(format!(
            "watchdog model must be one of: {}",
            WATCHDOG_MODELS.join(", ")
        )));
    }
    if !WATCHDOG_ACTIONS.contains(&action) {
        return Err(LibvirtError::Invalid(format!(
            "watchdog action must be one of: {}",
            WATCHDOG_ACTIONS.join(", ")
        )));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let xml = format!(
        r#"<watchdog model="{}" action="{}"/>"#,
        xml::escape(model),
        xml::escape(action),
    );
    let flags = get_domain_flags(&domain);
    domain
        .attach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("attach watchdog: {e}")))?;
    Ok(())
}

const SOUND_MODELS: &[&str] = &["ich6", "ich9", "ac97", "es1370", "sb16", "pcspk"];

/// Attach a virtual sound card.
pub fn attach_sound(conn: &Connect, vm_name: &str, model: &str) -> Result<(), LibvirtError> {
    if !SOUND_MODELS.contains(&model) {
        return Err(LibvirtError::Invalid(format!(
            "sound model must be one of: {}",
            SOUND_MODELS.join(", ")
        )));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let xml = format!(r#"<sound model="{}"/>"#, xml::escape(model));
    let flags = get_domain_flags(&domain);
    domain
        .attach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("attach sound: {e}")))?;
    Ok(())
}

fn vsock_present(xml: &str) -> bool {
    xml.contains("<vsock") || xml.contains("<vsock ")
}

/// Attach a virtio vsock device (guest↔host AF_VSOCK). `cid` is optional; libvirt auto-assigns when omitted.
pub fn attach_vsock(conn: &Connect, vm_name: &str, cid: Option<u32>) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    if vsock_present(&desc) {
        return Err(LibvirtError::Invalid(
            "VM already has a vsock device".into(),
        ));
    }
    let cid_xml = match cid {
        Some(id) => format!(r#"  <cid address="{}"/>"#, id),
        None => r#"  <cid auto="yes"/>"#.into(),
    };
    let xml = format!(
        r#"<vsock model="virtio">
{cid_xml}
</vsock>"#
    );
    let flags = get_domain_flags(&domain);
    domain
        .attach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("attach vsock: {e}")))?;
    Ok(())
}

/// Remove the first vsock device from the domain XML.
pub fn detach_vsock(conn: &Connect, vm_name: &str) -> Result<DetachOutcome, LibvirtError> {
    let domain = lookup_domain(conn, vm_name)?;
    let desc = domain
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("get_xml"))?;
    let blocks = split_blocks(&desc, "vsock");
    let Some(first) = blocks.first() else {
        return Err(LibvirtError::NotFound(format!(
            "No vsock on VM '{vm_name}'"
        )));
    };
    // See detach_tpm: strip runtime-only <alias>/<address> before detaching.
    let xml = strip_runtime_only_attrs(first);
    let flags = get_domain_flags(&domain);
    domain
        .detach_device_flags(&xml, flags)
        .map_err(|e| LibvirtError::Operation(format!("detach vsock: {e}")))?;

    let is_running = domain.is_active().unwrap_or(false);
    let live_removed = if is_running {
        wait_until_absent_from_live(&domain, DETACH_LIVE_WAIT, |live_xml| {
            !vsock_present(live_xml)
        })
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

/// Add another serial+console pair on a PTY (`port` is the guest index, e.g. 1 for ttyS1).
pub fn attach_serial_pty(conn: &Connect, vm_name: &str, port: u32) -> Result<(), LibvirtError> {
    if port > 32 {
        return Err(LibvirtError::Invalid("serial port must be 0..=32".into()));
    }
    let domain = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags(&domain);
    // Attach as two calls — some libvirt versions prefer separate attach for serial vs console
    domain
        .attach_device_flags(
            &format!(
                r#"<serial type="pty">
  <target port="{}"/>
</serial>"#,
                port
            ),
            flags,
        )
        .map_err(|e| LibvirtError::Operation(format!("attach serial: {e}")))?;
    domain
        .attach_device_flags(
            &format!(
                r#"<console type="pty">
  <target type="serial" port="{}"/>
</console>"#,
                port
            ),
            flags,
        )
        .map_err(|e| LibvirtError::Operation(format!("attach console: {e}")))?;
    Ok(())
}
