// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::guest_context::{self, GuestAiSnapshot};
use crate::engine::guestkit_bridge::{self, GuestkitDoctorReport, GuestkitMigratePlanReport};

use super::llm::{self, CompletionRequest};
use super::migration;
use super::routing::TaskClass;

fn vm_is_stopped(state: &str) -> bool {
    matches!(
        state,
        "stopped" | "shut off" | "shutoff" | "Shutoff" | "Shut Off"
    )
}

#[derive(Debug, Deserialize)]
pub struct MigrationReadinessRequest {
    #[serde(default)]
    pub vm_ids: Vec<Uuid>,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmMigrationReadinessRow {
    pub vm_id: String,
    pub vm_name: String,
    pub readiness_percent: u8,
    pub install_state: String,
    pub os_pretty_name: String,
    pub guest_ip: String,
    pub qga_gaps: Vec<String>,
    pub remediation: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assurance_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guestkit_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReadinessReport {
    pub executive_summary: String,
    pub vm_count: usize,
    pub rows: Vec<VmMigrationReadinessRow>,
    pub prioritized_remediation: Vec<String>,
    pub llm_powered: bool,
}

pub async fn generate(
    pool: &DbPool,
    cfg: &ControllerConfig,
    req: &MigrationReadinessRequest,
) -> anyhow::Result<MigrationReadinessReport> {
    let vm_ids = if req.vm_ids.is_empty() {
        crate::db::query_scalar::<_, Uuid>(
            "SELECT id FROM vms WHERE COALESCE(inventory_source, 'libvirt') = 'libvirt' ORDER BY name LIMIT 25",
        )
        .fetch_all(pool)
        .await?
    } else {
        req.vm_ids.clone()
    };

    let provider = req.provider.as_deref().unwrap_or("vmware");
    let meta: Vec<(Uuid, String, String)> = if vm_ids.is_empty() {
        Vec::new()
    } else {
        let placeholders = std::iter::repeat_n("?", vm_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id, name, observed_state FROM vms WHERE id IN ({placeholders})");
        let mut q = crate::db::query_as::<_, (Uuid, String, String)>(&sql);
        for id in &vm_ids {
            q = q.bind(id);
        }
        q.fetch_all(pool).await?
    };

    let mut running_ids = Vec::new();
    let mut stopped_ids = Vec::new();
    for (id, _name, state) in &meta {
        if vm_is_stopped(state) {
            stopped_ids.push(*id);
        } else if state == "running" || state == "paused" {
            running_ids.push(*id);
        } else {
            stopped_ids.push(*id);
        }
    }

    let snapshots = guest_context::gather_fleet_snapshots(pool, cfg, running_ids, false).await;
    let mut rows = Vec::new();
    let mut all_remediation = Vec::new();
    let name_by_id: std::collections::HashMap<Uuid, String> = meta
        .iter()
        .map(|(id, name, _)| (*id, name.clone()))
        .collect();

    for (id, res) in snapshots {
        let vm_name = name_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        let snap = match res {
            Ok(s) => s,
            Err(e) => {
                rows.push(VmMigrationReadinessRow {
                    vm_id: id.to_string(),
                    vm_name,
                    readiness_percent: 40,
                    install_state: "unknown".into(),
                    os_pretty_name: String::new(),
                    guest_ip: String::new(),
                    qga_gaps: vec![e],
                    remediation: vec![
                        "Start VM for live QGA inventory, or enable GuestKit for offline disk scan"
                            .into(),
                    ],
                    assurance_mode: None,
                    guestkit_summary: None,
                });
                continue;
            }
        };
        let (row, rem) = row_from_snapshot(&snap, provider);
        all_remediation.extend(rem);
        rows.push(row);
    }

    for id in stopped_ids {
        let vm_name = name_by_id
            .get(&id)
            .cloned()
            .unwrap_or_else(|| id.to_string());
        if cfg.guestkit_enabled {
            match guestkit_bridge::migrate_plan_vm(cfg, pool, &cfg.disk_image_dir, id, "kvm").await
            {
                Ok(plan) => {
                    let doctor = guestkit_bridge::doctor_vm(
                        cfg,
                        pool,
                        &cfg.disk_image_dir,
                        id,
                        "kvm",
                        false,
                    )
                    .await
                    .ok();
                    let (row, rem) =
                        row_from_guestkit_offline(id, &vm_name, &plan, doctor.as_ref());
                    all_remediation.extend(rem);
                    rows.push(row);
                }
                Err(e) => {
                    rows.push(VmMigrationReadinessRow {
                        vm_id: id.to_string(),
                        vm_name,
                        readiness_percent: 35,
                        install_state: "offline".into(),
                        os_pretty_name: String::new(),
                        guest_ip: String::new(),
                        qga_gaps: vec![format!("GuestKit offline scan failed: {e}")],
                        remediation: vec![
                            "Ensure VM disk path is set in vm_disks or disk_image_dir".into(),
                            "Start VM for live guest-agent checks".into(),
                        ],
                        assurance_mode: Some("offline_guestkit".into()),
                        guestkit_summary: None,
                    });
                }
            }
        } else {
            rows.push(VmMigrationReadinessRow {
                vm_id: id.to_string(),
                vm_name,
                readiness_percent: 45,
                install_state: "offline".into(),
                os_pretty_name: String::new(),
                guest_ip: String::new(),
                qga_gaps: vec!["VM stopped — live QGA unavailable".into()],
                remediation: vec![
                    "Set GUESTKIT_ENABLED=1 for offline boot/migration scoring".into(),
                    "Start VM before cutover for guest-agent inventory".into(),
                ],
                assurance_mode: None,
                guestkit_summary: None,
            });
        }
    }

    all_remediation.sort();
    all_remediation.dedup();
    let prioritized_remediation: Vec<String> = all_remediation.into_iter().take(12).collect();

    let executive_summary = if super::settings::llm_enabled(pool).await.unwrap_or(false)
        && !rows.is_empty()
    {
        let system = "Write a 3-5 sentence executive summary for a KVM migration readiness report. Mention QGA gaps and top remediation priorities.";
        let user = serde_json::to_string(&rows)?;
        if let Ok(Some(text)) = llm::complete(
            pool,
            CompletionRequest {
                task_class: TaskClass::Infrastructure,
                system: system.to_string(),
                user,
                agent_id: None,
                user_id: None,
            },
        )
        .await
        {
            return Ok(MigrationReadinessReport {
                executive_summary: text,
                vm_count: rows.len(),
                rows,
                prioritized_remediation,
                llm_powered: true,
            });
        }
        deterministic_summary(&rows)
    } else {
        deterministic_summary(&rows)
    };

    Ok(MigrationReadinessReport {
        executive_summary,
        vm_count: rows.len(),
        rows,
        prioritized_remediation,
        llm_powered: false,
    })
}

fn row_from_snapshot(
    s: &GuestAiSnapshot,
    _provider: &str,
) -> (VmMigrationReadinessRow, Vec<String>) {
    let os_hint = if s.os_pretty_name.to_lowercase().contains("windows") {
        "windows"
    } else {
        "linux"
    };
    let mut adv = migration::advise_vmware_vm(&s.vm_name, os_hint, false);
    let mut remediation = adv.remediation.clone();
    let mut qga_gaps = Vec::new();
    let mut score = adv.readiness_percent as i32;

    if !s.agent_ping {
        qga_gaps.push("guest agent not responding".into());
        remediation
            .push("Install guestkit-agent and enable virtio channel (QGA-compatible)".into());
        score -= 15;
    }
    if s.install_state == "channel_only" {
        qga_gaps.push("Guest agent channel attached but package not running".into());
        score -= 10;
    }
    if let Some(ms) = s.time_delta_ms {
        if ms.abs() > 5000 {
            qga_gaps.push(format!("Time drift {ms} ms"));
            remediation.push("Sync guest time after migration".into());
            score -= 5;
        }
    }
    if s.guest_ip.is_empty() {
        qga_gaps.push("No guest IP discovered".into());
        score -= 10;
    }

    adv.readiness_percent = score.clamp(0, 100) as u8;

    let row = VmMigrationReadinessRow {
        vm_id: s.vm_id.clone(),
        vm_name: s.vm_name.clone(),
        readiness_percent: adv.readiness_percent,
        install_state: s.install_state.clone(),
        os_pretty_name: s.os_pretty_name.clone(),
        guest_ip: s.guest_ip.clone(),
        qga_gaps,
        remediation: remediation.clone(),
        assurance_mode: Some("live_qga".into()),
        guestkit_summary: None,
    };
    (row, remediation)
}

fn row_from_guestkit_offline(
    vm_id: Uuid,
    vm_name: &str,
    plan: &GuestkitMigratePlanReport,
    doctor: Option<&GuestkitDoctorReport>,
) -> (VmMigrationReadinessRow, Vec<String>) {
    let mut qga_gaps =
        vec!["VM stopped — live QEMU guest-agent unavailable (GuestKit offline scan)".into()];
    let mut remediation = plan.required_changes.clone();
    for w in &plan.licensing_warnings {
        remediation.push(format!("Licensing: {w}"));
    }
    for inj in &plan.driver_injections {
        remediation.push(format!("Driver injection: {inj}"));
    }
    if let Some(d) = doctor {
        for b in &d.blockers {
            qga_gaps.push(b.clone());
        }
        for w in &d.warnings {
            remediation.push(w.clone());
        }
    }
    let readiness_percent =
        ((plan.migration_score + plan.boot_score) / 2.0).clamp(0.0, 100.0) as u8;

    let row = VmMigrationReadinessRow {
        vm_id: vm_id.to_string(),
        vm_name: vm_name.to_string(),
        readiness_percent,
        install_state: "offline".into(),
        os_pretty_name: String::new(),
        guest_ip: String::new(),
        qga_gaps,
        remediation: remediation.clone(),
        assurance_mode: Some("offline_guestkit".into()),
        guestkit_summary: Some(plan.summary.clone()),
    };
    (row, remediation)
}

fn deterministic_summary(rows: &[VmMigrationReadinessRow]) -> String {
    if rows.is_empty() {
        return "No VMs with guest-agent data available for migration readiness.".into();
    }
    let avg: u32 = rows.iter().map(|r| r.readiness_percent as u32).sum::<u32>() / rows.len() as u32;
    let no_qga = rows.iter().filter(|r| !r.qga_gaps.is_empty()).count();
    let offline = rows
        .iter()
        .filter(|r| r.assurance_mode.as_deref() == Some("offline_guestkit"))
        .count();
    format!(
        "Migration readiness for {} VM(s): average score {}%. {} VM(s) have QGA or networking gaps; {} assessed via GuestKit offline disk scan. Enable live guest agent before cutover where possible.",
        rows.len(),
        avg,
        no_qga,
        offline
    )
}
