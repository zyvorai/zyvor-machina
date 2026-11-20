// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Infrastructure DNA — fleet health score 0–100 (Phase 56 v1).

use serde::Serialize;
use crate::db::DbPool;

use crate::config::ControllerConfig;
use crate::engine::fleet_linux;
use crate::engine::fleet_mission;
use crate::engine::fleet_storage;
use crate::engine::fleet_updates;
use crate::engine::operations;

#[derive(Debug, Clone, Serialize)]
pub struct DnaPillar {
    pub id: String,
    pub label: String,
    pub score: i32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetDnaOverview {
    pub score: i32,
    pub grade: String,
    pub summary: String,
    pub pillars: Vec<DnaPillar>,
}

fn grade_from_score(score: i32) -> String {
    match score {
        90..=100 => "A".into(),
        80..=89 => "B".into(),
        70..=79 => "C".into(),
        60..=69 => "D".into(),
        _ => "F".into(),
    }
}

fn compliance_score_from_grade(grade: &str) -> i32 {
    match grade.trim().to_uppercase().chars().next().unwrap_or('C') {
        'A' => 95,
        'B' => 82,
        'C' => 72,
        'D' => 62,
        _ => 50,
    }
}

pub async fn overview(
    pool: &DbPool,
    cfg: &ControllerConfig,
) -> anyhow::Result<FleetDnaOverview> {
    let mission = fleet_mission::overview(pool).await?;
    let updates = fleet_updates::overview(pool, cfg).await?;
    let linux = fleet_linux::overview(pool, cfg).await?;
    let storage = fleet_storage::overview(pool, cfg)
        .await
        .unwrap_or_else(|_| fleet_storage::FleetStorageOverview {
            summary: "Storage unavailable".into(),
            pool_count: 0,
            tier_count: 0,
            total_capacity_gib: 0,
            total_used_gib: 0,
            pools_over_85_pct: 0,
            smart_failure_count: 0,
            smart_hosts_affected: 0,
            smart_hosts_sampled: 0,
            pools: vec![],
            smart_disks: vec![],
        });
    let ops = operations::overview(pool).await.ok();

    let availability = mission.summary.health_pct.clamp(0, 100) as i32;

    let patch_hygiene = if updates.hosts_scanned == 0 {
        100
    } else {
        let clean = updates
            .hosts_scanned
            .saturating_sub(updates.hosts_with_updates);
        ((clean as f64 / updates.hosts_scanned as f64) * 100.0).round() as i32
    };

    let linux_penalty = (linux
        .pressure_hosts
        .saturating_add(linux.thermal_alerts)
        .saturating_add(linux.smart_alerts) as i32)
        .saturating_mul(8);
    let linux_health = (100 - linux_penalty).clamp(0, 100);

    let storage_penalty = (storage
        .pools_over_85_pct
        .saturating_mul(10)
        .saturating_add(storage.smart_failure_count.saturating_mul(5))
        .min(100)) as i32;
    let backup_posture = (100 - storage_penalty).clamp(0, 100);

    let mut pillars = vec![
        DnaPillar {
            id: "availability".into(),
            label: "Availability".into(),
            score: availability,
            detail: format!(
                "{}% fleet health · {} hosts online",
                mission.summary.health_pct, mission.summary.hosts_online
            ),
        },
        DnaPillar {
            id: "patch_hygiene".into(),
            label: "Patch hygiene".into(),
            score: patch_hygiene,
            detail: format!(
                "{} of {} hosts need OS updates",
                updates.hosts_with_updates, updates.hosts_scanned
            ),
        },
        DnaPillar {
            id: "linux_health".into(),
            label: "Linux health".into(),
            score: linux_health,
            detail: linux.summary,
        },
        DnaPillar {
            id: "backup_posture".into(),
            label: "Storage posture".into(),
            score: backup_posture,
            detail: storage.summary,
        },
    ];

    if let Some(o) = ops {
        let comp = compliance_score_from_grade(&o.compliance_grade);
        pillars.push(DnaPillar {
            id: "compliance".into(),
            label: "Compliance".into(),
            score: comp,
            detail: format!("Fleet ops grade {}", o.compliance_grade),
        });
    }

    let score = if pillars.is_empty() {
        100
    } else {
        (pillars.iter().map(|p| p.score).sum::<i32>() as f64 / pillars.len() as f64).round() as i32
    };
    let grade = grade_from_score(score);

    Ok(FleetDnaOverview {
        score,
        grade: grade.clone(),
        summary: format!("Infrastructure DNA {score}/100 (grade {grade})"),
        pillars,
    })
}
