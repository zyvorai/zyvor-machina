// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! NUMA tuning (`virDomainGetNumaParameters` / `virDomainSetNumaParameters`).

use virt::connect::Connect;

use super::domain::lookup_domain;
use super::resize::domain_affect_flag;
use crate::LibvirtError;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct NumaTuneState {
    pub node_set: Option<String>,
    /// Raw libvirt mode (`virDomainNumatuneMemMode`).
    pub mode: Option<i32>,
}

pub fn get_numa_tune(conn: &Connect, name: &str) -> Result<NumaTuneState, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let affect = domain_affect_flag(&domain);
    let p = domain
        .get_numa_parameters(affect)
        .map_err(|e| LibvirtError::Operation(format!("get_numa_parameters for '{name}': {e}")))?;
    Ok(NumaTuneState {
        node_set: p.node_set,
        mode: p.mode,
    })
}

#[derive(Clone, serde::Deserialize)]
pub struct SetNumaTuneRequest {
    pub node_set: Option<String>,
    pub mode: Option<i32>,
}

pub fn set_numa_tune(
    conn: &Connect,
    name: &str,
    req: &SetNumaTuneRequest,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let affect = domain_affect_flag(&domain);
    let mut p = domain
        .get_numa_parameters(affect)
        .map_err(|e| LibvirtError::Operation(format!("get_numa_parameters for '{name}': {e}")))?;
    if let Some(ref s) = req.node_set {
        p.node_set = Some(s.clone());
    }
    if let Some(m) = req.mode {
        p.mode = Some(m);
    }
    domain
        .set_numa_parameters(p, affect)
        .map_err(|e| LibvirtError::Operation(format!("set_numa_parameters for '{name}': {e}")))?;
    Ok(())
}
