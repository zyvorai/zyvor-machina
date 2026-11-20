// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Aggregated guest health: libvirt state + QEMU agent + VM metrics.

use virt::connect::Connect;

use super::domain;
use super::guest_agent::{self, GuestInfo};
use super::guest_agent_diag::{probe_guest_agent, GuestAgentDiagnostics};
use super::metrics;
use crate::state::VmMetrics;
use crate::LibvirtError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestHealthReport {
    pub vm_name: String,
    pub state: String,
    pub agent_reachable: bool,
    pub metrics_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<VmMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest: Option<GuestInfo>,
    pub issues: Vec<String>,
    pub healthy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_pretty_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_init_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<GuestAgentDiagnostics>,
}

pub fn gather_guest_health(conn: &Connect, name: &str) -> Result<GuestHealthReport, LibvirtError> {
    let details = domain::get_vm_details(conn, name)?;
    let state = details.state.clone();
    let running = state.eq_ignore_ascii_case("running");

    let guest = guest_agent::get_guest_observability(conn, name).ok();
    let diagnostics = probe_guest_agent(conn, name, guest.as_ref()).ok();
    let agent_reachable = diagnostics
        .as_ref()
        .map(|d| d.agent_ping)
        .unwrap_or_else(|| {
            guest.as_ref().is_some_and(|g| {
                g.ip_addresses
                    .iter()
                    .any(|a| a.ip_type == "ipv4" && !a.address.starts_with("127."))
                    || !g.filesystems.is_empty()
                    || (!g.hostname.is_empty() && g.hostname != "localhost")
            })
        });

    let metrics = if running {
        metrics::get_vm_metrics(conn, name).ok()
    } else {
        None
    };
    let metrics_available = metrics.is_some();

    let mut issues = Vec::new();
    if running && !agent_reachable {
        let xml = domain::lookup_domain(conn, name)
            .and_then(|d| {
                d.get_xml_desc(0)
                    .map_err(LibvirtError::map_op("get_xml_desc"))
            })
            .unwrap_or_default();
        let channel_attached = xml.contains("org.qemu.guest_agent.0");
        let channel_disconnected =
            xml.contains("state='disconnected'") || xml.contains("state=\"disconnected\"");
        if channel_attached && channel_disconnected {
            issues.push(
                "Guest agent channel attached but guestkit-agent is not running in the VM".into(),
            );
        } else if guest.as_ref().is_some_and(|g| g.ip_addresses.is_empty()) {
            issues.push(
                "No guest IPv4 from DHCP lease, ARP, or guest agent — check the VM network (VNC) or install guestkit-agent".into(),
            );
        } else {
            issues.push("Guest agent unreachable — install and start guestkit-agent".into());
        }
    }
    if let Some(m) = &metrics {
        if m.memory_pct >= 95.0 {
            issues.push(format!("memory utilization {:.0}%", m.memory_pct));
        }
    }
    let os_pretty_name = guest
        .as_ref()
        .and_then(|g| g.os_pretty_name.clone())
        .filter(|s| !s.is_empty());
    let cloud_init_status = guest
        .as_ref()
        .and_then(|g| g.cloud_init_status.clone())
        .filter(|s| !s.is_empty());
    if let Some(ref st) = cloud_init_status {
        if st.to_ascii_lowercase().contains("error") || st.to_ascii_lowercase().contains("failed") {
            issues.push(format!("cloud-init: {st}"));
        }
    }
    if let Some(g) = &guest {
        for fs in &g.filesystems {
            if fs.total_bytes > 0 {
                let pct = (fs.used_bytes as f64 / fs.total_bytes as f64) * 100.0;
                if pct >= 90.0 {
                    issues.push(format!(
                        "guest filesystem {} {:.0}% full",
                        fs.mountpoint, pct
                    ));
                }
            }
        }
    }
    if !running {
        issues.push(format!("VM is {state}"));
    }

    let healthy = running && agent_reachable && issues.is_empty();

    Ok(GuestHealthReport {
        vm_name: name.to_string(),
        state,
        agent_reachable,
        metrics_available,
        metrics,
        guest,
        issues,
        healthy,
        os_pretty_name,
        cloud_init_status,
        diagnostics,
    })
}
