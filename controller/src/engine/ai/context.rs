// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

use crate::config::ControllerConfig;

#[derive(Debug, Clone, Serialize)]
pub struct AssembledContext {
    pub cluster_vms: i64,
    pub cluster_hosts_online: i64,
    pub recent_tasks: Vec<TaskBrief>,
    pub recent_events: Vec<String>,
    pub recommendations_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmBrief>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<HostBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostBrief {
    pub id: String,
    pub hostname: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_pressure_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskBrief {
    pub operation: String,
    pub status: String,
    pub progress: i16,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmBrief {
    pub id: String,
    pub name: String,
    pub state: String,
    pub vcpus: i32,
    pub memory_mib: i64,
}

pub async fn assemble(
    pool: &DbPool,
    cfg: &ControllerConfig,
    vm_id: Option<Uuid>,
    host_id: Option<Uuid>,
) -> anyhow::Result<AssembledContext> {
    let cluster_vms: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vms")
        .fetch_one(pool)
        .await?;
    let cluster_hosts_online: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM hosts WHERE state = 'online'")
            .fetch_one(pool)
            .await?;

    let recent_tasks: Vec<TaskBrief> = crate::db::query_as(
        "SELECT operation, status, progress FROM tasks ORDER BY created_at DESC LIMIT 8",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(operation, status, progress)| TaskBrief {
        operation,
        status,
        progress,
    })
    .collect();

    let recent_events: Vec<String> = crate::db::query_scalar(
        "SELECT kind || ': ' || COALESCE(message, '') FROM events ORDER BY created_at DESC LIMIT 6",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let recommendations_count = crate::engine::recommendations::generate_recommendations(pool)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);

    let vm = if let Some(id) = vm_id {
        crate::db::query_as::<_, (Uuid, String, String, i32, i64)>(
            "SELECT id, name, observed_state, vcpus, memory_mib FROM vms WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .map(|(id, name, state, vcpus, memory_mib)| VmBrief {
            id: id.to_string(),
            name,
            state,
            vcpus,
            memory_mib,
        })
    } else {
        None
    };

    let host = if let Some(id) = host_id {
        let row: Option<(String, String)> =
            crate::db::query_as("SELECT hostname, state FROM hosts WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        let mut brief = row.map(|(hostname, state)| HostBrief {
            id: id.to_string(),
            hostname,
            state,
            io_pressure_pct: None,
            linux_summary: None,
        });
        if let Some(ref mut h) = brief {
            if let Ok(obs) = crate::engine::host_os::linux_observability(pool, cfg, id).await {
                // `avg10` from `/proc/pressure/io` is already a 0-100
                // percentage — multiplying by 100 here inflated it 100x.
                let io = obs
                    .get("pressure")
                    .and_then(|p| p.get("io"))
                    .and_then(|i| i.get("some"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                h.io_pressure_pct = Some(io);
                h.linux_summary = Some(format!("IO PSI {:.0}%", io));
            }
        }
        brief
    } else {
        None
    };

    Ok(AssembledContext {
        cluster_vms,
        cluster_hosts_online,
        recent_tasks,
        recent_events,
        recommendations_count,
        vm,
        host,
    })
}
