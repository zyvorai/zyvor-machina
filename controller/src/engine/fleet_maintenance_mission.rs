// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Maintenance Mission plan — 7-step guided patch timeline (Phase 55 v1).

use serde::Serialize;
use crate::db::DbPool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::fleet_updates;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStepId {
    Scan,
    Assess,
    Schedule,
    EnterMaintenance,
    Evacuate,
    ApplyPreview,
    VerifyExit,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Ready,
    Done,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionStepState {
    pub id: MissionStepId,
    pub label: String,
    pub status: StepStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceMissionHost {
    pub host_id: String,
    pub hostname: String,
    pub state: String,
    pub maintenance_mode: bool,
    pub validation_status: String,
    pub pending_packages: Option<u32>,
    pub reboot_required: bool,
    pub agent_drift: bool,
    pub update_summary: Option<String>,
    pub recommended_step: MissionStepId,
    pub steps: Vec<MissionStepState>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetMaintenanceMissionOverview {
    pub summary: String,
    pub hosts_with_updates: usize,
    pub hosts_in_maintenance: usize,
    pub pending_schedules: usize,
    pub hosts: Vec<MaintenanceMissionHost>,
}

#[allow(clippy::too_many_arguments)]
fn build_steps(
    scanned: bool,
    assessed: bool,
    has_schedule: bool,
    in_maintenance: bool,
    evacuate_planned: bool,
    needs_updates: bool,
    host_online: bool,
    validation_ok: bool,
    update_summary: &Option<String>,
) -> (Vec<MissionStepState>, MissionStepId, Vec<String>) {
    let mut blockers = Vec::new();

    let scan_status = if scanned {
        StepStatus::Done
    } else {
        StepStatus::Pending
    };

    let assess_status = if !scanned {
        StepStatus::Blocked
    } else if assessed {
        StepStatus::Done
    } else {
        StepStatus::Pending
    };

    let schedule_status = if !needs_updates && !assessed {
        StepStatus::Skipped
    } else if !assessed {
        StepStatus::Blocked
    } else if has_schedule {
        StepStatus::Done
    } else {
        StepStatus::Ready
    };

    let enter_status = if !needs_updates && validation_ok && host_online && !in_maintenance {
        StepStatus::Skipped
    } else if !has_schedule && needs_updates {
        blockers.push("Schedule a maintenance window before entering maintenance.".into());
        StepStatus::Blocked
    } else if in_maintenance {
        StepStatus::Done
    } else if assessed && (has_schedule || !needs_updates) {
        StepStatus::Ready
    } else {
        StepStatus::Pending
    };

    let evacuate_status = if !evacuate_planned {
        StepStatus::Skipped
    } else if in_maintenance {
        StepStatus::Done
    } else if has_schedule {
        StepStatus::Ready
    } else {
        StepStatus::Blocked
    };

    let preview_status = if !needs_updates {
        StepStatus::Skipped
    } else if in_maintenance {
        StepStatus::Ready
    } else {
        StepStatus::Blocked
    };

    let verify_status = if in_maintenance {
        StepStatus::Pending
    } else if host_online && validation_ok && !needs_updates {
        StepStatus::Done
    } else if !in_maintenance && needs_updates {
        StepStatus::Pending
    } else if host_online && !in_maintenance {
        StepStatus::Ready
    } else {
        StepStatus::Pending
    };

    let steps = vec![
        MissionStepState {
            id: MissionStepId::Scan,
            label: "Scan fleet".into(),
            status: scan_status,
            detail: None,
        },
        MissionStepState {
            id: MissionStepId::Assess,
            label: "Assess risk".into(),
            status: assess_status,
            detail: update_summary.clone(),
        },
        MissionStepState {
            id: MissionStepId::Schedule,
            label: "Schedule window".into(),
            status: schedule_status,
            detail: None,
        },
        MissionStepState {
            id: MissionStepId::EnterMaintenance,
            label: "Enter maintenance".into(),
            status: enter_status,
            detail: None,
        },
        MissionStepState {
            id: MissionStepId::Evacuate,
            label: "Evacuate VMs".into(),
            status: evacuate_status,
            detail: None,
        },
        MissionStepState {
            id: MissionStepId::ApplyPreview,
            label: "Apply preview".into(),
            status: preview_status,
            detail: Some(
                "Preview or apply via platform — host must be in maintenance mode.".into(),
            ),
        },
        MissionStepState {
            id: MissionStepId::VerifyExit,
            label: "Verify & exit".into(),
            status: verify_status,
            detail: None,
        },
    ];

    let recommended = steps
        .iter()
        .find(|s| {
            matches!(
                s.status,
                StepStatus::Pending | StepStatus::Ready | StepStatus::Blocked
            )
        })
        .map(|s| s.id.clone())
        .unwrap_or(MissionStepId::VerifyExit);

    (steps, recommended, blockers)
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetMaintenanceMissionOverview> {
    let updates = fleet_updates::overview(pool, cfg).await?;

    let host_rows: Vec<(Uuid, String, String, bool, String)> = crate::db::query_as(
        "SELECT id, hostname, state, maintenance_mode, COALESCE(validation_status, 'pending') FROM hosts ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let schedule_rows: Vec<(Uuid, bool, String)> = crate::db::query_as(
        "SELECT host_id, evacuate, status FROM maintenance_schedules WHERE status IN ('pending', 'queued')",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut schedule_by_host: HashMap<Uuid, (bool, String)> = HashMap::new();
    for (host_id, evacuate, status) in schedule_rows {
        schedule_by_host
            .entry(host_id)
            .or_insert((evacuate, status));
    }

    let update_by_host: HashMap<String, &fleet_updates::FleetHostUpdateItem> = updates
        .hosts
        .iter()
        .map(|h| (h.host_id.clone(), h))
        .collect();

    let mut hosts = Vec::new();
    let mut hosts_in_maintenance = 0usize;

    for (id, hostname, state, maintenance_mode, validation_status) in host_rows {
        if maintenance_mode {
            hosts_in_maintenance += 1;
        }
        let hid = id.to_string();
        let upd = update_by_host.get(&hid);
        let scanned = upd.is_some();
        let pending_packages = upd.and_then(|u| u.pending_count);
        let reboot_required = upd.map(|u| u.reboot_required).unwrap_or(false);
        let agent_drift = upd.map(|u| u.agent_update_available).unwrap_or(false);
        let update_summary = upd.and_then(|u| u.summary.clone());
        let needs_updates = upd
            .map(|u| u.status == "updates" || u.reboot_required || u.agent_update_available)
            .unwrap_or(false);
        let assessed = scanned;
        let (has_schedule, evacuate_planned) = schedule_by_host
            .get(&id)
            .map(|(ev, _)| (true, *ev))
            .unwrap_or((false, false));
        let host_online = state == "online";
        let validation_ok = validation_status == "ok" || validation_status == "valid";

        let (steps, recommended_step, blockers) = build_steps(
            scanned,
            assessed,
            has_schedule,
            maintenance_mode,
            evacuate_planned,
            needs_updates,
            host_online,
            validation_ok,
            &update_summary,
        );

        hosts.push(MaintenanceMissionHost {
            host_id: hid,
            hostname,
            state,
            maintenance_mode,
            validation_status,
            pending_packages,
            reboot_required,
            agent_drift,
            update_summary,
            recommended_step,
            steps,
            blockers,
        });
    }

    let pending_schedules = schedule_by_host.len();
    Ok(FleetMaintenanceMissionOverview {
        summary: format!(
            "{} host(s) with updates · {} in maintenance · {} pending schedule(s)",
            updates.hosts_with_updates, hosts_in_maintenance, pending_schedules
        ),
        hosts_with_updates: updates.hosts_with_updates,
        hosts_in_maintenance,
        pending_schedules,
        hosts,
    })
}
