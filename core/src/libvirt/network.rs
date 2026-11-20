// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;
use virt::network::Network;

use crate::state::NetworkInfo;
use crate::LibvirtError;

fn lookup_network(conn: &Connect, name: &str) -> Result<Network, LibvirtError> {
    Network::lookup_by_name(conn, name)
        .map_err(|e| LibvirtError::NotFound(format!("Network '{name}' not found: {e}")))
}

pub fn list_networks(conn: &Connect) -> Result<Vec<NetworkInfo>, LibvirtError> {
    let networks = conn
        .list_all_networks(0)
        .map_err(LibvirtError::map_op("Failed to list networks"))?;

    let mut result = Vec::new();
    for net in networks {
        let name = net
            .get_name()
            .map_err(LibvirtError::map_op("Failed to get network name"))?;

        result.push(NetworkInfo {
            name,
            uuid: net.get_uuid_string().unwrap_or_default(),
            active: net.is_active().unwrap_or(false),
            persistent: net.is_persistent().unwrap_or(false),
            autostart: net.get_autostart().unwrap_or(false),
            bridge: net.get_bridge_name().unwrap_or_default(),
        });
    }

    Ok(result)
}

pub fn start_network(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let net = lookup_network(conn, name)?;
    net.create()
        .map_err(|e| LibvirtError::Operation(format!("Failed to start network '{name}': {e}")))?;
    Ok(())
}

const DEFAULT_NETWORK_TEMPLATE: &str = "/usr/share/libvirt/networks/default.xml";

/// Re-define the packaged `default` NAT network when libvirt cannot create `virbr0` (stale state).
pub fn repair_default_network(conn: &Connect) -> Result<(), LibvirtError> {
    let xml = std::fs::read_to_string(DEFAULT_NETWORK_TEMPLATE)
        .map_err(|e| LibvirtError::Operation(format!("read {DEFAULT_NETWORK_TEMPLATE}: {e}")))?;
    if let Ok(net) = Network::lookup_by_name(conn, "default") {
        if net.is_active().unwrap_or(false) {
            let _ = net.destroy();
        }
        let _ = net.undefine();
    }
    Network::define_xml(conn, &xml)
        .map_err(|e| LibvirtError::Operation(format!("Failed to redefine default network: {e}")))?;
    let net = lookup_network(conn, "default")?;
    let _ = net.set_autostart(true);
    net.create().map_err(|e| {
        LibvirtError::Operation(format!("Failed to start repaired default network: {e}"))
    })?;
    Ok(())
}

/// Start a defined libvirt network if it is not already active (e.g. before starting a guest that uses it).
pub fn ensure_network_active(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let net = lookup_network(conn, name)?;
    if net.is_active().unwrap_or(false) {
        return Ok(());
    }
    match net.create() {
        Ok(_) => Ok(()),
        Err(e) if name == "default" && e.to_string().contains("File exists") => {
            repair_default_network(conn)
        }
        Err(e) => Err(LibvirtError::Operation(format!(
            "Failed to start network '{name}': {e}"
        ))),
    }
}

/// Start every persistent network marked autostart that is currently inactive (daemon boot helper).
pub fn bootstrap_autostart_networks(conn: &Connect) -> Vec<(String, String)> {
    let Ok(networks) = list_networks(conn) else {
        return Vec::new();
    };
    let mut failures = Vec::new();
    for n in networks {
        if n.autostart && !n.active {
            if let Err(e) = ensure_network_active(conn, &n.name) {
                failures.push((n.name, e.to_string()));
            }
        }
    }
    failures
}

pub fn stop_network(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let net = lookup_network(conn, name)?;
    net.destroy()
        .map_err(|e| LibvirtError::Operation(format!("Failed to stop network '{name}': {e}")))?;
    Ok(())
}

pub fn get_network_xml(conn: &Connect, name: &str) -> Result<String, LibvirtError> {
    let net = lookup_network(conn, name)?;
    net.get_xml_desc(0)
        .map_err(|e| LibvirtError::Operation(format!("get_xml network '{name}': {e}")))
}

/// Maximum accepted size for a network XML document (bytes).
const MAX_NETWORK_XML_BYTES: usize = 512 * 1024;

/// Extract `<name>...</name>` from a libvirt network XML document (first occurrence).
fn parse_network_name_from_xml(xml: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let key = "<name>";
    let start = lower.find(key)?;
    let rest = xml.get(start + key.len()..)?;
    let close = rest.to_ascii_lowercase().find("</name>")?;
    let inner = rest.get(..close)?.trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
}

/// Replace persistent network definition from XML. The `<name>` in XML must match `name`.
///
/// If the network was active, it is destroyed and started again so the running instance matches
/// the new definition (brief disconnect for attached guests).
pub fn update_network_xml(conn: &Connect, name: &str, xml: &str) -> Result<(), LibvirtError> {
    crate::validate::validate_name(name)?;

    let xml = xml.trim();
    if xml.is_empty() {
        return Err(LibvirtError::Invalid("Network XML is empty".into()));
    }
    if xml.len() > MAX_NETWORK_XML_BYTES {
        return Err(LibvirtError::Invalid(format!(
            "Network XML exceeds {} bytes",
            MAX_NETWORK_XML_BYTES
        )));
    }
    if !xml.to_ascii_lowercase().contains("<network") {
        return Err(LibvirtError::Invalid(
            "Network XML must contain a <network> root element".into(),
        ));
    }

    let xml_name = parse_network_name_from_xml(xml).ok_or_else(|| {
        LibvirtError::Invalid("Network XML must contain a <name>...</name> element".into())
    })?;
    if xml_name != name {
        return Err(LibvirtError::Invalid(format!(
            "XML <name> must match the network being edited (expected '{name}', got '{xml_name}')"
        )));
    }

    let net = lookup_network(conn, name)?;
    let was_active = net.is_active().unwrap_or(false);

    Network::define_xml(conn, xml).map_err(|e| {
        LibvirtError::Operation(format!("Failed to update network '{name}' definition: {e}"))
    })?;

    if was_active {
        let net = lookup_network(conn, name)?;
        net.destroy().map_err(|e| {
            LibvirtError::Operation(format!(
                "Updated definition but failed to stop network '{name}' to apply changes: {e}"
            ))
        })?;
        let net = lookup_network(conn, name)?;
        net.create().map_err(|e| {
            LibvirtError::Operation(format!(
                "Updated definition but failed to restart network '{name}': {e}. Start it manually from the UI."
            ))
        })?;
    }

    Ok(())
}

fn validate_ip(ip: &str, label: &str) -> Result<std::net::Ipv4Addr, LibvirtError> {
    ip.parse::<std::net::Ipv4Addr>().map_err(|_| {
        LibvirtError::Invalid(format!("Invalid {label}: '{ip}' (expected IPv4 address)"))
    })
}

fn validate_subnet_prefix(subnet: &str) -> Result<(), LibvirtError> {
    // Subnet should be like "192.168.100" (first 3 octets)
    let parts: Vec<&str> = subnet.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return Err(LibvirtError::Operation(format!(
            "Invalid subnet prefix: '{subnet}' (expected format: 192.168.100)"
        )));
    }
    Ok(())
}

pub fn create_network(
    conn: &Connect,
    name: &str,
    subnet: &str,
    dhcp_start: &str,
    dhcp_end: &str,
) -> Result<(), LibvirtError> {
    crate::validate::validate_name(name)?;
    validate_subnet_prefix(subnet)?;
    let start = validate_ip(dhcp_start, "DHCP start")?;
    let end = validate_ip(dhcp_end, "DHCP end")?;
    if u32::from(start) > u32::from(end) {
        return Err(LibvirtError::Invalid(format!(
            "DHCP start ({dhcp_start}) must not be greater than end ({dhcp_end})"
        )));
    }

    // Validate DHCP range is within the subnet
    let subnet_octets: Vec<u8> = subnet.split('.').filter_map(|p| p.parse().ok()).collect();
    if subnet_octets.len() == 3 {
        let start_octets = start.octets();
        let end_octets = end.octets();
        if start_octets[0] != subnet_octets[0]
            || start_octets[1] != subnet_octets[1]
            || start_octets[2] != subnet_octets[2]
        {
            return Err(LibvirtError::Invalid(format!(
                "DHCP start ({dhcp_start}) is not within subnet {subnet}.0/24"
            )));
        }
        if end_octets[0] != subnet_octets[0]
            || end_octets[1] != subnet_octets[1]
            || end_octets[2] != subnet_octets[2]
        {
            return Err(LibvirtError::Invalid(format!(
                "DHCP end ({dhcp_end}) is not within subnet {subnet}.0/24"
            )));
        }
    }

    let xml = format!(
        r#"<network>
  <name>{}</name>
  <forward mode='nat'/>
  <bridge stp='on' delay='0'/>
  <ip address='{}.1' netmask='255.255.255.0'>
    <dhcp>
      <range start='{}' end='{}'/>
    </dhcp>
  </ip>
</network>"#,
        crate::xml::escape(name),
        crate::xml::escape(subnet),
        crate::xml::escape(dhcp_start),
        crate::xml::escape(dhcp_end),
    );

    Network::define_xml(conn, &xml)
        .map_err(|e| LibvirtError::Operation(format!("Failed to create network '{name}': {e}")))?;

    Ok(())
}

pub fn delete_network(conn: &Connect, name: &str) -> Result<(), LibvirtError> {
    let net = lookup_network(conn, name)?;

    // A swallowed destroy() error here used to fall straight through to undefine():
    // undefine() on a still-active network only drops the persistent config and
    // leaves it running as a transient network, so the caller got a plain "deleted"
    // success while the network (and its dnsmasq/iptables state) kept running.
    if net.is_active().unwrap_or(false) {
        net.destroy().map_err(|e| {
            LibvirtError::Operation(format!("Failed to stop network '{name}': {e}"))
        })?;
    }

    net.undefine()
        .map_err(|e| LibvirtError::Operation(format!("Failed to delete network '{name}': {e}")))?;
    Ok(())
}

pub fn set_network_autostart(
    conn: &Connect,
    name: &str,
    autostart: bool,
) -> Result<(), LibvirtError> {
    let net = lookup_network(conn, name)?;
    net.set_autostart(autostart)
        .map_err(LibvirtError::map_op("Failed to set autostart"))?;
    if autostart && !net.is_active().unwrap_or(false) {
        net.create().map_err(|e| {
            LibvirtError::Operation(format!(
                "Failed to start network '{name}' after enabling autostart: {e}"
            ))
        })?;
    }
    Ok(())
}

fn valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `virsh net-update` arguments that add (or delete) a static DHCP host entry. Everything is validated first:
/// the values end up inside an XML fragment.
pub fn dhcp_host_args(network: &str, mac: &str, ip: &str, add: bool) -> Result<Vec<String>, LibvirtError> {
    crate::validate::validate_name(network)?;
    if !valid_mac(mac) {
        return Err(LibvirtError::Invalid(format!("'{mac}' is not a MAC address")));
    }
    let ip: std::net::Ipv4Addr = ip
        .parse()
        .map_err(|_| LibvirtError::Invalid(format!("'{ip}' is not an IPv4 address")))?;
    Ok(vec![
        "net-update".into(),
        network.into(),
        if add { "add" } else { "delete" }.into(),
        "ip-dhcp-host".into(),
        format!("<host mac='{}' ip='{ip}'/>", mac.to_ascii_lowercase()),
        "--live".into(),
        "--config".into(),
    ])
}

fn run_virsh(args: &[String]) -> Result<(), LibvirtError> {
    let out = std::process::Command::new("virsh")
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("virsh {}: {e}", args.first().map_or("", String::as_str))))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(LibvirtError::Operation(format!(
            "virsh {}: {}",
            args.first().map_or("", String::as_str),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Pin `mac` to `ip` on a libvirt network (or unpin). Pinning replaces an earlier entry for the same MAC or address, so
/// repeating the call is safe.
pub fn set_dhcp_host(network: &str, mac: &str, ip: &str, enabled: bool) -> Result<(), LibvirtError> {
    if enabled {
        // net-update refuses a duplicate: drop any previous entry first and ignore "not found".
        let _ = run_virsh(&dhcp_host_args(network, mac, ip, false)?);
        run_virsh(&dhcp_host_args(network, mac, ip, true)?)
    } else {
        run_virsh(&dhcp_host_args(network, mac, ip, false)?)
    }
}

#[cfg(test)]
mod dhcp_host_tests {
    use super::*;

    #[test]
    fn builds_the_update_for_add_and_delete() {
        let a = dhcp_host_args("mc-abc", "52:54:00:AA:BB:CC", "10.20.1.7", true).unwrap();
        assert_eq!(a[..4], ["net-update", "mc-abc", "add", "ip-dhcp-host"]);
        assert_eq!(a[4], "<host mac='52:54:00:aa:bb:cc' ip='10.20.1.7'/>");
        assert_eq!(a[5..], ["--live", "--config"]);
        assert_eq!(dhcp_host_args("mc-abc", "52:54:00:aa:bb:cc", "10.20.1.7", false).unwrap()[2], "delete");
    }

    #[test]
    fn nothing_that_could_break_out_of_the_xml_is_accepted() {
        assert!(dhcp_host_args("mc-abc", "52:54:00:aa:bb:c'", "10.20.1.7", true).is_err());
        assert!(dhcp_host_args("mc-abc", "52:54:00:aa:bb", "10.20.1.7", true).is_err());
        assert!(dhcp_host_args("mc-abc", "52:54:00:aa:bb:cc", "10.20.1.7' x='", true).is_err());
        assert!(dhcp_host_args("bad name;", "52:54:00:aa:bb:cc", "10.20.1.7", true).is_err());
        assert!(dhcp_host_args("mc-abc", "52:54:00:aa:bb:cc", "::1", true).is_err());
    }
}
