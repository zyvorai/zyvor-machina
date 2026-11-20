// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use std::process::Command;
use virt::connect::Connect;

use crate::libvirt::domain::lookup_domain;
use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDevice {
    pub bus: String,
    pub device: String,
    pub vendor_id: String,
    pub product_id: String,
    pub description: String,
}

/// List host USB devices via lsusb.
pub fn list_usb_devices() -> Result<Vec<UsbDevice>, LibvirtError> {
    let output = Command::new("lsusb")
        .output()
        .map_err(LibvirtError::map_op("Failed to run lsusb"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines() {
        // Format: Bus 001 Device 002: ID 1234:5678 Description
        let parts: Vec<&str> = line.splitn(7, ' ').collect();
        if parts.len() >= 7 {
            let bus = parts[1].to_string();
            let device = parts[3].trim_end_matches(':').to_string();
            let id = parts[5];
            let id_parts: Vec<&str> = id.split(':').collect();
            if id_parts.len() == 2 {
                devices.push(UsbDevice {
                    bus,
                    device,
                    vendor_id: id_parts[0].to_string(),
                    product_id: id_parts[1].to_string(),
                    description: parts[6..].join(" "),
                });
            }
        }
    }

    Ok(devices)
}

/// Attach a USB device to a VM by vendor:product ID.
pub fn attach_usb(
    conn: &Connect,
    vm_name: &str,
    vendor_id: &str,
    product_id: &str,
) -> Result<(), LibvirtError> {
    if vendor_id.len() != 4
        || product_id.len() != 4
        || !vendor_id.chars().all(|c| c.is_ascii_hexdigit())
        || !product_id.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(LibvirtError::Invalid(
            "Invalid USB vendor/product ID format".to_string(),
        ));
    }

    let domain = lookup_domain(conn, vm_name)?;
    let xml = format!(
        r#"<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x{vendor_id}'/>
    <product id='0x{product_id}'/>
  </source>
</hostdev>"#,
    );

    let flags = crate::libvirt::device::get_domain_flags_pub(&domain);
    domain
        .attach_device_flags(&xml, flags)
        .map_err(LibvirtError::map_op("Failed to attach USB device"))?;
    Ok(())
}

/// Detach a USB device from a VM.
pub fn detach_usb(
    conn: &Connect,
    vm_name: &str,
    vendor_id: &str,
    product_id: &str,
) -> Result<(), LibvirtError> {
    if vendor_id.len() != 4
        || product_id.len() != 4
        || !vendor_id.chars().all(|c| c.is_ascii_hexdigit())
        || !product_id.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(LibvirtError::Invalid(
            "Invalid USB vendor/product ID format".to_string(),
        ));
    }

    let domain = lookup_domain(conn, vm_name)?;
    let xml = format!(
        r#"<hostdev mode='subsystem' type='usb' managed='yes'>
  <source>
    <vendor id='0x{vendor_id}'/>
    <product id='0x{product_id}'/>
  </source>
</hostdev>"#,
    );

    let flags = crate::libvirt::device::get_domain_flags_pub(&domain);
    domain
        .detach_device_flags(&xml, flags)
        .map_err(LibvirtError::map_op("Failed to detach USB device"))?;
    Ok(())
}
