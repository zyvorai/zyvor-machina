// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDevice {
    pub name: String,
    pub parent: String,
    pub driver: String,
    pub capability_type: String,
    pub xml: String,
}

pub fn list_node_devices(
    conn: &Connect,
    capability: Option<&str>,
) -> Result<Vec<NodeDevice>, LibvirtError> {
    let flags = 0u32;
    let devices = conn
        .list_all_node_devices(flags)
        .map_err(LibvirtError::map_op("Failed to list node devices"))?;

    let mut result = Vec::new();
    for dev in devices {
        let name = dev.get_name().unwrap_or_default();
        let xml = dev.get_xml_desc(0).unwrap_or_default();
        let parent = dev.get_parent().unwrap_or_default();

        let cap_type = crate::xml::extract_attr(&xml, "capability", "type").unwrap_or_default();
        let driver = crate::xml::extract_text(&xml, "driver").unwrap_or_default();

        if let Some(filter) = capability {
            if cap_type != filter {
                continue;
            }
        }

        result.push(NodeDevice {
            name,
            parent,
            driver,
            capability_type: cap_type,
            xml,
        });
    }

    Ok(result)
}

pub fn get_node_device_xml(conn: &Connect, name: &str) -> Result<String, LibvirtError> {
    let dev = virt::nodedev::NodeDevice::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Device '{}' not found: {}", name, e)))?;
    dev.get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get device XML"))
}

/// `virNodeDeviceDettach` — required before some PCI passthrough / VFIO workflows.
pub fn detach_node_device(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let dev = virt::nodedev::NodeDevice::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Device '{}' not found: {}", name, e)))?;
    dev.detach().map_err(|e| {
        LibvirtError::Operation(format!("Failed to detach node device '{name}': {e}"))
    })?;
    Ok(())
}

/// `virNodeDeviceReAttach` — return device to host drivers after `detach_node_device`.
pub fn reattach_node_device(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let dev = virt::nodedev::NodeDevice::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Device '{}' not found: {}", name, e)))?;
    dev.reattach().map_err(|e| {
        LibvirtError::Operation(format!("Failed to reattach node device '{name}': {e}"))
    })?;
    Ok(())
}
