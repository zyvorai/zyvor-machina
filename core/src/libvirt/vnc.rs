// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Resolve libvirt VNC TCP endpoints (hyper2kvm-style `virsh vncdisplay` + domain XML).

use std::process::Command;

use crate::libvirt::domain;
use crate::xml::{extract_attr, split_blocks};
use crate::LibvirtError;
use virt::connect::Connect;

/// Parse one line of `virsh vncdisplay` output (`:N`, `host:N`, …) into `(host, tcp_port)`.
pub fn parse_vnc_display_line(line: &str) -> Option<(String, u16)> {
    let s = line.trim();
    if s.is_empty() {
        return None;
    }
    let (host_part, num_str) = s.rsplit_once(':')?;
    let host = if host_part.is_empty() {
        "127.0.0.1"
    } else {
        host_part.trim()
    };
    let host = match host {
        "localhost" => "127.0.0.1",
        h => h,
    };
    let n: u32 = num_str.trim().parse().ok()?;
    let port = 5900u32.checked_add(n)?;
    let port = u16::try_from(port).ok()?;
    Some((host.to_string(), port))
}

fn xml_has_graphics_type(xml: &str, want: &str) -> bool {
    for block in split_blocks(xml, "graphics") {
        if extract_attr(&block, "graphics", "type").as_deref() == Some(want) {
            return true;
        }
    }
    false
}

/// If domain XML already has a concrete VNC TCP port, return `(listen_host, port)`.
fn try_endpoint_from_vnc_graphics_xml(xml: &str) -> Option<(String, u16)> {
    for block in split_blocks(xml, "graphics") {
        if extract_attr(&block, "graphics", "type").as_deref()? != "vnc" {
            continue;
        }
        let port: i32 = extract_attr(&block, "graphics", "port")?.parse().ok()?;
        if port <= 0 {
            continue;
        }
        let port = u16::try_from(port).ok()?;
        let listen = extract_attr(&block, "graphics", "listen")
            .or_else(|| extract_attr(&block, "listen", "address"))
            .unwrap_or_default();
        let host = normalize_vnc_listen(&listen);
        return Some((host, port));
    }
    None
}

fn normalize_vnc_listen(listen: &str) -> String {
    match listen.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        h => h.to_string(),
    }
}

fn vnc_endpoint_from_virsh(domain_name: &str) -> Result<(String, u16), LibvirtError> {
    let out = Command::new("virsh")
        .args(["vncdisplay", domain_name])
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh vncdisplay: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LibvirtError::Operation(format!(
            "virsh vncdisplay failed: {stderr}"
        )));
    }
    let line = String::from_utf8_lossy(&out.stdout);
    parse_vnc_display_line(&line).ok_or_else(|| {
        LibvirtError::Operation("virsh vncdisplay returned no usable display".into())
    })
}

/// Resolve VNC using live domain XML, then `virsh vncdisplay` when the XML port is still `-1` / `0` (autoport).
pub fn resolve_vnc_tcp_xml(
    _conn: &Connect,
    domain_name: &str,
    xml: &str,
) -> Result<(String, u16), LibvirtError> {
    if !xml_has_graphics_type(xml, "vnc") {
        return Err(LibvirtError::Operation(
            "Domain has no VNC graphics device".into(),
        ));
    }
    if let Some(ep) = try_endpoint_from_vnc_graphics_xml(xml) {
        return Ok(ep);
    }
    vnc_endpoint_from_virsh(domain_name)
}

/// Same as [`resolve_vnc_tcp_xml`] but fetches domain XML first.
pub fn resolve_vnc_tcp(conn: &Connect, domain_name: &str) -> Result<(String, u16), LibvirtError> {
    let xml = domain::get_vm_xml(conn, domain_name)?;
    resolve_vnc_tcp_xml(conn, domain_name, &xml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_colon_zero() {
        let (h, p) = parse_vnc_display_line(":0").unwrap();
        assert_eq!(h, "127.0.0.1");
        assert_eq!(p, 5900);
    }

    #[test]
    fn parse_colon_one() {
        let (h, p) = parse_vnc_display_line(":1").unwrap();
        assert_eq!(h, "127.0.0.1");
        assert_eq!(p, 5901);
    }

    #[test]
    fn parse_host_with_display() {
        let (h, p) = parse_vnc_display_line("192.168.1.10:2").unwrap();
        assert_eq!(h, "192.168.1.10");
        assert_eq!(p, 5902);
    }

    #[test]
    fn parse_localhost() {
        let (h, p) = parse_vnc_display_line("localhost:0").unwrap();
        assert_eq!(h, "127.0.0.1");
        assert_eq!(p, 5900);
    }

    #[test]
    fn parse_empty_none() {
        assert!(parse_vnc_display_line("").is_none());
        assert!(parse_vnc_display_line("   ").is_none());
    }

    #[test]
    fn try_from_xml_positive_port() {
        let xml = r#"<domain>
  <devices>
    <graphics type='vnc' port='5905' listen='127.0.0.1'/>
  </devices>
</domain>"#;
        let ep = try_endpoint_from_vnc_graphics_xml(xml).unwrap();
        assert_eq!(ep.0, "127.0.0.1");
        assert_eq!(ep.1, 5905);
    }

    #[test]
    fn try_from_xml_autoport_skipped() {
        let xml = r#"<graphics type='vnc' port='-1' autoport='yes' listen='0.0.0.0'/>"#;
        assert!(try_endpoint_from_vnc_graphics_xml(xml).is_none());
    }

    #[test]
    fn listen_child_address() {
        let xml = r#"<graphics type='vnc' port='5900' autoport='yes'>
      <listen type='address' address='0.0.0.0'/>
    </graphics>"#;
        let ep = try_endpoint_from_vnc_graphics_xml(xml).unwrap();
        assert_eq!(ep.0, "127.0.0.1");
        assert_eq!(ep.1, 5900);
    }
}
