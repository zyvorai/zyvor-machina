// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Background job progress (`virDomainGetJobInfo` / `virDomainGetJobStats`).

use serde_json::json;
use virt::connect::Connect;

use super::domain::lookup_domain;
use crate::LibvirtError;

pub fn job_info(conn: &Connect, name: &str) -> Result<serde_json::Value, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let j = domain
        .get_job_info()
        .map_err(|e| LibvirtError::Operation(format!("get_job_info '{name}': {e}")))?;
    Ok(job_stats_to_json(&j))
}

pub fn job_stats(
    conn: &Connect,
    name: &str,
    flags: virt::sys::virDomainGetJobStatsFlags,
) -> Result<serde_json::Value, LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let j = domain
        .get_job_stats(flags)
        .map_err(|e| LibvirtError::Operation(format!("get_job_stats '{name}': {e}")))?;
    Ok(job_stats_to_json(&j))
}

/// Same as [`job_stats`] with `flags` as `u32` (`virDomainGetJobStatsFlags`).
pub fn job_stats_u32(
    conn: &Connect,
    name: &str,
    flags: u32,
) -> Result<serde_json::Value, LibvirtError> {
    job_stats(conn, name, flags as _)
}

fn job_stats_to_json(j: &virt::domain::JobStats) -> serde_json::Value {
    json!({
        "type": j.r#type,
        "auto_converge_throttle": j.auto_converge_throttle,
        "compression_bytes": j.compression_bytes,
        "compression_cache": j.compression_cache,
        "compression_cache_misses": j.compression_cache_misses,
        "compression_overflow": j.compression_overflow,
        "compression_pages": j.compression_pages,
        "data_processed": j.data_processed,
        "data_remaining": j.data_remaining,
        "data_total": j.data_total,
        "disk_bps": j.disk_bps,
        "disk_processed": j.disk_processed,
        "disk_remaining": j.disk_remaining,
        "disk_temp_total": j.disk_temp_total,
        "disk_temp_used": j.disk_temp_used,
        "disk_total": j.disk_total,
        "downtime": j.downtime,
        "downtime_net": j.downtime_net,
        "error_message": j.error_message,
        "mem_bps": j.mem_bps,
        "mem_constant": j.mem_constant,
        "mem_dirty_rate": j.mem_dirty_rate,
        "mem_iteration": j.mem_iteration,
        "mem_normal": j.mem_normal,
        "mem_normal_bytes": j.mem_normal_bytes,
        "mem_page_size": j.mem_page_size,
        "mem_postcopy_reqs": j.mem_postcopy_reqs,
        "mem_processed": j.mem_processed,
        "mem_remaining": j.mem_remaining,
        "mem_total": j.mem_total,
        "operation": j.operation,
        "setup_time": j.setup_time,
        "success": j.success,
        "time_elapsed": j.time_elapsed,
        "time_elapsed_net": j.time_elapsed_net,
        "time_remaining": j.time_remaining,
    })
}
