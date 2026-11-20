// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use crate::LibvirtError;

// ── Host interface listing ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInterface {
    pub name: String,
    pub iface_type: String, // physical, bridge, bond, vlan, virtual
    pub state: String,      // up, down
    pub mac: String,
    pub ipv4: Vec<String>,
    pub mtu: u32,
    pub master: String, // bridge/bond master if enslaved
}

/// List all host network interfaces.
pub fn list_host_interfaces() -> Result<Vec<HostInterface>, LibvirtError> {
    let output = Command::new(find_bin("ip"))
        .args(["-j", "addr", "show"])
        .output()
        .map_err(LibvirtError::map_op("Failed to run ip addr"))?;

    if !output.status.success() {
        return Err(LibvirtError::Operation("ip addr failed".to_string()));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| LibvirtError::Operation(format!("Failed to parse ip output: {e}")))?;

    let mut interfaces = Vec::new();
    if let Some(arr) = json.as_array() {
        for iface in arr {
            let name = iface["ifname"].as_str().unwrap_or("").to_string();
            if name == "lo" {
                continue;
            }

            let link_type = iface["link_type"].as_str().unwrap_or("");
            let iface_type = classify_interface(&name, link_type, iface);
            let state = if iface["operstate"].as_str().unwrap_or("") == "UP" {
                "up"
            } else {
                "down"
            };
            let mac = iface["address"].as_str().unwrap_or("").to_string();
            let mtu = iface["mtu"].as_u64().unwrap_or(1500) as u32;
            let master = iface["master"].as_str().unwrap_or("").to_string();

            let mut ipv4 = Vec::new();
            if let Some(addrs) = iface["addr_info"].as_array() {
                for addr in addrs {
                    if addr["family"].as_str() == Some("inet") {
                        if let (Some(ip), Some(prefix)) =
                            (addr["local"].as_str(), addr["prefixlen"].as_u64())
                        {
                            ipv4.push(format!("{ip}/{prefix}"));
                        }
                    }
                }
            }

            interfaces.push(HostInterface {
                name,
                iface_type: iface_type.to_string(),
                state: state.to_string(),
                mac,
                ipv4,
                mtu,
                master,
            });
        }
    }

    Ok(interfaces)
}

fn classify_interface(name: &str, link_type: &str, iface: &serde_json::Value) -> &'static str {
    if link_type == "bridge" {
        return "bridge";
    }
    if link_type == "bond" {
        return "bond";
    }
    if name.contains('.') || iface.get("link").is_some() {
        return "vlan";
    }
    if name.starts_with("vnet")
        || name.starts_with("tap")
        || name.starts_with("veth")
        || name.starts_with("virbr")
        || name.starts_with("docker")
    {
        return "virtual";
    }
    "physical"
}

// ── Bridge management ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBridgeRequest {
    pub name: String,
    #[serde(default)]
    pub interfaces: Vec<String>, // physical NICs to add
    #[serde(default = "default_mtu")]
    pub mtu: u32,
    #[serde(default = "default_stp")]
    pub stp: bool,
}

fn default_mtu() -> u32 {
    1500
}
fn default_stp() -> bool {
    true
}

/// Detect the network management backend on this system.
fn detect_network_backend() -> &'static str {
    // Check for netplan (Ubuntu/Debian with systemd-networkd)
    if Path::new("/usr/sbin/netplan").exists() || Path::new("/usr/bin/netplan").exists() {
        return "netplan";
    }
    // Check for NetworkManager (Fedora/RHEL)
    if Command::new(find_bin("nmcli"))
        .arg("--version")
        .output()
        .is_ok()
    {
        return "nmcli";
    }
    // Fallback: raw ip commands
    "ip"
}

/// Detect the firewall backend (delegates to Zeus Firewall module).
fn detect_firewall_backend() -> &'static str {
    crate::firewall::detect_backend().as_str()
}

pub fn create_bridge(req: &CreateBridgeRequest) -> Result<(), LibvirtError> {
    crate::validate::validate_name(&req.name)?;
    if req.mtu < 68 || req.mtu > 9216 {
        return Err(LibvirtError::Invalid(
            "MTU must be between 68 and 9216".to_string(),
        ));
    }

    // Validate interface names
    for iface in &req.interfaces {
        if !iface
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(LibvirtError::Invalid(format!(
                "Invalid interface name: {iface}"
            )));
        }
    }

    // Auto-detect network backend
    match detect_network_backend() {
        "netplan" => create_bridge_netplan(req),
        "nmcli" => create_bridge_nmcli(req),
        _ => create_bridge_ip(req),
    }
}

/// Create bridge via netplan (Ubuntu with systemd-networkd).
fn create_bridge_netplan(req: &CreateBridgeRequest) -> Result<(), LibvirtError> {
    let interfaces_yaml = if req.interfaces.is_empty() {
        String::from("        interfaces: []")
    } else {
        let ifaces: Vec<String> = req
            .interfaces
            .iter()
            .map(|i| format!("          - {i}"))
            .collect();
        format!("        interfaces:\n{}", ifaces.join("\n"))
    };

    let stp_val = if req.stp { "true" } else { "false" };
    let yaml = format!(
        r#"network:
  version: 2
  bridges:
    {name}:
      mtu: {mtu}
{interfaces}
      parameters:
        stp: {stp}
      dhcp4: false
"#,
        name = req.name,
        mtu = req.mtu,
        interfaces = interfaces_yaml,
        stp = stp_val,
    );

    let config_path = format!("/etc/netplan/90-machina-{}.yaml", req.name);
    std::fs::write(&config_path, &yaml)
        .map_err(|e| LibvirtError::Operation(format!("Failed to write netplan config: {e}")))?;

    // Apply netplan
    let netplan_bin = find_bin("netplan");
    run_cmd(&netplan_bin, &["apply"], "Failed to apply netplan")?;

    Ok(())
}

fn create_bridge_nmcli(req: &CreateBridgeRequest) -> Result<(), LibvirtError> {
    let stp = if req.stp { "yes" } else { "no" };

    run_cmd(
        "nmcli",
        &[
            "connection",
            "add",
            "type",
            "bridge",
            "ifname",
            &req.name,
            "con-name",
            &req.name,
            "bridge.stp",
            stp,
            "802-3-ethernet.mtu",
            &req.mtu.to_string(),
        ],
        "Failed to create bridge",
    )?;

    // Bring it up
    let _ = run_cmd("nmcli", &["connection", "up", &req.name], "Bridge up");

    // Add slave interfaces
    for (i, iface) in req.interfaces.iter().enumerate() {
        let slave_name = format!("{}-port{}", req.name, i);
        run_cmd(
            "nmcli",
            &[
                "connection",
                "add",
                "type",
                "bridge-slave",
                "ifname",
                iface,
                "con-name",
                &slave_name,
                "master",
                &req.name,
            ],
            &format!("Failed to add {iface} to bridge"),
        )?;
        let _ = run_cmd("nmcli", &["connection", "up", &slave_name], "Slave up");
    }

    Ok(())
}

fn create_bridge_ip(req: &CreateBridgeRequest) -> Result<(), LibvirtError> {
    run_cmd(
        "ip",
        &["link", "add", "name", &req.name, "type", "bridge"],
        "Failed to create bridge",
    )?;
    run_cmd(
        "ip",
        &["link", "set", &req.name, "mtu", &req.mtu.to_string()],
        "MTU set",
    )?;

    let stp_val = if req.stp { "1" } else { "0" };
    let stp_path = format!("/sys/class/net/{}/bridge/stp_state", req.name);
    let _ = std::fs::write(&stp_path, stp_val);

    for iface in &req.interfaces {
        run_cmd(
            "ip",
            &["link", "set", iface, "master", &req.name],
            &format!("Failed to add {iface}"),
        )?;
    }

    run_cmd(
        "ip",
        &["link", "set", &req.name, "up"],
        "Failed to bring bridge up",
    )?;
    Ok(())
}

pub fn delete_bridge(name: &str) -> Result<(), LibvirtError> {
    crate::validate::validate_name(name)?;

    if detect_network_backend() == "netplan" {
        // Remove netplan config and apply
        let config_path = format!("/etc/netplan/90-machina-{name}.yaml");
        let _ = std::fs::remove_file(&config_path);
        let netplan_bin = find_bin("netplan");
        run_cmd(&netplan_bin, &["apply"], "Failed to apply netplan")?;
        return Ok(());
    }

    if Command::new(find_bin("nmcli"))
        .arg("--version")
        .output()
        .is_ok()
    {
        // Delete all slave connections first
        if let Ok(output) = Command::new(find_bin("nmcli"))
            .args(["-t", "-f", "NAME,DEVICE", "connection", "show"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(&format!("{name}-port")) {
                    let con_name = line.split(':').next().unwrap_or("");
                    let _ = run_cmd("nmcli", &["connection", "delete", con_name], "delete slave");
                }
            }
        }
        run_cmd(
            "nmcli",
            &["connection", "delete", name],
            "Failed to delete bridge",
        )
    } else {
        run_cmd("ip", &["link", "set", name, "down"], "bridge down")?;
        run_cmd(
            "ip",
            &["link", "delete", name, "type", "bridge"],
            "Failed to delete bridge",
        )
    }
}

// ── Port forwarding ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardRule {
    pub id: String,
    pub protocol: String, // tcp, udp
    pub host_port: u16,
    pub vm_ip: String,
    pub vm_port: u16,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePortForwardRequest {
    pub protocol: String,
    pub host_port: u16,
    pub vm_ip: String,
    pub vm_port: u16,
    #[serde(default)]
    pub description: String,
}

pub fn list_port_forwards() -> Result<Vec<PortForwardRule>, LibvirtError> {
    // Parse iptables PREROUTING DNAT rules with our comment marker
    let output = Command::new(find_bin("iptables"))
        .args([
            "-t",
            "nat",
            "-L",
            "PREROUTING",
            "-n",
            "--line-numbers",
            "-v",
        ])
        .output()
        .map_err(LibvirtError::map_op("Failed to list iptables rules"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rules = Vec::new();

    for line in stdout.lines() {
        if !line.contains("machina:") {
            continue;
        }

        // Parse: num ... tcp dpt:HOST_PORT ... to:VM_IP:VM_PORT /* machina:desc */
        let proto = if line.contains(" tcp ") {
            "tcp"
        } else if line.contains(" udp ") {
            "udp"
        } else {
            continue;
        };

        let host_port = extract_dpt(line).unwrap_or(0);
        let (vm_ip, vm_port) = extract_dnat_target(line).unwrap_or_default();
        let desc = extract_comment(line, "machina:").unwrap_or_default();
        let id = format!("{proto}-{host_port}-{vm_ip}-{vm_port}");

        if host_port > 0 && vm_port > 0 {
            rules.push(PortForwardRule {
                id,
                protocol: proto.to_string(),
                host_port,
                vm_ip,
                vm_port,
                description: desc,
            });
        }
    }

    Ok(rules)
}

/// Get the detected network/firewall backends for display in the UI.
pub fn get_detected_backends() -> (String, String) {
    (
        detect_network_backend().to_string(),
        detect_firewall_backend().to_string(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdNetworkDiagnostics {
    pub systemd_networkd_active: bool,
    pub network_manager_active: bool,
    pub networkctl_list: String,
    pub networkctl_status_all: String,
    pub resolvectl_status: String,
    pub resolvectl_statistics: String,
    pub networkd_recent_logs: String,
    pub resolved_recent_logs: String,
}

/// Kernel IPv4/IPv6 routing tables (`ip route …`), read-only for operator visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostRoutingTables {
    pub ipv4: String,
    pub ipv6: String,
}

/// Runs `ip route show table all` and `ip -6 route show table all`. IPv6 is best-effort if the stack errors.
pub fn get_host_routing_tables() -> Result<HostRoutingTables, LibvirtError> {
    let ip = find_bin("ip");
    let ipv4 = run_capture(
        &ip,
        &["route", "show", "table", "all"],
        "Failed to run ip route show table all",
    )?;
    let ipv6 = run_capture_soft(&ip, &["-6", "route", "show", "table", "all"]);
    Ok(HostRoutingTables { ipv4, ipv6 })
}

/// Add or delete a single kernel static route (`ip route …`). Linux only; validated tokens only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelRouteChangeRequest {
    /// `ipv4` or `ipv6`
    pub family: String,
    /// `add` or `delete`
    pub operation: String,
    /// `default` or a CIDR (e.g. `192.168.10.0/24`, `2001:db8::/64`).
    pub destination: String,
    #[serde(default)]
    pub via: Option<String>,
    #[serde(default)]
    pub dev: Option<String>,
    #[serde(default)]
    pub table: Option<u32>,
}

fn validate_route_iface(dev: &str) -> Result<(), LibvirtError> {
    if dev.is_empty()
        || dev.len() > 32
        || !dev
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
    {
        return Err(LibvirtError::Invalid(
            "invalid interface name for route".into(),
        ));
    }
    Ok(())
}

fn valid_ipv4_route_dest(dest: &str) -> bool {
    if dest == "default" {
        return true;
    }
    let parts: Vec<&str> = dest.splitn(2, '/').collect();
    if parts.len() == 2 {
        if let (Ok(_addr), Ok(plen)) = (
            parts[0].parse::<std::net::Ipv4Addr>(),
            parts[1].parse::<u8>(),
        ) {
            return plen <= 32;
        }
        false
    } else {
        parts[0].parse::<std::net::Ipv4Addr>().is_ok()
    }
}

fn valid_ipv6_route_dest(dest: &str) -> bool {
    if dest == "default" {
        return true;
    }
    let parts: Vec<&str> = dest.splitn(2, '/').collect();
    if parts.len() == 2 {
        if let (Ok(_addr), Ok(plen)) = (
            parts[0].parse::<std::net::Ipv6Addr>(),
            parts[1].parse::<u8>(),
        ) {
            return plen <= 128;
        }
        false
    } else {
        parts[0].parse::<std::net::Ipv6Addr>().is_ok()
    }
}

pub fn modify_kernel_route(req: &KernelRouteChangeRequest) -> Result<(), LibvirtError> {
    let fam = req.family.to_lowercase();
    if fam != "ipv4" && fam != "ipv6" {
        return Err(LibvirtError::Invalid("family must be ipv4 or ipv6".into()));
    }
    let op = req.operation.to_lowercase();
    if op != "add" && op != "delete" {
        return Err(LibvirtError::Invalid(
            "operation must be add or delete".into(),
        ));
    }
    let dest = req.destination.trim();
    if dest.is_empty() || dest.len() > 128 {
        return Err(LibvirtError::Invalid("invalid destination".into()));
    }
    if fam == "ipv4" && !valid_ipv4_route_dest(dest) {
        return Err(LibvirtError::Invalid(
            "invalid IPv4 route destination".into(),
        ));
    }
    if fam == "ipv6" && !valid_ipv6_route_dest(dest) {
        return Err(LibvirtError::Invalid(
            "invalid IPv6 route destination".into(),
        ));
    }

    if let Some(ref d) = req.dev {
        validate_route_iface(d)?;
    }
    if let Some(ref v) = req.via {
        if v.len() > 64 || v.trim().is_empty() {
            return Err(LibvirtError::Invalid("invalid via gateway".into()));
        }
        let vtrim = v.trim();
        if fam == "ipv4" && vtrim.parse::<std::net::Ipv4Addr>().is_err() {
            return Err(LibvirtError::Invalid(
                "via must be a valid IPv4 address".into(),
            ));
        }
        if fam == "ipv6" && vtrim.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(LibvirtError::Invalid(
                "via must be a valid IPv6 address".into(),
            ));
        }
    }
    let ip = find_bin("ip");
    let mut args: Vec<String> = Vec::new();
    if fam == "ipv6" {
        args.push("-6".into());
    }
    args.push("route".into());
    args.push(op.clone());
    if dest == "default" {
        args.push("default".into());
    } else {
        args.push(dest.to_string());
    }
    if let Some(ref v) = req.via {
        args.push("via".into());
        args.push(v.trim().to_string());
    }
    if let Some(ref d) = req.dev {
        args.push("dev".into());
        args.push(d.clone());
    }
    if let Some(t) = req.table {
        args.push("table".into());
        args.push(t.to_string());
    }

    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_cmd(&ip, &argv, "ip route")
}

pub fn get_systemd_network_diagnostics() -> Result<SystemdNetworkDiagnostics, LibvirtError> {
    let systemd_networkd_active = is_service_active("systemd-networkd").unwrap_or(false);
    let network_manager_active = is_service_active("NetworkManager").unwrap_or(false);

    let networkctl_list = run_capture(
        &find_bin("networkctl"),
        &["list"],
        "Failed to run networkctl list",
    )?;
    let networkctl_status_all = run_capture(
        &find_bin("networkctl"),
        &["status", "--all"],
        "Failed to run networkctl status --all",
    )?;
    let resolvectl_status = run_capture(
        &find_bin("resolvectl"),
        &["status"],
        "Failed to run resolvectl status",
    )?;
    let resolvectl_statistics = run_capture(
        &find_bin("resolvectl"),
        &["statistics"],
        "Failed to run resolvectl statistics",
    )?;
    let networkd_recent_logs = run_capture(
        &find_bin("journalctl"),
        &[
            "-u",
            "systemd-networkd",
            "--since",
            "5 minutes ago",
            "--no-pager",
            "-n",
            "120",
        ],
        "Failed to read systemd-networkd logs",
    )?;
    let resolved_recent_logs = run_capture(
        &find_bin("journalctl"),
        &[
            "-u",
            "systemd-resolved",
            "--since",
            "5 minutes ago",
            "--no-pager",
            "-n",
            "120",
        ],
        "Failed to read systemd-resolved logs",
    )?;

    Ok(SystemdNetworkDiagnostics {
        systemd_networkd_active,
        network_manager_active,
        networkctl_list,
        networkctl_status_all,
        resolvectl_status,
        resolvectl_statistics,
        networkd_recent_logs,
        resolved_recent_logs,
    })
}

// ── LLDP (systemd-networkd / NetworkManager) ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LldpNeighbor {
    pub local_interface: String,
    pub chassis_id: String,
    pub system_name: String,
    pub port_id: String,
    pub port_description: String,
    pub system_description: String,
    pub capabilities: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LldpInventory {
    /// `systemd_networkd`, `network_manager`, or `none`
    pub source: String,
    pub neighbors: Vec<LldpNeighbor>,
    pub raw_text: String,
    pub summary: String,
}

/// Gather LLDP neighbors from systemd-networkd (`networkctl lldp`) or NetworkManager (`nmcli device lldp list`).
pub fn gather_lldp_neighbors() -> LldpInventory {
    let networkd_active = is_service_active("systemd-networkd").unwrap_or(false);
    let nm_active = is_service_active("NetworkManager").unwrap_or(false);

    if networkd_active {
        if let Ok(inv) = gather_lldp_from_networkctl() {
            if !inv.neighbors.is_empty() {
                return inv;
            }
        }
        if let Ok(inv) = gather_lldp_from_systemd_json_dir() {
            if !inv.neighbors.is_empty() {
                return inv;
            }
        }
    }

    if nm_active {
        if let Ok(inv) = gather_lldp_from_nmcli() {
            if !inv.neighbors.is_empty() {
                return inv;
            }
        }
    }

    let hint = if networkd_active {
        "No LLDP neighbors yet — set LLDP=yes in .network files or wait for switch advertisements."
    } else if nm_active {
        "No LLDP neighbors — set connection.lldp=on in NetworkManager profiles."
    } else {
        "Neither systemd-networkd nor NetworkManager is active for LLDP collection."
    };

    LldpInventory {
        source: "none".into(),
        neighbors: vec![],
        raw_text: String::new(),
        summary: hint.into(),
    }
}

fn gather_lldp_from_networkctl() -> Result<LldpInventory, LibvirtError> {
    let networkctl = find_bin("networkctl");
    let raw = run_capture_soft(&networkctl, &["lldp"]);
    if raw.contains("(failed to run") || raw.contains("(exit ") {
        return Err(LibvirtError::Operation(
            "networkctl lldp unavailable".into(),
        ));
    }
    let neighbors = parse_networkctl_lldp_text(&raw);
    let summary = if neighbors.is_empty() {
        "systemd-networkd LLDP — no neighbors advertised".into()
    } else {
        format!("{} LLDP neighbor(s) via systemd-networkd", neighbors.len())
    };
    Ok(LldpInventory {
        source: "systemd_networkd".into(),
        neighbors,
        raw_text: raw,
        summary,
    })
}

fn gather_lldp_from_systemd_json_dir() -> Result<LldpInventory, LibvirtError> {
    let dir = Path::new("/run/systemd/netif/lldp");
    if !dir.is_dir() {
        return Err(LibvirtError::Operation("no systemd lldp dir".into()));
    }
    let mut neighbors = Vec::new();
    let mut raw_parts = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| LibvirtError::Operation(e.to_string()))? {
        let entry = entry.map_err(|e| LibvirtError::Operation(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        raw_parts.push(text.clone());
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(arr) = parsed.as_array() {
                for item in arr {
                    neighbors.push(json_lldp_neighbor(item));
                }
            } else if parsed.is_object() {
                neighbors.push(json_lldp_neighbor(&parsed));
            }
        }
    }
    if neighbors.is_empty() {
        return Err(LibvirtError::Operation("empty systemd lldp json".into()));
    }
    Ok(LldpInventory {
        source: "systemd_networkd".into(),
        summary: format!("{} LLDP neighbor(s) from systemd JSON", neighbors.len()),
        neighbors,
        raw_text: raw_parts.join("\n---\n"),
    })
}

fn json_lldp_neighbor(v: &serde_json::Value) -> LldpNeighbor {
    LldpNeighbor {
        local_interface: v
            .get("IfName")
            .or_else(|| v.get("ifname"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        chassis_id: v
            .get("ChassisID")
            .or_else(|| v.get("chassis_id"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        system_name: v
            .get("SystemName")
            .or_else(|| v.get("system_name"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        port_id: v
            .get("PortID")
            .or_else(|| v.get("port_id"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        port_description: v
            .get("PortDescription")
            .or_else(|| v.get("port_description"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        system_description: v
            .get("SystemDescription")
            .or_else(|| v.get("system_description"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        capabilities: v
            .get("Capabilities")
            .or_else(|| v.get("capabilities"))
            .map(|x| x.to_string())
            .unwrap_or_default(),
    }
}

fn gather_lldp_from_nmcli() -> Result<LldpInventory, LibvirtError> {
    let nmcli = find_bin("nmcli");
    let raw = run_capture_soft(&nmcli, &["-t", "device", "lldp", "list"]);
    if raw.contains("(failed to run") {
        return Err(LibvirtError::Operation("nmcli lldp unavailable".into()));
    }
    let neighbors = parse_nmcli_lldp_text(&raw);
    let summary = if neighbors.is_empty() {
        "NetworkManager LLDP — no neighbors advertised".into()
    } else {
        format!("{} LLDP neighbor(s) via NetworkManager", neighbors.len())
    };
    Ok(LldpInventory {
        source: "network_manager".into(),
        neighbors,
        raw_text: raw,
        summary,
    })
}

fn parse_networkctl_lldp_text(raw: &str) -> Vec<LldpNeighbor> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty()
            || t.starts_with("IDX")
            || t.starts_with("LINK")
            || t.starts_with("──")
            || t.starts_with("No neighbors")
        {
            continue;
        }
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        // Skip optional numeric index column.
        let base = if parts[0].chars().all(|c| c.is_ascii_digit()) {
            1
        } else {
            0
        };
        if parts.len() <= base + 4 {
            continue;
        }
        out.push(LldpNeighbor {
            local_interface: parts[base].to_string(),
            chassis_id: parts[base + 1].to_string(),
            system_name: parts.get(base + 2).copied().unwrap_or("").to_string(),
            capabilities: parts.get(base + 3).copied().unwrap_or("").to_string(),
            port_id: parts.get(base + 4).copied().unwrap_or("").to_string(),
            port_description: parts
                .get(base + 5..)
                .map(|p| p.join(" "))
                .unwrap_or_default(),
            system_description: String::new(),
        });
    }
    out
}

fn parse_nmcli_lldp_text(raw: &str) -> Vec<LldpNeighbor> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let cols: Vec<&str> = t.split(':').collect();
        if cols.len() < 4 {
            continue;
        }
        out.push(LldpNeighbor {
            local_interface: cols[0].to_string(),
            chassis_id: cols.get(1).copied().unwrap_or("").to_string(),
            system_name: cols.get(2).copied().unwrap_or("").to_string(),
            port_id: cols.get(3).copied().unwrap_or("").to_string(),
            port_description: cols.get(4..).map(|c| c.join(":")).unwrap_or_default(),
            system_description: String::new(),
            capabilities: String::new(),
        });
    }
    out
}

pub fn get_systemd_interface_status(iface: &str) -> Result<String, LibvirtError> {
    if iface.is_empty()
        || !iface
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@')
    {
        return Err(LibvirtError::Invalid("Invalid interface name".to_string()));
    }
    run_capture(
        &find_bin("networkctl"),
        &["status", iface],
        "Failed to run networkctl status for interface",
    )
}

/// Reserved host ports for Machina daemons — must not be used for guest NAT rules.
const PORT_FORWARD_RESERVED_HOST_PORTS: &[u16] = &[5092, 5093, 50051, 50052];

pub fn validate_port_forward_protocol(protocol: &str) -> Result<(), LibvirtError> {
    if protocol != "tcp" && protocol != "udp" {
        return Err(LibvirtError::Invalid(
            "Protocol must be 'tcp' or 'udp'".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_port_forward_host_port(host_port: u16) -> Result<(), LibvirtError> {
    if host_port == 0 {
        return Err(LibvirtError::Invalid(
            "host_port must be non-zero".to_string(),
        ));
    }
    if host_port < 1024 {
        return Err(LibvirtError::Invalid(
            "host_port must be >= 1024 (privileged ports are not allowed)".to_string(),
        ));
    }
    if PORT_FORWARD_RESERVED_HOST_PORTS.contains(&host_port) {
        return Err(LibvirtError::Invalid(format!(
            "host_port {host_port} is reserved for Machina services"
        )));
    }
    Ok(())
}

pub fn validate_port_forward_vm_port(vm_port: u16) -> Result<(), LibvirtError> {
    if vm_port == 0 {
        return Err(LibvirtError::Invalid(
            "vm_port must be non-zero".to_string(),
        ));
    }
    Ok(())
}

pub fn create_port_forward(req: &CreatePortForwardRequest) -> Result<(), LibvirtError> {
    validate_port_forward_protocol(&req.protocol)?;
    validate_port_forward_host_port(req.host_port)?;
    validate_port_forward_vm_port(req.vm_port)?;
    if req.vm_ip.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(LibvirtError::Invalid(format!(
            "Invalid VM IP: {}",
            req.vm_ip
        )));
    }
    // Sanitize description
    let desc = req
        .description
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>();
    let comment = format!("machina:{desc}");

    // PREROUTING DNAT rule
    run_cmd(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "PREROUTING",
            "-p",
            &req.protocol,
            "--dport",
            &req.host_port.to_string(),
            "-j",
            "DNAT",
            "--to-destination",
            &format!("{}:{}", req.vm_ip, req.vm_port),
            "-m",
            "comment",
            "--comment",
            &comment,
        ],
        "Failed to create DNAT rule",
    )?;

    // FORWARD rule to allow the traffic
    run_cmd(
        "iptables",
        &[
            "-I",
            "FORWARD",
            "-p",
            &req.protocol,
            "-d",
            &req.vm_ip,
            "--dport",
            &req.vm_port.to_string(),
            "-j",
            "ACCEPT",
            "-m",
            "comment",
            "--comment",
            &comment,
        ],
        "Failed to create FORWARD rule",
    )?;

    Ok(())
}

pub fn delete_port_forward(
    proto: &str,
    host_port: u16,
    vm_ip: &str,
    vm_port: u16,
) -> Result<(), LibvirtError> {
    validate_port_forward_protocol(proto)?;
    validate_port_forward_host_port(host_port)?;
    validate_port_forward_vm_port(vm_port)?;
    if vm_ip.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(LibvirtError::Invalid(format!("Invalid VM IP: {vm_ip}")));
    }

    // Create installs `-m comment --comment machina:…`. A comment-less `iptables -D` never
    // matches that rule (exit 1 / "Bad rule"), so we delete by line number after listing.
    let proto_needle = format!(" {proto} ");
    let dnat_target = format!("{vm_ip}:{vm_port}");
    delete_iptables_matching_lines(Some("nat"), "PREROUTING", |line| {
        line.contains("machina:")
            && line.contains(&proto_needle)
            && extract_dpt(line) == Some(host_port)
            && extract_dnat_target(line).is_some_and(|(ip, port)| ip == vm_ip && port == vm_port)
    })?;

    delete_iptables_matching_lines(None, "FORWARD", |line| {
        line.contains("machina:")
            && line.contains(&proto_needle)
            && line.contains(vm_ip)
            && extract_dpt(line) == Some(vm_port)
            && !line.contains(&dnat_target) // FORWARD ACCEPT, not DNAT
    })?;

    // Surface a real failure if the listed DNAT rule is still present (caller previously
    // got ok:true while the forward stayed live).
    let still = list_port_forwards()?.into_iter().any(|r| {
        r.protocol == proto && r.host_port == host_port && r.vm_ip == vm_ip && r.vm_port == vm_port
    });
    if still {
        return Err(LibvirtError::Operation(format!(
            "port forward {proto}/{host_port}->{vm_ip}:{vm_port} still present after delete"
        )));
    }

    Ok(())
}

/// Delete every matching rule in `chain` by repeating list→delete-by-line-number until none
/// remain. Line numbers shift after each `-D N`, so we always re-list.
fn delete_iptables_matching_lines(
    table: Option<&str>,
    chain: &str,
    matches: impl Fn(&str) -> bool,
) -> Result<(), LibvirtError> {
    let iptables = find_bin("iptables");
    for _ in 0..64 {
        let mut list_args: Vec<&str> = Vec::new();
        if let Some(t) = table {
            list_args.extend_from_slice(&["-t", t]);
        }
        list_args.extend_from_slice(&["-L", chain, "-n", "--line-numbers", "-v"]);
        let output =
            Command::new(&iptables)
                .args(&list_args)
                .output()
                .map_err(LibvirtError::map_op(
                    "Failed to list iptables rules for delete",
                ))?;
        if !output.status.success() {
            return Err(LibvirtError::Operation(format!(
                "iptables -L {chain} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line_num = stdout.lines().find_map(|line| {
            if !matches(line) {
                return None;
            }
            line.split_whitespace().next()?.parse::<u32>().ok()
        });
        let Some(n) = line_num else {
            return Ok(());
        };
        let n_s = n.to_string();
        let mut del_args: Vec<&str> = Vec::new();
        if let Some(t) = table {
            del_args.extend_from_slice(&["-t", t]);
        }
        del_args.extend_from_slice(&["-D", chain, &n_s]);
        let del = Command::new(&iptables)
            .args(&del_args)
            .output()
            .map_err(LibvirtError::map_op("Failed to delete iptables rule"))?;
        if !del.status.success() {
            return Err(LibvirtError::Operation(format!(
                "iptables -D {chain} {n} failed: {}",
                String::from_utf8_lossy(&del.stderr).trim()
            )));
        }
    }
    Err(LibvirtError::Operation(format!(
        "too many matching iptables rules in {chain} (gave up after 64 deletes)"
    )))
}

// ── Native load balancing (weighted round-robin via iptables) ──────
//
// A native "Fleet Cloud" load balancer is a listener (protocol + host_port) that fans out to
// N backend VM:port members. Each LB owns two dedicated, per-LB chains (one in `nat`, one in
// `filter`) so the whole backend set can be replaced by flushing and repopulating those two
// chains -- no need to track which exact rules were previously installed to issue matching
// `-D` deletes, which is what makes this safe to call on every member/health-status change.
// Weighting uses the standard iptables trick: chained `-m statistic --mode random --probability`
// DNAT rules, where each rule's probability is its share of the still-unassigned weight and the
// last member has no match condition (catches whatever's left).

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LbMember {
    pub vm_ip: String,
    pub port: u16,
    pub weight: u32,
}

/// Per-LB chain names, derived from the LB id so multiple LBs never collide. Kept short
/// (iptables chain names cap at 28 usable chars) by taking the first 12 hex characters.
fn lb_chain_names(lb_id: &str) -> (String, String) {
    let short: String = lb_id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(12)
        .collect();
    (format!("MCLB-{short}"), format!("MCLBF-{short}"))
}

fn iptables_ok(args: &[&str]) -> bool {
    Command::new(find_bin("iptables"))
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ensure_lb_chain(table: &str, chain: &str) {
    if !iptables_ok(&["-t", table, "-L", chain, "-n"]) {
        let _ = Command::new(find_bin("iptables"))
            .args(["-t", table, "-N", chain])
            .output();
    }
}

/// Install (or fully replace) the weighted round-robin rule set for one native load balancer.
/// Idempotent: flushes the LB's own chains before repopulating, so this is safe to call on
/// every member add/remove/health-status change without tracking prior rule state.
pub fn set_load_balancer_rules(
    lb_id: &str,
    protocol: &str,
    host_port: u16,
    members: &[LbMember],
) -> Result<(), LibvirtError> {
    validate_port_forward_protocol(protocol)?;
    validate_port_forward_host_port(host_port)?;
    for m in members {
        if m.vm_ip.parse::<std::net::Ipv4Addr>().is_err() {
            return Err(LibvirtError::Invalid(format!(
                "Invalid member IP: {}",
                m.vm_ip
            )));
        }
        if m.port == 0 {
            return Err(LibvirtError::Invalid(
                "member port must be non-zero".to_string(),
            ));
        }
    }

    let (nat_chain, fwd_chain) = lb_chain_names(lb_id);
    ensure_lb_chain("nat", &nat_chain);
    ensure_lb_chain("filter", &fwd_chain);

    let comment = format!("machina-lb-{lb_id}");

    if !iptables_ok(&[
        "-t",
        "nat",
        "-C",
        "PREROUTING",
        "-p",
        protocol,
        "--dport",
        &host_port.to_string(),
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        &nat_chain,
    ]) {
        run_cmd(
            "iptables",
            &[
                "-t",
                "nat",
                "-A",
                "PREROUTING",
                "-p",
                protocol,
                "--dport",
                &host_port.to_string(),
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                &nat_chain,
            ],
            "Failed to add load balancer jump rule",
        )?;
    }
    if !iptables_ok(&[
        "-t",
        "filter",
        "-C",
        "FORWARD",
        "-p",
        protocol,
        "-m",
        "comment",
        "--comment",
        &comment,
        "-j",
        &fwd_chain,
    ]) {
        run_cmd(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                "FORWARD",
                "-p",
                protocol,
                "-m",
                "comment",
                "--comment",
                &comment,
                "-j",
                &fwd_chain,
            ],
            "Failed to add load balancer forward jump rule",
        )?;
    }

    run_cmd(
        "iptables",
        &["-t", "nat", "-F", &nat_chain],
        "Failed to flush load balancer chain",
    )?;
    run_cmd(
        "iptables",
        &["-t", "filter", "-F", &fwd_chain],
        "Failed to flush load balancer forward chain",
    )?;

    let total_weight: f64 = members.iter().map(|m| m.weight.max(1) as f64).sum();
    let mut remaining = total_weight;
    for (i, m) in members.iter().enumerate() {
        let w = m.weight.max(1) as f64;
        let dest = format!("{}:{}", m.vm_ip, m.port);
        // nftables-backed iptables requires an explicit `-p` whenever the target
        // carries a port (`--to-destination ip:port`); omitting it yields
        // "Need TCP, UDP, SCTP or DCCP with port specification".
        if i == members.len() - 1 {
            run_cmd(
                "iptables",
                &[
                    "-t",
                    "nat",
                    "-A",
                    &nat_chain,
                    "-p",
                    protocol,
                    "-j",
                    "DNAT",
                    "--to-destination",
                    &dest,
                ],
                "Failed to add load balancer backend rule",
            )?;
        } else {
            let probability = format!("{:.4}", (w / remaining).clamp(0.0001, 0.9999));
            run_cmd(
                "iptables",
                &[
                    "-t",
                    "nat",
                    "-A",
                    &nat_chain,
                    "-p",
                    protocol,
                    "-m",
                    "statistic",
                    "--mode",
                    "random",
                    "--probability",
                    &probability,
                    "-j",
                    "DNAT",
                    "--to-destination",
                    &dest,
                ],
                "Failed to add load balancer backend rule",
            )?;
        }
        remaining -= w;

        run_cmd(
            "iptables",
            &[
                "-t",
                "filter",
                "-A",
                &fwd_chain,
                "-p",
                protocol,
                "-d",
                &m.vm_ip,
                "--dport",
                &m.port.to_string(),
                "-j",
                "ACCEPT",
            ],
            "Failed to add load balancer forward-accept rule",
        )?;
    }

    Ok(())
}

/// Tear down a native load balancer's jump rules and chains. Best-effort: failures are logged,
/// not surfaced -- the caller (deleting the load balancer) has nothing useful left to retry.
pub fn delete_load_balancer_rules(lb_id: &str, protocol: &str, host_port: u16) {
    let (nat_chain, fwd_chain) = lb_chain_names(lb_id);
    let comment = format!("machina-lb-{lb_id}");

    let _ = Command::new(find_bin("iptables"))
        .args([
            "-t",
            "nat",
            "-D",
            "PREROUTING",
            "-p",
            protocol,
            "--dport",
            &host_port.to_string(),
            "-m",
            "comment",
            "--comment",
            &comment,
            "-j",
            &nat_chain,
        ])
        .output();
    let _ = Command::new(find_bin("iptables"))
        .args([
            "-t",
            "filter",
            "-D",
            "FORWARD",
            "-p",
            protocol,
            "-m",
            "comment",
            "--comment",
            &comment,
            "-j",
            &fwd_chain,
        ])
        .output();

    for (table, chain) in [("nat", nat_chain.as_str()), ("filter", fwd_chain.as_str())] {
        let _ = Command::new(find_bin("iptables"))
            .args(["-t", table, "-F", chain])
            .output();
        let _ = Command::new(find_bin("iptables"))
            .args(["-t", table, "-X", chain])
            .output();
    }
}

// ── Per-VM firewall rules ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub id: String,
    pub vm_ip: String,
    pub direction: String, // inbound, outbound
    pub protocol: String,  // tcp, udp, icmp, all
    pub port: u16,         // 0 = all ports
    pub action: String,    // accept, drop
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFirewallRuleRequest {
    pub vm_ip: String,
    pub direction: String,
    pub protocol: String,
    pub port: u16,
    pub action: String,
    #[serde(default)]
    pub description: String,
}

pub fn list_firewall_rules() -> Result<Vec<FirewallRule>, LibvirtError> {
    let output = Command::new(find_bin("iptables"))
        .args(["-L", "FORWARD", "-n", "--line-numbers", "-v"])
        .output()
        .map_err(LibvirtError::map_op("Failed to list firewall rules"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut rules = Vec::new();

    for line in stdout.lines() {
        if !line.contains("vs-fw:") {
            continue;
        }

        let action = if line.contains("ACCEPT") {
            "accept"
        } else if line.contains("DROP") {
            "drop"
        } else {
            continue;
        };
        let proto = if line.contains(" tcp ") {
            "tcp"
        } else if line.contains(" udp ") {
            "udp"
        } else if line.contains(" icmp ") {
            "icmp"
        } else {
            "all"
        };
        let port = extract_dpt(line).unwrap_or(0);
        let desc = extract_comment(line, "vs-fw:").unwrap_or_default();

        // Determine direction and VM IP from source/dest
        let (direction, vm_ip) = parse_fw_direction(line);

        let id = format!("{direction}-{proto}-{port}-{vm_ip}-{action}");
        rules.push(FirewallRule {
            id,
            vm_ip,
            direction,
            protocol: proto.to_string(),
            port,
            action: action.to_string(),
            description: desc,
        });
    }

    Ok(rules)
}

pub fn create_firewall_rule(req: &CreateFirewallRuleRequest) -> Result<(), LibvirtError> {
    if req.vm_ip.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(LibvirtError::Invalid(format!(
            "Invalid VM IP: {}",
            req.vm_ip
        )));
    }
    if req.direction != "inbound" && req.direction != "outbound" {
        return Err(LibvirtError::Invalid(
            "Direction must be 'inbound' or 'outbound'".to_string(),
        ));
    }
    if req.action != "accept" && req.action != "drop" {
        return Err(LibvirtError::Invalid(
            "Action must be 'accept' or 'drop'".to_string(),
        ));
    }
    let valid_protos = ["tcp", "udp", "icmp", "all"];
    if !valid_protos.contains(&req.protocol.as_str()) {
        return Err(LibvirtError::Invalid(format!(
            "Protocol must be one of: {}",
            valid_protos.join(", ")
        )));
    }

    let target = if req.action == "accept" {
        "ACCEPT"
    } else {
        "DROP"
    };
    let desc = req
        .description
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>();
    let comment = format!("vs-fw:{desc}");

    let mut args = vec!["-I", "FORWARD"];

    // Direction
    if req.direction == "inbound" {
        args.extend(["-d", &req.vm_ip]);
    } else {
        args.extend(["-s", &req.vm_ip]);
    }

    // Protocol
    if req.protocol != "all" {
        args.extend(["-p", &req.protocol]);
    }

    // Port (only for tcp/udp)
    let port_str = req.port.to_string();
    if req.port > 0 && (req.protocol == "tcp" || req.protocol == "udp") {
        args.extend(["--dport", &port_str]);
    }

    args.extend(["-j", target, "-m", "comment", "--comment", &comment]);

    run_cmd("iptables", &args, "Failed to create firewall rule")
}

pub fn delete_firewall_rule(req: &CreateFirewallRuleRequest) -> Result<(), LibvirtError> {
    let target = if req.action == "accept" {
        "ACCEPT"
    } else {
        "DROP"
    };

    let mut args = vec!["-D", "FORWARD"];
    if req.direction == "inbound" {
        args.extend(["-d", &req.vm_ip]);
    } else {
        args.extend(["-s", &req.vm_ip]);
    }
    if req.protocol != "all" {
        args.extend(["-p", &req.protocol]);
    }
    let port_str = req.port.to_string();
    if req.port > 0 && (req.protocol == "tcp" || req.protocol == "udp") {
        args.extend(["--dport", &port_str]);
    }
    args.extend(["-j", target]);

    // Best-effort (the rule may already be gone) but still worth logging: a swallowed
    // failure here previously left the FORWARD rule in place while callers were told
    // it was deleted.
    match Command::new(find_bin("iptables")).args(&args).output() {
        Ok(out) if !out.status.success() => {
            tracing::warn!(
                "delete_firewall_rule: iptables delete failed for {} {} {}:{}: {}",
                req.direction,
                req.protocol,
                req.vm_ip,
                req.port,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(e) => tracing::warn!("delete_firewall_rule: failed to run iptables: {e}"),
        Ok(_) => {}
    }
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────

/// Find binary in common locations.
fn find_bin(name: &str) -> String {
    let candidates = [
        format!("/usr/bin/{name}"),
        format!("/usr/sbin/{name}"),
        format!("/sbin/{name}"),
        format!("/bin/{name}"),
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return c.clone();
        }
    }
    name.to_string()
}

fn run_cmd(cmd: &str, args: &[&str], context: &str) -> Result<(), LibvirtError> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("{context}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "{context}: {}",
            stderr.trim()
        )));
    }
    Ok(())
}

fn run_capture(cmd: &str, args: &[&str], context: &str) -> Result<String, LibvirtError> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| LibvirtError::Operation(format!("{context}: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LibvirtError::Operation(format!(
            "{context}: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Like [`run_capture`], but returns stdout/stderr text instead of failing (for optional probes).
fn run_capture_soft(cmd: &str, args: &[&str]) -> String {
    match Command::new(cmd).args(args).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            format!(
                "(exit {})\n{}\n{}",
                out.status.code().unwrap_or(-1),
                stderr.trim(),
                stdout.trim()
            )
            .trim()
            .to_string()
        }
        Err(e) => format!("(failed to run {cmd}: {e})"),
    }
}

fn is_service_active(name: &str) -> Result<bool, LibvirtError> {
    let output = Command::new(find_bin("systemctl"))
        .args(["is-active", name])
        .output()
        .map_err(LibvirtError::map_op("Failed to run systemctl is-active"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "active")
}

fn extract_dpt(line: &str) -> Option<u16> {
    let pos = line.find("dpt:")?;
    let rest = &line[pos + 4..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn extract_dnat_target(line: &str) -> Option<(String, u16)> {
    let pos = line.find("to:")?;
    let rest = &line[pos + 3..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != ':')
        .unwrap_or(rest.len());
    let target = &rest[..end];
    let parts: Vec<&str> = target.split(':').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].parse().ok()?))
    } else {
        None
    }
}

fn extract_comment(line: &str, prefix: &str) -> Option<String> {
    let pos = line.find(prefix)?;
    let rest = &line[pos + prefix.len()..];
    let end = rest.find(['*', '/']).unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
}

fn parse_fw_direction(line: &str) -> (String, String) {
    // Parse source/dest from iptables -v output
    // Format: ... src_ip dest_ip ...
    let parts: Vec<&str> = line.split_whitespace().collect();
    // In verbose output, source is column 8, dest is column 9 (0-indexed)
    if parts.len() > 9 {
        let src = parts[8];
        let dest = parts[9];
        if src != "0.0.0.0/0" && dest == "0.0.0.0/0" {
            return ("outbound".to_string(), src.to_string());
        }
        if dest != "0.0.0.0/0" {
            return ("inbound".to_string(), dest.to_string());
        }
    }
    ("inbound".to_string(), "0.0.0.0".to_string())
}

#[cfg(test)]
mod port_forward_validation_tests {
    use super::{
        validate_port_forward_host_port, validate_port_forward_protocol,
        validate_port_forward_vm_port,
    };

    #[test]
    fn rejects_privileged_host_port() {
        assert!(validate_port_forward_host_port(22).is_err());
        assert!(validate_port_forward_host_port(80).is_err());
    }

    #[test]
    fn rejects_reserved_machina_ports() {
        assert!(validate_port_forward_host_port(50051).is_err());
        assert!(validate_port_forward_host_port(5092).is_err());
    }

    #[test]
    fn accepts_unprivileged_host_port() {
        assert!(validate_port_forward_host_port(2222).is_ok());
    }

    #[test]
    fn rejects_zero_ports() {
        assert!(validate_port_forward_host_port(0).is_err());
        assert!(validate_port_forward_vm_port(0).is_err());
    }

    #[test]
    fn rejects_invalid_protocol() {
        assert!(validate_port_forward_protocol("icmp").is_err());
        assert!(validate_port_forward_protocol("tcp").is_ok());
    }
}
