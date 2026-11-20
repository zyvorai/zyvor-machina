// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Resolve libvirt SPICE endpoints, mirroring the robustness of the VNC resolver
//! (`vnc.rs`). A plain XML scan only sees a numeric `port` when the guest was
//! given a TCP autoport listen; modern libvirt defaults to a **unix socket** and
//! TLS-only setups expose only `tlsPort` — in both cases the XML `port` is `-1`.
//! We therefore fall back to `virsh domdisplay`, which reports the live endpoint
//! (`spice://host:port` or `spice+unix:///path`) the same way `virsh vncdisplay`
//! backstops VNC.

use std::process::Command;

use crate::xml::{extract_attr, split_blocks};

/// A resolved SPICE server the console proxy can bridge a WebSocket to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpiceEndpoint {
    /// Plain-text SPICE over TCP (`host`, `port`).
    Tcp(String, u16),
    /// SPICE exposed on a local unix domain socket.
    Unix(String),
}

fn normalize_listen(listen: &str) -> String {
    match listen.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        "localhost" => "127.0.0.1".to_string(),
        h => h.to_string(),
    }
}

/// Extract a SPICE endpoint straight from a domain's (live) XML, if it exposes
/// one we can connect to without TLS. Prefers a unix socket, then a TCP port.
fn try_endpoint_from_spice_graphics_xml(xml: &str) -> Option<SpiceEndpoint> {
    for block in split_blocks(xml, "graphics") {
        if extract_attr(&block, "graphics", "type").as_deref() != Some("spice") {
            continue;
        }
        // A `<listen type='socket' socket='/path'>` (libvirt's modern default)
        // has no TCP port at all — bridge the unix socket directly.
        if extract_attr(&block, "listen", "type").as_deref() == Some("socket") {
            if let Some(path) = extract_attr(&block, "listen", "socket") {
                if !path.trim().is_empty() {
                    return Some(SpiceEndpoint::Unix(path.trim().to_string()));
                }
            }
        }
        // Some libvirt versions carry the socket path on the `<graphics>` element.
        if let Some(path) = extract_attr(&block, "graphics", "socket") {
            if !path.trim().is_empty() {
                return Some(SpiceEndpoint::Unix(path.trim().to_string()));
            }
        }
        let port: i32 = extract_attr(&block, "graphics", "port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        if port > 0 {
            let port = u16::try_from(port).ok()?;
            let listen = extract_attr(&block, "graphics", "listen")
                .or_else(|| extract_attr(&block, "listen", "address"))
                .unwrap_or_default();
            return Some(SpiceEndpoint::Tcp(normalize_listen(&listen), port));
        }
    }
    None
}

/// Parse one line of `virsh domdisplay` output into a SPICE endpoint.
/// Handles `spice://host:port`, `spice+unix:///path`, and bare `host:port`.
pub fn parse_domdisplay_line(line: &str) -> Option<SpiceEndpoint> {
    let s = line.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("spice+unix://") {
        let path = rest.trim();
        if !path.is_empty() {
            return Some(SpiceEndpoint::Unix(path.to_string()));
        }
        return None;
    }
    let rest = s.strip_prefix("spice://").unwrap_or(s);
    // Strip any URL path/query (e.g. `host:port/?tls-port=...`).
    let authority = rest.split(['/', '?']).next().unwrap_or(rest).trim();
    let (host_part, port_str) = authority.rsplit_once(':')?;
    let host = normalize_listen(host_part);
    let port: u16 = port_str.trim().parse().ok()?;
    if port == 0 {
        return None;
    }
    Some(SpiceEndpoint::Tcp(host, port))
}

fn endpoint_from_virsh(domain_name: &str) -> Option<SpiceEndpoint> {
    let out = Command::new("virsh")
        .args(["domdisplay", "--type", "spice", domain_name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // `virsh domdisplay` can print several URIs (one per graphics device); take
    // the first SPICE one we can turn into a connectable endpoint.
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().find_map(parse_domdisplay_line)
}

/// Resolve a connectable SPICE endpoint for `domain_name`, given its (live) XML.
///
/// Order mirrors the VNC resolver: trust the XML first, then fall back to
/// `virsh domdisplay` so autoport / unix-socket / TLS-only guests still resolve
/// instead of the proxy silently closing the socket ("Disconnected · SPICE").
pub fn resolve_spice_endpoint(domain_name: &str, xml: &str) -> Option<SpiceEndpoint> {
    if let Some(ep) = try_endpoint_from_spice_graphics_xml(xml) {
        return Some(ep);
    }
    endpoint_from_virsh(domain_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_tcp_autoport_port() {
        let xml = r#"<domain><devices>
            <graphics type='spice' port='5901' autoport='yes' listen='0.0.0.0'/>
        </devices></domain>"#;
        assert_eq!(
            resolve_spice_endpoint("vm", xml),
            Some(SpiceEndpoint::Tcp("127.0.0.1".into(), 5901))
        );
    }

    #[test]
    fn xml_unix_socket_listen() {
        let xml = r#"<domain><devices>
            <graphics type='spice' autoport='no'>
              <listen type='socket' socket='/var/lib/libvirt/qemu/domain-1-vm/spice.sock'/>
            </graphics>
        </devices></domain>"#;
        assert_eq!(
            try_endpoint_from_spice_graphics_xml(xml),
            Some(SpiceEndpoint::Unix(
                "/var/lib/libvirt/qemu/domain-1-vm/spice.sock".into()
            ))
        );
    }

    #[test]
    fn xml_tls_only_has_no_plain_port() {
        // port='-1' with only a tlsPort — not connectable in plain text, so the
        // XML scan yields nothing and the caller falls back to virsh.
        let xml = r#"<domain><devices>
            <graphics type='spice' port='-1' tlsPort='5902' autoport='yes' listen='127.0.0.1'/>
        </devices></domain>"#;
        assert_eq!(try_endpoint_from_spice_graphics_xml(xml), None);
    }

    #[test]
    fn domdisplay_tcp_uri() {
        assert_eq!(
            parse_domdisplay_line("spice://localhost:5900"),
            Some(SpiceEndpoint::Tcp("127.0.0.1".into(), 5900))
        );
    }

    #[test]
    fn domdisplay_unix_uri() {
        assert_eq!(
            parse_domdisplay_line("spice+unix:///run/spice.sock"),
            Some(SpiceEndpoint::Unix("/run/spice.sock".into()))
        );
    }

    #[test]
    fn domdisplay_uri_with_query() {
        assert_eq!(
            parse_domdisplay_line("spice://127.0.0.1:5900/?tls-port=5901"),
            Some(SpiceEndpoint::Tcp("127.0.0.1".into(), 5900))
        );
    }

    #[test]
    fn domdisplay_empty() {
        assert_eq!(parse_domdisplay_line("  "), None);
    }
}
