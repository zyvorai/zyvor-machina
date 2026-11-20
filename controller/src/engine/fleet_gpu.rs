// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// GPU Command Center rollup — host tags + VM inventory (Phase 54 v1).

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GpuProfileKind {
    Mig,
    Vgpu,
    Passthrough,
    Cuda,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuHostItem {
    pub host_id: String,
    pub hostname: String,
    pub site: String,
    pub rack: String,
    pub state: String,
    pub gpu_capable: bool,
    pub profile: GpuProfileKind,
    pub model_hint: String,
    pub vm_count: i32,
    pub gpu_vm_count: i32,
    pub vgpu_slices: i32,
    pub cuda_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuVmItem {
    pub vm_id: String,
    pub vm_name: String,
    pub host_id: Option<String>,
    pub hostname: Option<String>,
    pub observed_state: String,
    pub profile: GpuProfileKind,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuProfileSummary {
    pub kind: GpuProfileKind,
    pub label: String,
    pub host_count: i32,
    pub vm_count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetGpuOverview {
    pub summary: String,
    pub gpu_host_count: i64,
    pub gpu_vm_count: i64,
    pub cuda_ready_hosts: i64,
    pub mig_hosts: i64,
    pub vgpu_hosts: i64,
    pub hosts: Vec<GpuHostItem>,
    pub vms: Vec<GpuVmItem>,
    pub profiles: Vec<GpuProfileSummary>,
}

fn tag_profile(tags: &[String]) -> GpuProfileKind {
    let joined = tags
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.contains("mig") {
        GpuProfileKind::Mig
    } else if joined.contains("vgpu") {
        GpuProfileKind::Vgpu
    } else if joined.contains("cuda") {
        GpuProfileKind::Cuda
    } else if joined.contains("gpu") || joined.contains("nvidia") || joined.contains("passthrough")
    {
        GpuProfileKind::Passthrough
    } else {
        GpuProfileKind::Unknown
    }
}

fn gpu_capable(tags: &[String]) -> bool {
    tags.iter().any(|t| {
        let tl = t.to_lowercase();
        tl.contains("gpu")
            || tl == "nvidia"
            || tl.contains("mig")
            || tl.contains("vgpu")
            || tl.contains("cuda")
            || tl.contains("inference")
    })
}

fn model_hint(tags: &[String]) -> String {
    for t in tags {
        let tl = t.to_lowercase();
        if tl.starts_with("gpu:") || tl.starts_with("nvidia:") {
            return t
                .split_once(':')
                .map(|(_, m)| m.to_string())
                .unwrap_or_else(|| t.clone());
        }
        if tl.contains("a100") || tl.contains("h100") || tl.contains("l40") || tl.contains("rtx") {
            return t.clone();
        }
    }
    if gpu_capable(tags) {
        "GPU host (tag inventory)".into()
    } else {
        "—".into()
    }
}

fn vgpu_slices_from_tags(tags: &[String]) -> i32 {
    for t in tags {
        if let Some(rest) = t.to_lowercase().strip_prefix("vgpu:") {
            if let Ok(n) = rest.parse::<i32>() {
                return n.max(1);
            }
        }
    }
    if tag_profile(tags) == GpuProfileKind::Vgpu {
        4
    } else {
        0
    }
}

pub async fn overview(pool: &DbPool) -> anyhow::Result<FleetGpuOverview> {
    let host_rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        i32,
        crate::db::Json<Vec<String>>,
    )> = crate::db::query_as(
        "SELECT id, hostname, state, COALESCE(site, ''), COALESCE(rack, ''), vm_count,
                COALESCE(tags, '[]') AS tags
         FROM hosts ORDER BY hostname",
    )
    .fetch_all(pool)
    .await?;

    let vm_rows: Vec<(Uuid, String, Option<Uuid>, String, crate::db::Json<Vec<String>>)> = crate::db::query_as(
        "SELECT id, name, host_id, observed_state, COALESCE(tags, '[]') AS tags FROM vms ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    let host_names: std::collections::HashMap<Uuid, String> = host_rows
        .iter()
        .map(|(id, hostname, ..)| (*id, hostname.clone()))
        .collect();

    let mut gpu_vms = Vec::new();
    for (vm_id, name, host_id, state, tags) in &vm_rows {
        if !gpu_capable(tags) {
            continue;
        }
        gpu_vms.push(GpuVmItem {
            vm_id: vm_id.to_string(),
            vm_name: name.clone(),
            host_id: host_id.map(|h| h.to_string()),
            hostname: host_id.and_then(|h| host_names.get(&h).cloned()),
            observed_state: state.clone(),
            profile: tag_profile(tags),
            tags: (**tags).clone(),
        });
    }

    let gpu_vm_by_host: std::collections::HashMap<String, i32> = gpu_vms
        .iter()
        .filter_map(|v| v.host_id.as_ref().map(|h| (h.clone(), 1)))
        .fold(std::collections::HashMap::new(), |mut acc, (h, n)| {
            *acc.entry(h).or_insert(0) += n;
            acc
        });

    let mut hosts = Vec::new();
    for (id, hostname, state, site, rack, vm_count, tags) in host_rows {
        let capable = gpu_capable(&tags);
        if !capable {
            continue;
        }
        let profile = tag_profile(&tags);
        let hid = id.to_string();
        let gpu_vm_count = gpu_vm_by_host.get(&hid).copied().unwrap_or(0);
        hosts.push(GpuHostItem {
            host_id: hid,
            hostname,
            site,
            rack,
            state,
            gpu_capable: capable,
            profile: profile.clone(),
            model_hint: model_hint(&tags),
            vm_count,
            gpu_vm_count,
            vgpu_slices: vgpu_slices_from_tags(&tags),
            cuda_ready: profile == GpuProfileKind::Cuda
                || tags.iter().any(|t| t.to_lowercase().contains("cuda")),
        });
    }

    let gpu_host_count = hosts.len() as i64;
    let gpu_vm_count = gpu_vms.len() as i64;
    let cuda_ready_hosts = hosts.iter().filter(|h| h.cuda_ready).count() as i64;
    let mig_hosts = hosts
        .iter()
        .filter(|h| h.profile == GpuProfileKind::Mig)
        .count() as i64;
    let vgpu_hosts = hosts
        .iter()
        .filter(|h| h.profile == GpuProfileKind::Vgpu)
        .count() as i64;

    let mut profile_counts: std::collections::HashMap<GpuProfileKind, (i32, i32)> =
        std::collections::HashMap::new();
    for h in &hosts {
        let e = profile_counts.entry(h.profile.clone()).or_insert((0, 0));
        e.0 += 1;
        e.1 += h.gpu_vm_count;
    }
    for v in &gpu_vms {
        let e = profile_counts.entry(v.profile.clone()).or_insert((0, 0));
        e.1 += 1;
    }

    let profile_label = |k: &GpuProfileKind| -> &'static str {
        match k {
            GpuProfileKind::Mig => "MIG partitions",
            GpuProfileKind::Vgpu => "vGPU slices",
            GpuProfileKind::Passthrough => "GPU passthrough",
            GpuProfileKind::Cuda => "CUDA ready",
            GpuProfileKind::Unknown => "General GPU",
        }
    };

    let mut profiles: Vec<GpuProfileSummary> = profile_counts
        .into_iter()
        .map(|(kind, (host_count, vm_count))| GpuProfileSummary {
            kind: kind.clone(),
            label: profile_label(&kind).into(),
            host_count,
            vm_count,
        })
        .collect();
    profiles.sort_by_key(|x| std::cmp::Reverse(x.host_count));

    let summary = if gpu_host_count > 0 {
        format!(
            "{gpu_host_count} GPU host(s) · {gpu_vm_count} GPU VM(s) · {cuda_ready_hosts} CUDA-ready"
        )
    } else {
        "No GPU-tagged hosts — add gpu, nvidia, mig, vgpu, or cuda tags on host detail.".into()
    };

    Ok(FleetGpuOverview {
        summary,
        gpu_host_count,
        gpu_vm_count,
        cuda_ready_hosts,
        mig_hosts,
        vgpu_hosts,
        hosts,
        vms: gpu_vms,
        profiles,
    })
}
