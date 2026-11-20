// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::LibvirtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PciDevice {
    pub slot: String,
    pub class: String,
    pub vendor: String,
    pub device: String,
    pub iommu_group: String,
}

/// List host PCI devices by parsing `lspci -vmm -D` output.
///
/// `-D` forces `Slot` to always include the PCI domain (`0000:03:00.0`
/// rather than `03:00.0`) — callers (the PCI hostdev attach/detach UI) feed
/// `slot` straight into `parse_pci_bdf`, which requires a domain-qualified
/// `dom:bus:slot.func` address and rejects anything with fewer than three
/// `:`-separated segments.
pub fn list_pci_devices() -> Result<Vec<PciDevice>, LibvirtError> {
    let output = Command::new("lspci")
        .args(["-vmm", "-D"])
        .output()
        .map_err(LibvirtError::map_op("Failed to run lspci"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    let mut slot = String::new();
    let mut class = String::new();
    let mut vendor = String::new();
    let mut device = String::new();
    let mut iommu = String::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            if !slot.is_empty() {
                devices.push(PciDevice {
                    slot: slot.clone(),
                    class: class.clone(),
                    vendor: vendor.clone(),
                    device: device.clone(),
                    iommu_group: iommu.clone(),
                });
            }
            slot.clear();
            class.clear();
            vendor.clear();
            device.clear();
            iommu.clear();
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim().to_string();
            match key {
                "Slot" => slot = val,
                "Class" => class = val,
                "Vendor" => vendor = val,
                "Device" => device = val,
                "IOMMUGroup" => iommu = val,
                _ => {}
            }
        }
    }
    if !slot.is_empty() {
        devices.push(PciDevice {
            slot,
            class,
            vendor,
            device,
            iommu_group: iommu,
        });
    }

    Ok(devices)
}
