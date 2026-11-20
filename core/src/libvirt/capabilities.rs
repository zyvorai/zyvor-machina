// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypervisorCapabilities {
    pub host_arch: String,
    pub host_cpu_model: String,
    pub guests: Vec<GuestCapability>,
    pub raw_xml: String,
    /// True when libspice-server.so is present on the host (SPICE graphics available).
    pub spice_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestCapability {
    pub os_type: String,
    pub arch: String,
    pub machines: Vec<String>,
}

pub fn get_capabilities(conn: &Connect) -> Result<HypervisorCapabilities, LibvirtError> {
    let xml = conn
        .get_capabilities()
        .map_err(LibvirtError::map_op("Failed to get capabilities"))?;

    let host_arch = crate::xml::extract_text(&xml, "arch").unwrap_or_else(crate::unknown_string);
    let host_cpu_model =
        crate::xml::extract_text(&xml, "model").unwrap_or_else(crate::unknown_string);

    let mut guests = Vec::new();
    for guest_block in crate::xml::split_blocks(&xml, "guest") {
        let os_type = crate::xml::extract_text(&guest_block, "os_type").unwrap_or_default();
        let arch = crate::xml::extract_attr(&guest_block, "arch", "name").unwrap_or_default();
        let mut machines = Vec::new();
        for machine_block in crate::xml::split_blocks(&guest_block, "machine") {
            if let Some(text) = crate::xml::extract_text(&machine_block, "machine") {
                machines.push(text);
            }
        }
        guests.push(GuestCapability {
            os_type,
            arch,
            machines,
        });
    }

    Ok(HypervisorCapabilities {
        host_arch,
        host_cpu_model,
        guests,
        raw_xml: xml,
        spice_available: super::create::has_spice(),
    })
}

pub fn get_sysinfo(conn: &Connect) -> Result<String, LibvirtError> {
    conn.get_sys_info(0)
        .map_err(LibvirtError::map_op("Failed to get sysinfo"))
}
