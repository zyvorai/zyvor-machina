// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Pin emulator threads to physical CPUs (`virDomainPinEmulator`).

use virt::connect::Connect;

use super::device::get_domain_flags_pub;
use super::domain::lookup_domain;
use crate::LibvirtError;

fn bools_to_cpumap(cpus: &[bool]) -> Vec<u8> {
    cpus.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &set)| if set { acc | (1 << i) } else { acc })
        })
        .collect()
}

pub fn pin_emulator(conn: &Connect, name: &str, cpus: &[bool]) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let cpumap = bools_to_cpumap(cpus);
    let flags = get_domain_flags_pub(&domain);
    domain
        .pin_emulator(&cpumap, flags)
        .map_err(|e| LibvirtError::Operation(format!("pin_emulator for '{name}': {e}")))?;
    Ok(())
}
