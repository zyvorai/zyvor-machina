// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct GpuHostCandidate {
    pub host_id: String,
    pub hostname: String,
    pub gpu_capable: bool,
    pub numa_hint: String,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct GpuPlacementReport {
    pub workload: String,
    pub candidates: Vec<GpuHostCandidate>,
    pub summary: String,
}

pub async fn advise_gpu(pool: &DbPool, workload: &str) -> anyhow::Result<GpuPlacementReport> {
    let rows: Vec<(
        Uuid,
        String,
        f32,
        i64,
        i64,
        i32,
        crate::db::Json<Vec<String>>,
    )> = crate::db::query_as(
        "SELECT id, hostname, cpu_percent, memory_used_mib, memory_total_mib, vm_count,
                COALESCE(tags, '[]') AS tags
         FROM hosts WHERE state = 'online' AND maintenance_mode = FALSE ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let mut candidates = Vec::new();
    for (id, hostname, cpu, mem_used, mem_total, vm_count, tags) in rows {
        let gpu_capable = tags.iter().any(|t| {
            let tl = t.to_lowercase();
            tl.contains("gpu") || tl == "nvidia" || tl == "inference"
        });
        let mem_pct = if mem_total > 0 {
            mem_used as f32 / mem_total as f32 * 100.0
        } else {
            0.0
        };
        let mut score = 50.0_f32;
        if gpu_capable {
            score += 35.0;
        }
        score += (100.0 - cpu.min(95.0)) * 0.2;
        score += (100.0 - mem_pct.min(95.0)) * 0.15;
        score -= vm_count as f32 * 1.5;

        let numa_hint = if gpu_capable {
            "Prefer local NUMA node for GPU passthrough / vGPU".into()
        } else {
            "No GPU tag — use for CPU inference fallback only".into()
        };

        let reason = if gpu_capable {
            format!("GPU-tagged host · CPU {cpu:.0}% · {vm_count} VMs")
        } else {
            format!("General compute · CPU {cpu:.0}% · mem {mem_pct:.0}%")
        };

        candidates.push(GpuHostCandidate {
            host_id: id.to_string(),
            hostname,
            gpu_capable,
            numa_hint,
            score,
            reason,
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let gpu_count = candidates.iter().filter(|c| c.gpu_capable).count();
    let summary = if gpu_count > 0 {
        format!(
            "{} GPU-capable host(s) for '{workload}' — top: {}",
            gpu_count,
            candidates
                .first()
                .map(|c| c.hostname.as_str())
                .unwrap_or("—")
        )
    } else {
        format!(
            "No GPU-tagged hosts — tag hosts with gpu/nvidia for affinity. Workload: {workload}"
        )
    };

    Ok(GpuPlacementReport {
        workload: workload.into(),
        candidates: candidates.into_iter().take(8).collect(),
        summary,
    })
}
