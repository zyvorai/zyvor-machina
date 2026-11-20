// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;
use virt::nwfilter::NWFilter;

use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NWFilterInfo {
    pub name: String,
    pub uuid: String,
    pub xml: String,
}

pub fn list_nwfilters(conn: &Connect) -> Result<Vec<NWFilterInfo>, LibvirtError> {
    let filters = conn
        .list_all_nw_filters(0)
        .map_err(LibvirtError::map_op("Failed to list network filters"))?;

    let mut result = Vec::new();
    for f in filters {
        result.push(NWFilterInfo {
            name: f.get_name().unwrap_or_default(),
            uuid: f.get_uuid_string().unwrap_or_default(),
            xml: f.get_xml_desc(0).unwrap_or_default(),
        });
    }
    Ok(result)
}

pub fn get_nwfilter_xml(conn: &Connect, name: &str) -> Result<String, LibvirtError> {
    let filter = NWFilter::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Filter '{}' not found: {}", name, e)))?;
    filter
        .get_xml_desc(0)
        .map_err(LibvirtError::map_op("Failed to get filter XML"))
}

pub fn define_nwfilter(conn: &Connect, xml: &str) -> Result<String, LibvirtError> {
    let filter = NWFilter::define_xml(conn, xml)
        .map_err(|e| LibvirtError::Operation(format!("Failed to define filter: {e}")))?;
    filter
        .get_name()
        .map_err(LibvirtError::map_op("Failed to get filter name"))
}

pub fn delete_nwfilter(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let filter = NWFilter::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Filter '{}' not found: {}", name, e)))?;
    filter
        .undefine()
        .map_err(LibvirtError::map_op("Failed to delete filter"))
}
