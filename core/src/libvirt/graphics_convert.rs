// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Domain graphics via `virt-xml` (Cockpit Machines parity).
//!
//! `virt-xml` has mutually exclusive action modes: `--edit`, `--add-device`, and
//! `--remove-device` must not be combined. See `VirtXmlAction`.

use std::process::Command;

use crate::LibvirtError;

/// `virt-xml` action mode (only one per invocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VirtXmlAction {
    Edit,
    AddDevice,
    RemoveDevice,
}

/// Build argv for a `virt-xml` invocation (testable without running the binary).
fn virt_xml_argv(uri: &str, vm_name: &str, action: VirtXmlAction, args: &[&str]) -> Vec<String> {
    let mut argv = vec!["-c".to_string(), uri.to_string(), vm_name.to_string()];
    if action == VirtXmlAction::Edit {
        argv.push("--edit".to_string());
    }
    argv.extend(args.iter().map(|s| (*s).to_string()));
    argv
}

fn run_virt_xml(
    uri: &str,
    vm_name: &str,
    action: VirtXmlAction,
    args: &[&str],
) -> Result<String, LibvirtError> {
    let argv = virt_xml_argv(uri, vm_name, action, args);
    let mut cmd = Command::new("virt-xml");
    for a in &argv {
        cmd.arg(a);
    }
    let out = cmd
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virt-xml spawn failed: {e}")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(LibvirtError::Operation(format!(
        "virt-xml failed: {stderr}"
    )))
}

/// Emit one or two `<graphics/>` elements for domain XML templates.
pub fn graphics_elements_xml(listen: &str, graphics_type: &str) -> String {
    let listen_esc = crate::xml::escape(listen);
    let gt = graphics_type.trim().to_ascii_lowercase();
    let want_vnc = gt == "vnc" || gt == "both";
    let want_spice = gt == "spice" || gt == "both";
    let mut out = Vec::new();
    if want_vnc {
        out.push(format!(
            "<graphics type='vnc' port='-1' autoport='yes' listen='{listen_esc}'/>"
        ));
    }
    if want_spice {
        if super::create::has_spice() {
            out.push(format!(
                "<graphics type='spice' port='-1' autoport='yes' listen='{listen_esc}'/>"
            ));
        } else if !want_vnc {
            out.push(format!(
                "<graphics type='vnc' port='-1' autoport='yes' listen='{listen_esc}'/>"
            ));
        }
    }
    if out.is_empty() {
        out.push(format!(
            "<graphics type='vnc' port='-1' autoport='yes' listen='{listen_esc}'/>"
        ));
    }
    out.join("\n    ")
}

/// Splice a default VNC `<graphics>` device into domain XML that has none.
///
/// Every VM-creation path in this codebase (`create.rs`, `virt_install.rs`,
/// `translate::domain_xml`) already defaults to VNC, so a fresh VM can never
/// end up headless. Clone/snapshot-clone paths are different: they copy the
/// *source* domain's XML verbatim, so a VM originally defined outside those
/// paths (e.g. a manual `virsh define`/`virt-install --graphics none` run
/// directly on the host) without a display device silently propagates that
/// gap to every clone. Call this after cloning so the copy always has a
/// console option beyond serial.
pub fn ensure_graphics_present(xml: &str, listen: &str) -> String {
    if xml.contains("<graphics ") || xml.contains("<graphics>") {
        return xml.to_string();
    }
    let Some(pos) = xml.rfind("</devices>") else {
        return xml.to_string();
    };
    let mut out = String::with_capacity(xml.len() + listen.len() + 96);
    out.push_str(&xml[..pos]);
    out.push_str("    ");
    out.push_str(&graphics_elements_xml(listen, "vnc"));
    out.push('\n');
    out.push_str(&xml[pos..]);
    out
}

/// True when active or inactive domain XML contains a SPICE graphics device.
pub fn domain_has_spice_graphics(xml: &str) -> bool {
    for block in crate::xml::split_blocks(xml, "graphics") {
        if crate::xml::extract_attr(&block, "graphics", "type").as_deref() == Some("spice") {
            return true;
        }
    }
    false
}

/// True when domain XML contains a VNC graphics device.
pub fn domain_has_vnc_graphics(xml: &str) -> bool {
    for block in crate::xml::split_blocks(xml, "graphics") {
        if crate::xml::extract_attr(&block, "graphics", "type").as_deref() == Some("vnc") {
            return true;
        }
    }
    false
}

fn dump_domain_xml(libvirt_uri: &str, vm_name: &str) -> Result<String, LibvirtError> {
    let out = Command::new("virsh")
        .args(["-c", libvirt_uri, "dumpxml", vm_name])
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh dumpxml spawn failed: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "virsh dumpxml failed: {stderr}"
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn is_unsupported_convert_to_vnc_flag(err: &LibvirtError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("unrecognized arguments: --convert-to-vnc")
        || msg.contains("unrecognized argument: --convert-to-vnc")
        || msg.contains("unrecognized arguments: '--convert-to-vnc'")
}

/// Convert SPICE graphics to VNC.
///
/// Newer `virt-install` builds expose `--convert-to-vnc`. Ubuntu/virtinst 4.x
/// does not, so fall back to remove-SPICE + ensure-VNC via portable virt-xml
/// device ops (and no-op when the domain is already VNC-only).
pub fn virt_xml_convert_spice_to_vnc(
    libvirt_uri: &str,
    vm_name: &str,
) -> Result<String, LibvirtError> {
    match run_virt_xml(
        libvirt_uri,
        vm_name,
        VirtXmlAction::Edit,
        &["--convert-to-vnc"],
    ) {
        Ok(msg) => return Ok(msg),
        Err(e) if is_unsupported_convert_to_vnc_flag(&e) => {}
        Err(e) => return Err(e),
    }

    let xml = dump_domain_xml(libvirt_uri, vm_name)?;
    if !domain_has_spice_graphics(&xml) {
        return Ok("No SPICE graphics present; domain already uses VNC or has no display".into());
    }

    let remove_msg = run_virt_xml(
        libvirt_uri,
        vm_name,
        VirtXmlAction::RemoveDevice,
        &["--remove-device", "--graphics", "type=spice"],
    )?;

    let xml_after = dump_domain_xml(libvirt_uri, vm_name)?;
    if domain_has_vnc_graphics(&xml_after) {
        return Ok(remove_msg);
    }

    let listen = "127.0.0.1";
    let add_msg = virt_xml_add_graphics(libvirt_uri, vm_name, "vnc", listen)?;
    Ok(format!("{remove_msg}\n{add_msg}"))
}

pub fn virt_xml_add_graphics(
    libvirt_uri: &str,
    vm_name: &str,
    graphics_type: &str,
    listen: &str,
) -> Result<String, LibvirtError> {
    let graphics_type = graphics_type.trim().to_ascii_lowercase();
    crate::validate::validate_graphics_kind(&graphics_type)?;
    crate::validate::validate_graphics_listen(listen)?;
    // `port=-1` already makes libvirt allocate a port automatically (autoport='yes' in
    // the resulting XML). Do NOT also pass `autoport=yes`: it is not a valid --graphics
    // suboption in virt-install/virt-xml 4.x (accepts type/listen/port/tlsPort/address.*)
    // and makes the whole command fail with "Unknown --graphics options: ['autoport']",
    // which breaks VM create whenever a second graphics device is added.
    let spec = format!("type={graphics_type},listen={listen},port=-1");
    run_virt_xml(
        libvirt_uri,
        vm_name,
        VirtXmlAction::AddDevice,
        &["--add-device", "--graphics", &spec],
    )
}

pub fn virt_xml_remove_graphics(
    libvirt_uri: &str,
    vm_name: &str,
    graphics_type: &str,
) -> Result<String, LibvirtError> {
    let graphics_type = graphics_type.trim().to_ascii_lowercase();
    crate::validate::validate_graphics_kind(&graphics_type)?;
    let spec = format!("type={graphics_type}");
    run_virt_xml(
        libvirt_uri,
        vm_name,
        VirtXmlAction::RemoveDevice,
        &["--remove-device", "--graphics", &spec],
    )
}

#[cfg(test)]
mod tests {
    use super::{virt_xml_argv, VirtXmlAction};

    #[test]
    fn convert_to_vnc_uses_edit_mode() {
        let argv = virt_xml_argv(
            "qemu:///system",
            "vm1",
            VirtXmlAction::Edit,
            &["--convert-to-vnc"],
        );
        assert_eq!(
            argv,
            vec!["-c", "qemu:///system", "vm1", "--edit", "--convert-to-vnc",]
        );
    }

    #[test]
    fn add_graphics_omits_edit() {
        let argv = virt_xml_argv(
            "qemu:///system",
            "vm1",
            VirtXmlAction::AddDevice,
            &[
                "--add-device",
                "--graphics",
                "type=spice,listen=127.0.0.1,port=-1",
            ],
        );
        assert!(!argv.contains(&"--edit".to_string()));
        assert_eq!(argv[3], "--add-device");
    }

    #[test]
    fn remove_graphics_omits_edit() {
        let argv = virt_xml_argv(
            "qemu:///system",
            "vm1",
            VirtXmlAction::RemoveDevice,
            &["--remove-device", "--graphics", "type=vnc"],
        );
        assert!(!argv.contains(&"--edit".to_string()));
        assert_eq!(argv[3], "--remove-device");
    }

    #[test]
    fn domain_has_spice_graphics_detects_spice() {
        let xml = "<domain><graphics type='spice' listen='127.0.0.1'/></domain>";
        assert!(super::domain_has_spice_graphics(xml));
        assert!(!super::domain_has_spice_graphics(
            "<domain><graphics type='vnc' listen='127.0.0.1'/></domain>"
        ));
        assert!(super::domain_has_vnc_graphics(
            "<domain><graphics type='vnc' listen='127.0.0.1'/></domain>"
        ));
        assert!(!super::domain_has_vnc_graphics(xml));
    }

    #[test]
    fn unsupported_convert_flag_detection() {
        let err = crate::LibvirtError::Operation(
            "virt-xml failed: usage: virt-xml [options]\nvirt-xml: error: unrecognized arguments: --convert-to-vnc\n"
                .into(),
        );
        assert!(super::is_unsupported_convert_to_vnc_flag(&err));
    }

    #[test]
    fn ensure_graphics_present_injects_vnc_when_missing() {
        let xml = "<domain>\n  <devices>\n    <disk/>\n  </devices>\n</domain>";
        let out = super::ensure_graphics_present(xml, "127.0.0.1");
        assert!(out.contains("<graphics type='vnc'"));
        assert!(out.contains("<disk/>"));
        assert!(out.find("<graphics").unwrap() < out.find("</devices>").unwrap());
    }

    #[test]
    fn ensure_graphics_present_leaves_existing_graphics_untouched() {
        let xml = "<domain>\n  <devices>\n    <graphics type='spice' listen='127.0.0.1'/>\n  </devices>\n</domain>";
        assert_eq!(super::ensure_graphics_present(xml, "127.0.0.1"), xml);
    }

    #[test]
    fn ensure_graphics_present_is_a_noop_without_a_devices_close_tag() {
        let xml = "<domain><devices><disk/>";
        assert_eq!(super::ensure_graphics_present(xml, "127.0.0.1"), xml);
    }
}
