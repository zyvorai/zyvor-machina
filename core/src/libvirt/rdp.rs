// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Resolve guest RDP endpoints (IPv4 on port 3389) for built-in TCP/WebSocket proxy.

use virt::connect::Connect;

use super::guest_agent;
use crate::LibvirtError;

/// Prefer the first non-loopback IPv4 guest address; default port 3389.
pub fn resolve_rdp_endpoint(
    conn: &Connect,
    domain_name: &str,
) -> Result<(String, u16), LibvirtError> {
    let addrs = guest_agent::get_guest_interfaces(conn, domain_name)?;
    for a in &addrs {
        if a.ip_type.eq_ignore_ascii_case("ipv4") {
            let ip = a.address.trim();
            if !ip.is_empty() && !ip.starts_with("127.") && !ip.starts_with("169.254.") {
                return Ok((ip.to_string(), 3389));
            }
        }
    }
    Err(LibvirtError::NotFound(
        "No guest IPv4 address for RDP — ensure QEMU guest agent is running and the VM has an address"
            .into(),
    ))
}
