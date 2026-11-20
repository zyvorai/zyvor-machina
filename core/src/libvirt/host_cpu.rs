// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Host CPU comparison (`virConnectCompareCPU`).

use virt::connect::Connect;

use crate::LibvirtError;

#[derive(Clone, serde::Serialize)]
pub struct CpuCompareResult {
    /// Raw `virCPUCompareResult` (-1 error handled separately).
    pub code: i32,
    pub label: &'static str,
}

/// Compare guest CPU XML against this host. `flags` are `virConnectCompareCPUFlags` (often 0).
pub fn compare_cpu(
    conn: &Connect,
    cpu_xml: &str,
    flags: u32,
) -> Result<CpuCompareResult, LibvirtError> {
    let code = conn
        .compare_cpu(cpu_xml, flags as _)
        .map_err(|e| LibvirtError::Operation(format!("compare_cpu: {e}")))?;
    let code_i = code;
    let label = match code_i {
        x if x == virt::sys::VIR_CPU_COMPARE_IDENTICAL => "identical",
        x if x == virt::sys::VIR_CPU_COMPARE_SUPERSET => "superset",
        x if x == virt::sys::VIR_CPU_COMPARE_INCOMPATIBLE => "incompatible",
        _ => "unknown",
    };
    Ok(CpuCompareResult {
        code: code_i,
        label,
    })
}
