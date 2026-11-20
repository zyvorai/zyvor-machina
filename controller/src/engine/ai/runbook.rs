// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct Runbook {
    pub incident: String,
    pub title: String,
    pub steps: Vec<String>,
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

pub async fn generate(
    pool: &DbPool,
    incident: &str,
    context: &serde_json::Value,
) -> anyhow::Result<Runbook> {
    let (title, steps, commands) = template(incident);
    let mut rb = Runbook {
        incident: incident.into(),
        title: title.into(),
        steps: steps.iter().map(|s| s.to_string()).collect(),
        commands: commands.iter().map(|s| s.to_string()).collect(),
        summary: None,
    };

    if let Ok(Some(llm)) = super::llm::complete_simple(
        pool,
        "Write a short customer-friendly incident summary for IT operators.",
        &format!("Incident: {incident}\nContext: {context}"),
    )
    .await
    {
        rb.summary = Some(llm);
    } else {
        rb.summary = Some(format!("Follow the {title} runbook steps below."));
    }
    Ok(rb)
}

fn template(
    incident: &str,
) -> (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
) {
    match incident {
        "backup_failed" => (
            "Backup failure",
            &[
                "Check VM power state and guest tools",
                "Verify backup target path on host",
                "Review task message for agent errors",
                "Retry backup from VM Backup tab",
                "Escalate if storage pool is full",
            ],
            &["virsh domstate VM", "df -h /var/lib/libvirt/images"],
        ),
        "migration_failed" => (
            "Migration failure",
            &[
                "Review migrate pre-check results",
                "Ensure destination host is online",
                "Verify CPU compatibility matrix",
                "Check live migration network",
                "Retry offline migration if live fails",
            ],
            &["virsh list", "virsh domjobinfo VM"],
        ),
        "host_offline" => (
            "Host offline",
            &[
                "Check agent service on hypervisor",
                "Verify network to agent gRPC port",
                "Review last heartbeat timestamp",
                "Sync host inventory after recovery",
                "Evacuate VMs if host is fenced",
            ],
            &[
                "systemctl status machina-agent",
                "journalctl -u machina-agent -n 50",
            ],
        ),
        "firewall_drift" => (
            "Firewall drift remediation",
            &[
                "Review Zeus Firewall drift report for affected hosts",
                "Compare live inventory to last checkpoint",
                "Apply ProductionServer or Emergency profile",
                "Create rollback checkpoint before apply",
                "Verify open ports after remediation",
            ],
            &[
                "curl -s localhost:8080/api/v1/zeus-firewall/targets",
                "firewall-cmd --list-all",
            ],
        ),
        "storage_full" => (
            "Storage pool capacity",
            &[
                "Check pool used vs capacity on all hosts",
                "Identify snapshot-heavy VMs",
                "Run storage tier bind for bronze archive tier",
                "Expand pool or migrate VMs to alternate datastore",
                "Enable backup SLA review for retention",
            ],
            &["df -h /var/lib/libvirt/images", "virsh domblklist VM"],
        ),
        _ => (
            "VM unreachable",
            &[
                "Check VM power state",
                "Open serial/VNC console",
                "Verify guest agent",
                "Check host and network",
                "Review recent tasks and events",
            ],
            &["virsh domstate VM", "virsh domifaddr VM"],
        ),
    }
}
