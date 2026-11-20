// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use virt::connect::Connect;

use super::domain::lookup_domain;
use super::guest_agent::{self, GuestInfo};
use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestAgentCheck {
    pub id: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestAgentDiagnostics {
    /// `none` | `channel_only` | `running`
    pub install_state: String,
    pub channel_attached: bool,
    pub channel_connected: bool,
    pub agent_ping: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest_timezone: Option<String>,
    pub filesystem_count: u32,
    pub interface_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_discovery_source: Option<String>,
    pub checks: Vec<GuestAgentCheck>,
}

fn channel_state(xml: &str) -> (bool, bool) {
    let attached = xml.contains("org.qemu.guest_agent.0");
    let connected = attached
        && !xml.contains("state='disconnected'")
        && !xml.contains("state=\"disconnected\"");
    (attached, connected)
}

#[cfg(target_os = "linux")]
fn agent_ping(vm_name: &str) -> bool {
    guest_agent::qemu_agent_command(vm_name, r#"{"execute":"guest-ping"}"#)
        .and_then(|v| v.get("return").cloned())
        .is_some()
}

#[cfg(not(target_os = "linux"))]
fn agent_ping(_vm_name: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn agent_version(vm_name: &str) -> Option<String> {
    let v = guest_agent::qemu_agent_command(vm_name, r#"{"execute":"guest-info"}"#)?;
    let ret = v.get("return")?;
    let version = ret.get("version")?.as_str()?;
    Some(version.to_string())
}

#[cfg(not(target_os = "linux"))]
fn agent_version(_vm_name: &str) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn agent_timezone(vm_name: &str) -> Option<String> {
    let v = guest_agent::qemu_agent_command(vm_name, r#"{"execute":"guest-get-timezone"}"#)?;
    ret_string_field(&v, "timezone")
}

#[cfg(not(target_os = "linux"))]
fn agent_timezone(_vm_name: &str) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn ret_string_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get("return")
        .and_then(|r| r.get(key))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

pub fn probe_guest_agent(
    conn: &Connect,
    name: &str,
    guest: Option<&GuestInfo>,
) -> Result<GuestAgentDiagnostics, LibvirtError> {
    let xml = lookup_domain(conn, name)
        .and_then(|d| {
            d.get_xml_desc(0)
                .map_err(LibvirtError::map_op("get_xml_desc"))
        })
        .unwrap_or_default();
    let (channel_attached, channel_connected) = channel_state(&xml);
    let agent_ping = if channel_connected {
        agent_ping(name)
    } else {
        false
    };
    let agent_version = if agent_ping {
        agent_version(name)
    } else {
        None
    };
    let guest_timezone = if agent_ping {
        agent_timezone(name)
    } else {
        None
    };

    let install_state = if agent_ping {
        "running".to_string()
    } else if channel_attached {
        "channel_only".to_string()
    } else {
        "none".to_string()
    };

    let filesystem_count = guest.map(|g| g.filesystems.len() as u32).unwrap_or(0);
    let interface_count = guest.map(|g| g.ip_addresses.len() as u32).unwrap_or(0);
    let ip_discovery_source = guest.and_then(|g| {
        g.ip_addresses
            .iter()
            .find(|a| a.ip_type == "ipv4" && !a.address.starts_with("127."))
            .map(|a| a.source.clone())
    });

    let mut checks = Vec::new();
    checks.push(GuestAgentCheck {
        id: "virtio_channel".into(),
        label: "Virtio guest-agent channel".into(),
        passed: channel_attached,
        detail: if channel_attached {
            "org.qemu.guest_agent.0 present (GuestKit agent speaks QGA protocol)".into()
        } else {
            "Missing — use Install guest tools in Machina".into()
        },
    });
    checks.push(GuestAgentCheck {
        id: "channel_connected".into(),
        label: "Channel connected".into(),
        passed: channel_connected,
        detail: if channel_connected {
            "Hypervisor socket is connected".into()
        } else if channel_attached {
            "Channel attached but disconnected — start guestkit-agent in the guest".into()
        } else {
            "N/A until channel is attached".into()
        },
    });
    checks.push(GuestAgentCheck {
        id: "agent_ping".into(),
        label: "Guest agent ping (QGA guest-ping)".into(),
        passed: agent_ping,
        detail: if agent_ping {
            format!(
                "guest-ping OK{}",
                agent_version
                    .as_ref()
                    .map(|v| format!(" · {v}"))
                    .unwrap_or_default()
            )
        } else {
            "guest-ping failed — install guestkit-agent (replaces qemu-guest-agent)".into()
        },
    });
    let has_ip = ip_discovery_source.is_some();
    checks.push(GuestAgentCheck {
        id: "guest_ipv4".into(),
        label: "Guest IPv4 discoverable".into(),
        passed: has_ip,
        detail: if has_ip {
            format!(
                "Via {}",
                ip_discovery_source.as_deref().unwrap_or("unknown")
            )
        } else {
            "No lease, ARP, or agent IP — check VM networking".into()
        },
    });
    if let Some(g) = guest {
        if !g
            .os_pretty_name
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            checks.push(GuestAgentCheck {
                id: "os_info".into(),
                label: "Guest OS info".into(),
                passed: true,
                detail: g.os_pretty_name.clone().unwrap_or_default(),
            });
        }
        if filesystem_count > 0 {
            checks.push(GuestAgentCheck {
                id: "filesystems".into(),
                label: "Guest filesystem stats".into(),
                passed: true,
                detail: format!("{filesystem_count} mount(s) via guest-get-fsinfo"),
            });
        }
        if let Some(ref st) = g.cloud_init_status {
            if !st.is_empty() {
                let ok = !st.to_ascii_lowercase().contains("error")
                    && !st.to_ascii_lowercase().contains("failed");
                checks.push(GuestAgentCheck {
                    id: "cloud_init".into(),
                    label: "Cloud-init status".into(),
                    passed: ok,
                    detail: st.clone(),
                });
            }
        }
        if let Some(ref tz) = guest_timezone {
            checks.push(GuestAgentCheck {
                id: "timezone".into(),
                label: "Guest timezone".into(),
                passed: true,
                detail: tz.clone(),
            });
        }
    }

    Ok(GuestAgentDiagnostics {
        install_state,
        channel_attached,
        channel_connected,
        agent_ping,
        agent_version,
        guest_timezone,
        filesystem_count,
        interface_count,
        ip_discovery_source,
        checks,
    })
}
