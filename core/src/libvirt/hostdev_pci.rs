// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Attach / detach PCI `hostdev` devices (VFIO / PCI passthrough).

use virt::connect::Connect;

use super::device::{get_domain_flags_pub, DetachOutcome, DETACH_LIVE_WAIT};
use super::domain::{lookup_domain, wait_until_absent_from_live};
use crate::LibvirtError;

fn hex_u32_max4(s: &str) -> Result<u32, LibvirtError> {
    if s.is_empty() || s.len() > 4 {
        return Err(LibvirtError::Invalid(format!("Invalid hex segment '{s}'")));
    }
    u32::from_str_radix(s, 16).map_err(|_| LibvirtError::Invalid(format!("Invalid hex '{s}'")))
}

fn hex_u8_exact2(s: &str) -> Result<u8, LibvirtError> {
    if s.len() != 2 {
        return Err(LibvirtError::Invalid(format!(
            "PCI segment must be 2 hex digits, got '{s}'"
        )));
    }
    u8::from_str_radix(s, 16).map_err(|_| LibvirtError::Invalid(format!("Invalid hex '{s}'")))
}

/// PCI BDF as `BBBB:BB:DD.F` (e.g. `0000:03:00.0`), domain/bus/slot in hex, function 0–7.
pub fn parse_pci_bdf(s: &str) -> Result<(String, String, String, String), LibvirtError> {
    let s = s.trim();
    let (prefix, func_s) = s.rsplit_once('.').ok_or_else(|| {
        LibvirtError::Invalid(format!(
            "Invalid PCI address '{s}' (expected e.g. 0000:03:00.0)"
        ))
    })?;
    let func: u8 = func_s
        .parse()
        .map_err(|_| LibvirtError::Invalid(format!("Invalid PCI function '{func_s}'")))?;
    if func > 7 {
        return Err(LibvirtError::Invalid("PCI function must be 0–7".into()));
    }
    let parts: Vec<&str> = prefix.split(':').collect();
    if parts.len() != 3 {
        return Err(LibvirtError::Invalid(format!("Invalid PCI address '{s}'")));
    }
    let dom = hex_u32_max4(parts[0])?;
    let bus = hex_u8_exact2(parts[1])?;
    let slot = hex_u8_exact2(parts[2])?;
    Ok((
        format!("{dom:04x}"),
        format!("{bus:02x}"),
        format!("{slot:02x}"),
        format!("{func:x}"),
    ))
}

fn pci_hostdev_xml(pci_domain: &str, bus: &str, slot: &str, function: &str) -> String {
    format!(
        r#"<hostdev mode='subsystem' type='pci' managed='yes'>
  <source>
    <address domain='0x{pci_domain}' bus='0x{bus}' slot='0x{slot}' function='0x{function}'/>
  </source>
</hostdev>"#
    )
}

pub fn attach_pci_hostdev(
    conn: &Connect,
    vm_name: &str,
    pci_bdf: &str,
) -> Result<(), LibvirtError> {
    let (pci_domain, bus, slot, function) = parse_pci_bdf(pci_bdf)?;
    let xml = pci_hostdev_xml(&pci_domain, &bus, &slot, &function);
    let dom = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags_pub(&dom);
    dom.attach_device_flags(&xml, flags).map_err(|e| {
        LibvirtError::Operation(format!("Failed to attach PCI hostdev to '{vm_name}': {e}"))
    })?;
    Ok(())
}

pub fn detach_pci_hostdev(
    conn: &Connect,
    vm_name: &str,
    pci_bdf: &str,
) -> Result<DetachOutcome, LibvirtError> {
    let (pci_domain, bus, slot, function) = parse_pci_bdf(pci_bdf)?;
    let xml = pci_hostdev_xml(&pci_domain, &bus, &slot, &function);
    let dom = lookup_domain(conn, vm_name)?;
    let flags = get_domain_flags_pub(&dom);
    dom.detach_device_flags(&xml, flags).map_err(|e| {
        LibvirtError::Operation(format!(
            "Failed to detach PCI hostdev from '{vm_name}': {e}"
        ))
    })?;

    // Same async-completion caveat as disk/NIC detach: VFIO hot-unplug also needs the
    // guest to release the device before it actually leaves the live domain.
    let source_addr =
        format!("domain='0x{pci_domain}' bus='0x{bus}' slot='0x{slot}' function='0x{function}'");
    let is_running = dom.is_active().unwrap_or(false);
    let live_removed = if is_running {
        wait_until_absent_from_live(&dom, DETACH_LIVE_WAIT, |live_xml| {
            !live_xml.contains(&source_addr)
        })
    } else {
        true
    };
    Ok(DetachOutcome { live_removed })
}

#[cfg(test)]
mod tests {
    use super::parse_pci_bdf;

    #[test]
    fn parse_pci_bdf_ok() {
        let (d, b, s, f) = parse_pci_bdf("0000:03:00.0").unwrap();
        assert_eq!(d, "0000");
        assert_eq!(b, "03");
        assert_eq!(s, "00");
        assert_eq!(f, "0");
    }

    #[test]
    fn parse_pci_bdf_rejects_bad_func() {
        assert!(parse_pci_bdf("0000:03:00.9").is_err());
    }
}
