// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::agent_client;

#[derive(Debug, Clone, Serialize)]
pub struct HealthIssue {
    pub id: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmHealthReport {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub score: String,
    pub score_numeric: u8,
    pub score_label: String,
    pub healthy: bool,
    pub checks_passed: u32,
    pub checks_total: u32,
    pub issues: Vec<HealthIssue>,
    pub guest_tools_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest_hostname: Option<String>,
}

fn issue(
    id: &str,
    severity: &str,
    message: impl Into<String>,
    remediation: impl Into<String>,
    fix_action: Option<&str>,
    fix_label: Option<&str>,
) -> HealthIssue {
    HealthIssue {
        id: id.into(),
        severity: severity.into(),
        message: message.into(),
        remediation: Some(remediation.into()),
        fix_action: fix_action.map(String::from),
        fix_label: fix_label.map(String::from),
    }
}

pub async fn run_vm_health_check(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<VmHealthReport> {
    let row: Option<(
        String,
        Option<Uuid>,
        String,
        String,
        bool,
        crate::db::Json<Vec<String>>,
    )> = crate::db::query_as(
        "SELECT name, host_id, observed_state, COALESCE(guest_tools_status, 'unknown'),
                COALESCE(managed, TRUE), COALESCE(tags, '[]')
         FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;

    let Some((name, host_id, observed, guest_status, managed, tags)) = row else {
        anyhow::bail!("vm not found");
    };

    let mut issues = Vec::new();
    let mut passed = 0u32;
    let mut total = 0u32;

    total += 1;
    if observed == "running" {
        passed += 1;
    } else {
        issues.push(issue(
            "power_state",
            "warning",
            format!("VM is {observed}"),
            "Start the VM if it should be running",
            Some("start_vm"),
            Some("Start VM"),
        ));
    }

    total += 1;
    if managed {
        passed += 1;
    } else {
        issues.push(issue(
            "managed",
            "warning",
            "VM is not managed by the platform",
            "Adopt the VM to enable full lifecycle management",
            Some("adopt_vm"),
            Some("Adopt VM"),
        ));
    }

    let ha: bool =
        crate::db::query_scalar("SELECT COALESCE(enabled, FALSE) FROM ha_policies WHERE vm_id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(false);

    let is_prod = tags
        .iter()
        .any(|t| t.eq_ignore_ascii_case("prod") || t.eq_ignore_ascii_case("production"));

    total += 1;
    if !is_prod || ha {
        passed += 1;
    } else {
        issues.push(issue(
            "ha",
            "warning",
            "Production VM without HA policy",
            "Enable restart-on-host-failure for critical VMs",
            Some("enable_ha"),
            Some("Enable HA"),
        ));
    }

    let backup_count: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM backup_records WHERE vm_id = ? AND status = 'completed'",
    )
    .bind(vm_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    total += 1;
    if backup_count > 0 || !is_prod {
        passed += 1;
    } else {
        issues.push(issue(
            "backup",
            "warning",
            "No successful backup on record",
            "Schedule a backup for production VMs",
            Some("create_backup"),
            Some("Backup now"),
        ));
    }

    // Fail closed: defaulting a query error to 0 (as this used to) makes an unverifiable
    // snapshot count look identical to "no snapshots", which silently PASSES this check
    // (0 <= 5) instead of surfacing that we couldn't actually check it.
    total += 1;
    match crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM snapshot_records WHERE vm_id = ?")
        .bind(vm_id)
        .fetch_one(pool)
        .await
    {
        Ok(snap_count) if snap_count <= 5 => passed += 1,
        Ok(snap_count) => issues.push(issue(
            "snapshots",
            "warning",
            format!("{snap_count} snapshots — consolidate old snapshots"),
            "Delete or consolidate snapshots older than 30 days",
            Some("open_snapshots"),
            Some("Manage snapshots"),
        )),
        Err(e) => issues.push(issue(
            "snapshots_unknown",
            "warning",
            format!("Could not verify snapshot count: {e}"),
            "Retry the health check; if this persists, check controller DB connectivity",
            None,
            None,
        )),
    }

    let cpu_pressure: Option<f32> =
        crate::db::query_scalar("SELECT cpu_percent FROM vm_metrics WHERE vm_id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    if let Some(cpu) = cpu_pressure {
        total += 1;
        if cpu < 85.0 {
            passed += 1;
        } else {
            issues.push(issue(
                "cpu_pressure",
                "warning",
                format!("High CPU utilization ({cpu:.0}%)"),
                "Consider right-sizing or migrating to a less loaded host",
                None,
                None,
            ));
        }
    }

    let mut guest_ip = None;
    let mut guest_hostname = None;
    let mut guest_tools_status = guest_status;
    let mut os_family: Option<String> = None;

    if observed == "running" {
        if let Some(hid) = host_id {
            // Fail closed: an inability to reach the host agent or query the guest
            // must NOT fall through silently and leave `guest_tools_status` at its
            // stale, possibly-"healthy" cached value — that would report a VM as
            // healthy purely because we failed to check it. Surface it as an issue
            // instead so the score reflects the unknown state.
            let probe_err = match host_agent_addr(pool, hid).await {
                Ok(addr) => match agent_client::connect(&addr).await {
                    Ok(mut client) => {
                        match agent_client::get_guest_health(&mut client, &name).await {
                            Ok(gh) => {
                                guest_tools_status = if gh.agent_reachable {
                                    if gh.healthy {
                                        "healthy".into()
                                    } else {
                                        "installed".into()
                                    }
                                } else {
                                    "not_installed".into()
                                };
                                guest_ip = if gh.guest_ip.is_empty() {
                                    None
                                } else {
                                    Some(gh.guest_ip.clone())
                                };
                                guest_hostname = if gh.guest_hostname.is_empty() {
                                    None
                                } else {
                                    Some(gh.guest_hostname.clone())
                                };
                                if !gh.os_pretty_name.is_empty() {
                                    os_family = Some(gh.os_pretty_name.clone());
                                }

                                total += 1;
                                if gh.agent_reachable {
                                    passed += 1;
                                } else {
                                    issues.push(issue(
                                        "guest_agent",
                                        "warning",
                                        "Guest tools not installed or not responding",
                                        "Install qemu-guest-agent (Zyvor Guest Tools) for graceful shutdown and monitoring",
                                        Some("install_guest_tools"),
                                        Some("Install guest tools"),
                                    ));
                                }

                                for msg in gh.issues {
                                    total += 1;
                                    issues.push(issue(
                                        "guest_issue",
                                        "warning",
                                        msg,
                                        "Review guest agent and VM logs",
                                        None,
                                        None,
                                    ));
                                }
                                None
                            }
                            Err(e) => Some(e.to_string()),
                        }
                    }
                    Err(e) => Some(e.to_string()),
                },
                Err(e) => Some(e.to_string()),
            };

            if let Some(e) = probe_err {
                guest_tools_status = "unknown".into();
                total += 1;
                issues.push(issue(
                    "guest_agent_unreachable",
                    "warning",
                    format!("Could not verify guest health: {e}"),
                    "Verify machina-agent is reachable on the VM's host and retry",
                    None,
                    None,
                ));
            }
        }
    }

    let _ = crate::db::query(
        "UPDATE vms SET guest_tools_status = ?, guest_ip = ?, guest_hostname = ?,
         os_family = COALESCE(?, os_family), updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&guest_tools_status)
    .bind(&guest_ip)
    .bind(&guest_hostname)
    .bind(os_family.as_deref())
    .bind(vm_id)
    .execute(pool)
    .await;

    let healthy = issues.is_empty();
    let score_label: String = if healthy {
        "healthy".into()
    } else if issues.iter().any(|i| i.severity == "critical") {
        "critical".into()
    } else {
        "warning".into()
    };
    let score_numeric = if total > 0 {
        ((passed as f32 / total as f32) * 100.0).round() as u8
    } else {
        100
    };
    let score = score_label.clone();

    Ok(VmHealthReport {
        vm_id,
        vm_name: name,
        score,
        score_numeric,
        score_label,
        healthy,
        checks_passed: passed,
        checks_total: total,
        issues,
        guest_tools_status,
        guest_ip,
        guest_hostname,
    })
}

async fn host_agent_addr(pool: &DbPool, host_id: Uuid) -> anyhow::Result<String> {
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    Ok(addr)
}

pub async fn sync_guest_tools(pool: &DbPool, vm_id: Uuid, vm_name: &str, host_id: Uuid) {
    let Ok(addr) = host_agent_addr(pool, host_id).await else {
        return;
    };
    let Ok(mut client) = agent_client::connect(&addr).await else {
        return;
    };
    let Ok(gh) = agent_client::get_guest_health(&mut client, vm_name).await else {
        return;
    };
    let status = if gh.agent_reachable {
        if gh.healthy {
            "healthy"
        } else {
            "installed"
        }
    } else {
        "not_installed"
    };
    let _ = crate::db::query(
        "UPDATE vms SET guest_tools_status = ?, guest_ip = ?, guest_hostname = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(if gh.guest_ip.is_empty() { None::<String> } else { Some(gh.guest_ip) })
    .bind(if gh.guest_hostname.is_empty() { None::<String> } else { Some(gh.guest_hostname) })
    .bind(vm_id)
    .execute(pool)
    .await;
}
