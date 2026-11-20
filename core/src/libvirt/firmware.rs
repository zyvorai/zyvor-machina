// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Switch guest firmware between BIOS and UEFI (OVMF) using the same OVMF discovery as create.

use virt::connect::Connect;
use virt::domain::Domain;

use super::create::{find_ovmf_code, find_ovmf_vars_template};
use super::domain::lookup_domain;
use crate::xml;
use crate::LibvirtError;

fn find_os_block(xml: &str) -> Option<(usize, usize)> {
    let start = xml.find("<os")?;
    let gt = xml[start..].find('>')? + start;
    let after_open = gt + 1;
    let close_rel = xml[after_open..].find("</os>")?;
    let end = after_open + close_rel + "</os>".len();
    Some((start, end))
}

fn extract_type_open_line(os_block: &str) -> String {
    // First <type ...>...</type> line (may span one line)
    if let Some(p) = os_block.find("<type") {
        if let Some(gt) = os_block[p..].find('>') {
            let line_end = os_block[p..]
                .find("</type>")
                .map(|i| p + i + "</type>".len())
                .unwrap_or(p + gt + 1);
            return os_block[p..line_end].to_string();
        }
    }
    "<type arch='x86_64' machine='q35'>hvm</type>".to_string()
}

/// `uefi` uses host OVMF files when found; `bios` removes loader/nvram.
pub fn set_guest_firmware(conn: &Connect, vm_name: &str, uefi: bool) -> Result<(), LibvirtError> {
    crate::validate::validate_name(vm_name)?;
    let domain = lookup_domain(conn, vm_name)?;
    let mut full = domain
        .get_xml_desc(virt::sys::VIR_DOMAIN_XML_INACTIVE)
        .map_err(LibvirtError::map_op("get_xml inactive"))?;

    let (os_start, os_end) = find_os_block(&full).ok_or_else(|| {
        LibvirtError::Operation("Could not locate <os> block in domain XML".into())
    })?;
    let os_block = &full[os_start..os_end];
    let type_line = extract_type_open_line(os_block);

    let new_os = if uefi {
        let code = find_ovmf_code().ok_or_else(|| {
            LibvirtError::Operation(
                "No OVMF_CODE.fd found on host (install edk2-ovmf / qemu-ovmf packages)".into(),
            )
        })?;
        let code_esc = xml::escape(&code);
        let nvram_path = format!("/var/lib/libvirt/qemu/nvram/{}_VARS.fd", vm_name);
        let vars_template = find_ovmf_vars_template();
        let nvram_line = if let Some(ref t) = vars_template {
            format!(
                "    <nvram template='{}'>{}</nvram>",
                xml::escape(t),
                xml::escape(&nvram_path)
            )
        } else {
            format!("    <nvram>{}</nvram>", xml::escape(&nvram_path))
        };
        format!(
            "  <os>\n    {}\n    <loader readonly='yes' type='pflash'>{}</loader>\n{}\n  </os>",
            type_line.trim(),
            code_esc,
            nvram_line
        )
    } else {
        format!(
            "  <os>\n    {}\n    <boot dev='hd'/>\n  </os>",
            type_line.trim()
        )
    };

    full.replace_range(os_start..os_end, &new_os);
    Domain::define_xml(conn, &full)
        .map_err(|e| LibvirtError::Operation(format!("define_xml firmware: {e}")))?;
    Ok(())
}
