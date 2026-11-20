// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::path::Path;

use virt::connect::Connect;
use virt::domain::Domain;

use super::domain::lookup_domain;
use crate::LibvirtError;

fn find_virtiofsd() -> Option<&'static str> {
    // Common paths across distros
    [
        "/usr/libexec/virtiofsd",
        "/usr/lib/qemu/virtiofsd",
        "/usr/lib64/qemu/virtiofsd",
        "/usr/bin/virtiofsd",
    ]
    .into_iter()
    .find(|&p| Path::new(p).is_file())
    .map(|v| v as _)
}

fn ensure_memory_backing_shared(vm_xml: &str) -> String {
    // virtiofs requires a shared memory backing (memfd + shared access).
    // We preserve any existing memoryBacking content and only add missing elements.
    let open = "<memoryBacking";
    let close = "</memoryBacking>";
    if let Some(start) = vm_xml.find(open) {
        if let Some(end_rel) = vm_xml[start..].find(close) {
            let end = start + end_rel + close.len();
            let block = &vm_xml[start..end];
            let mut new_block = block.to_string();

            // Insert after the opening <memoryBacking...> tag.
            if let Some(gt) = new_block.find('>') {
                let after_open = gt + 1;
                if !new_block.contains("source type='memfd'")
                    && !new_block.contains("source type=\"memfd\"")
                {
                    new_block.insert_str(after_open, "\n    <source type='memfd'/>");
                }
                if !new_block.contains("access mode='shared'")
                    && !new_block.contains("access mode=\"shared\"")
                {
                    // place after source if present
                    if let Some(src_pos) = new_block.find("<source") {
                        if let Some(src_end) = new_block[src_pos..].find("/>") {
                            let insert_at = src_pos + src_end + 2;
                            new_block.insert_str(insert_at, "\n    <access mode='shared'/>");
                        } else {
                            new_block.insert_str(after_open, "\n    <access mode='shared'/>");
                        }
                    } else {
                        new_block.insert_str(after_open, "\n    <access mode='shared'/>");
                    }
                }
            }

            let mut out = String::new();
            out.push_str(&vm_xml[..start]);
            out.push_str(&new_block);
            out.push_str(&vm_xml[end..]);
            return out;
        }
    }

    // No memoryBacking block: insert before <devices>.
    if let Some(pos) = vm_xml.find("<devices>") {
        let mut out = String::new();
        out.push_str(&vm_xml[..pos]);
        out.push_str(
            "  <memoryBacking>\n    <source type='memfd'/>\n    <access mode='shared'/>\n  </memoryBacking>\n  ",
        );
        out.push_str(&vm_xml[pos..]);
        return out;
    }
    vm_xml.to_string()
}

fn insert_filesystem_into_devices(vm_xml: &str, fs_xml: &str) -> String {
    if let Some(pos) = vm_xml.rfind("</devices>") {
        let mut result = vm_xml[..pos].to_string();
        result.push_str("    ");
        result.push_str(fs_xml);
        result.push('\n');
        result.push_str("  ");
        result.push_str(&vm_xml[pos..]);
        result
    } else {
        vm_xml.to_string()
    }
}

fn remove_filesystem_by_tag(vm_xml: &str, mount_tag: &str) -> Result<String, LibvirtError> {
    if mount_tag.trim().is_empty() {
        return Err(LibvirtError::Invalid("mount_tag is required".into()));
    }
    let mut changed = false;
    let mut out = vm_xml.to_string();
    for block in crate::xml::split_blocks(vm_xml, "filesystem") {
        let tag = crate::xml::extract_attr(&block, "target", "dir").unwrap_or_default();
        if tag == mount_tag {
            out = out.replace(&block, "");
            changed = true;
            break;
        }
    }
    if !changed {
        return Err(LibvirtError::NotFound(format!(
            "No shared filesystem with mount_tag '{mount_tag}'"
        )));
    }
    Ok(out)
}

pub fn add_virtiofs_share(
    conn: &Connect,
    name: &str,
    source_dir: &str,
    mount_tag: &str,
    xattr: bool,
) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;

    // Cockpit requires shutoff; we do too because memoryBacking + virtiofs wiring is not hot-pluggable safely.
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("get domain info"))?;
    if info.state == 1 {
        return Err(LibvirtError::Invalid(
            "VM must be shut off to add a shared directory (virtiofs requires memoryBacking changes)".into(),
        ));
    }

    let p = Path::new(source_dir);
    if !p.is_absolute() {
        return Err(LibvirtError::Invalid(
            "source_dir must be an absolute path".into(),
        ));
    }
    if !p.is_dir() {
        return Err(LibvirtError::Operation(format!(
            "source_dir is not an existing directory: {source_dir}"
        )));
    }
    if mount_tag.trim().is_empty() {
        return Err(LibvirtError::Invalid("mount_tag is required".into()));
    }
    if !mount_tag
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(LibvirtError::Invalid(
            "mount_tag may only contain letters, digits, dot, underscore, hyphen".into(),
        ));
    }

    let vm_xml = domain.get_xml_desc(0).unwrap_or_default();
    for block in crate::xml::split_blocks(&vm_xml, "filesystem") {
        let tag = crate::xml::extract_attr(&block, "target", "dir").unwrap_or_default();
        if tag == mount_tag {
            return Err(LibvirtError::Invalid(format!(
                "A shared filesystem with mount_tag '{mount_tag}' already exists"
            )));
        }
    }

    let virtiofsd = find_virtiofsd().ok_or_else(|| {
        LibvirtError::Operation(
            "virtiofsd not found on host (install qemu/virtiofsd package or set up external virtiofsd)".into(),
        )
    })?;

    let binary_xml = if xattr {
        format!(
            "\n  <binary path='{}' xattr='on'/>",
            crate::xml::escape(virtiofsd)
        )
    } else {
        String::new()
    };
    let fs_xml = format!(
        r#"<filesystem type='mount' accessmode='passthrough'>
  <driver type='virtiofs' queue='1024'/>
  <source dir='{}'/>
  <target dir='{}'/>{}
</filesystem>"#,
        crate::xml::escape(source_dir),
        crate::xml::escape(mount_tag),
        binary_xml
    );

    let xml2 = ensure_memory_backing_shared(&vm_xml);
    let xml3 = insert_filesystem_into_devices(&xml2, &fs_xml);
    Domain::define_xml(conn, &xml3).map_err(|e| {
        LibvirtError::Operation(format!("Failed to define VM with shared directory: {e}"))
    })?;
    Ok(())
}

pub fn remove_share(conn: &Connect, name: &str, mount_tag: &str) -> Result<(), LibvirtError> {
    let domain = lookup_domain(conn, name)?;
    let info = domain
        .get_info()
        .map_err(LibvirtError::map_op("get domain info"))?;
    if info.state == 1 {
        return Err(LibvirtError::Invalid(
            "VM must be shut off to remove a shared directory".into(),
        ));
    }

    let vm_xml = domain.get_xml_desc(0).unwrap_or_default();
    let xml2 = remove_filesystem_by_tag(&vm_xml, mount_tag)?;
    Domain::define_xml(conn, &xml2).map_err(|e| {
        LibvirtError::Operation(format!("Failed to define VM without shared directory: {e}"))
    })?;
    Ok(())
}
