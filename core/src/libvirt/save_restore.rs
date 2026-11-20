// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use super::domain::lookup_domain;
use crate::LibvirtError;

pub fn managed_save(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .managed_save(0)
        .map_err(|e| LibvirtError::Operation(format!("Failed to managed-save VM '{name}': {e}")))?;
    Ok(())
}

pub fn managed_save_remove(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain.managed_save_remove(0).map_err(|e| {
        LibvirtError::Operation(format!("Failed to remove managed save for '{name}': {e}"))
    })?;
    Ok(())
}

pub fn has_managed_save(conn: &Connect, name: &str) -> Result<bool, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    domain
        .has_managed_save(0)
        .map_err(LibvirtError::map_op("Failed to check managed save"))
}
