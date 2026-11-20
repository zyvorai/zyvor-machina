// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use crate::state::NodeInfo;
use crate::LibvirtError;

pub fn get_node_info(conn: &Connect) -> Result<NodeInfo, LibvirtError> {
    let hostname = conn
        .get_hostname()
        .map_err(LibvirtError::map_op("Failed to get hostname"))?;

    let hv_type = conn
        .get_type()
        .map_err(LibvirtError::map_op("Failed to get hypervisor type"))?;

    let format_version =
        |v: u32| format!("{}.{}.{}", v / 1_000_000, (v / 1_000) % 1_000, v % 1_000);

    let hv_version = conn
        .get_hyp_version()
        .map(format_version)
        .unwrap_or_else(|_| crate::unknown_string());
    let lib_version = conn
        .get_lib_version()
        .map(format_version)
        .unwrap_or_else(|_| crate::unknown_string());

    let node = conn
        .get_node_info()
        .map_err(LibvirtError::map_op("Failed to get node info"))?;

    Ok(NodeInfo {
        hostname,
        hypervisor: hv_type,
        hypervisor_version: hv_version,
        lib_version,
        cpu_model: node.model,
        cpu_cores: node.cores,
        cpu_threads: node.threads,
        cpu_sockets: node.sockets,
        memory_mb: node.memory / 1024,
        numa_nodes: node.nodes,
        active_vms: conn.num_of_domains().unwrap_or(0),
        defined_vms: conn.num_of_defined_domains().unwrap_or(0),
    })
}
