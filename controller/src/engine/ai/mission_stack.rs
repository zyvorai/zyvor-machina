// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::AuthUser;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize)]
pub struct MissionStackPlan {
    pub label: String,
    pub review: String,
    pub gpu_node_count: i32,
    pub inference_ready: bool,
    pub preview_only: bool,
    pub estimated_monthly_usd: f64,
    pub network_monthly_usd: f64,
    pub phases: Vec<MissionStackPhase>,
}

#[derive(Debug, Serialize)]
pub struct MissionStackPhase {
    pub name: String,
    pub steps: Vec<String>,
    pub automated: bool,
}

pub fn plan_mission_stack(query: &str, vcpu_rate: f64, gib_rate: f64) -> MissionStackPlan {
    let ql = query.to_lowercase();
    let gpu_nodes = if ql.contains("cluster") { 4 } else { 2 };
    let vcpus = 16 * gpu_nodes;
    let mem_gib = 64 * gpu_nodes;
    let hourly = vcpus as f64 * vcpu_rate + mem_gib as f64 * gib_rate;
    let estimated_monthly_usd = hourly * 730.0;
    let network_monthly_usd = machina_core::mission_stack_network_cost(gpu_nodes);

    let label = if ql.contains("llama") || ql.contains("inference") {
        "GPU inference stack for Llama serving".into()
    } else if ql.contains("gpu") {
        "GPU compute cluster".into()
    } else {
        "AI mission stack".into()
    };

    let phases = vec![
        MissionStackPhase {
            name: "Infrastructure".into(),
            steps: vec![
                format!("Create {gpu_nodes} GPU-capable VMs (16 vCPU, 64 GiB each)"),
                "Create isolated high-bandwidth network segment".into(),
                "Provision NVMe storage pool for model weights".into(),
            ],
            automated: true,
        },
        MissionStackPhase {
            name: "Kubernetes".into(),
            steps: vec![
                "Install K3s/RKE2 control plane on node-01".into(),
                "Join worker nodes with GPU taints".into(),
                "Install NVIDIA GPU Operator / device plugin".into(),
            ],
            automated: false,
        },
        MissionStackPhase {
            name: "Inference".into(),
            steps: vec![
                "Deploy vLLM or TGI Helm chart".into(),
                "Mount model volume from storage pool".into(),
                "Expose inference API behind platform load balancer".into(),
            ],
            automated: false,
        },
    ];

    MissionStackPlan {
        review: format!(
            "{gpu_nodes} GPU nodes · ~${:.0}/mo infra · ${:.0}/mo network · K8s + inference phases require review",
            estimated_monthly_usd, network_monthly_usd
        ),
        label,
        gpu_node_count: gpu_nodes,
        inference_ready: false,
        preview_only: true,
        estimated_monthly_usd: estimated_monthly_usd + network_monthly_usd,
        network_monthly_usd,
        phases,
    }
}

#[derive(Debug, Deserialize)]
pub struct MissionStackExecuteBody {
    pub query: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct StackVmTask {
    pub name: String,
    pub host: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MissionStackExecuteResult {
    pub dry_run: bool,
    pub plan: MissionStackPlan,
    pub vm_tasks: Vec<StackVmTask>,
    pub summary: String,
}

fn gpu_vm_spec(name: &str) -> serde_json::Value {
    serde_json::json!({
        "api_version": "virt.zyvor.dev/v1",
        "kind": "VirtualMachine",
        "metadata": { "name": name, "project": "mission-stack" },
        "spec": {
            "cpu": { "sockets": 1, "cores": 16 },
            "memory": "64Gi",
            "storage": [{ "name": "root", "size": "100Gi", "class": "silver" }],
            "network": [{ "network": "default", "ip_mode": "dhcp" }],
            "firmware": "bios",
            "graphics": { "type": "vnc", "listen": "127.0.0.1" }
        }
    })
}

pub async fn execute_stack(
    state: &AppState,
    actor: &AuthUser,
    body: &MissionStackExecuteBody,
) -> Result<MissionStackExecuteResult, ApiError> {
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let plan = plan_mission_stack(&body.query, rates.0, rates.1);
    let mut vm_tasks = Vec::new();

    for i in 1..=plan.gpu_node_count {
        let name = format!("gpu-stack-{i:02}");
        let host_id = crate::engine::placement::pick_host_for_vm(
            &state.pool,
            &["gpu".into(), "mission-stack".into()],
            65536,
        )
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

        let hostname: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if body.dry_run {
            vm_tasks.push(StackVmTask {
                name: name.clone(),
                host: hostname,
                task_id: None,
            });
            continue;
        }

        crate::auth::require_admin(actor)?;

        let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let vm_id = Uuid::new_v4();
        let spec_json = gpu_vm_spec(&name);
        let tags: Vec<String> = vec!["gpu".into(), "mission-stack".into()];
        let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());

        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        crate::db::query(
            "INSERT INTO vms (id, cluster_id, host_id, name, project, spec_json, desired_state, lifecycle_phase, vcpus, memory_mib, tags)
             VALUES (?, ?, ?, ?, 'mission-stack', ?, 'running', 'creating', 16, 65536, ?)",
        )
        .bind(vm_id)
        .bind(cluster_id)
        .bind(host_id)
        .bind(&name)
        .bind(&spec_json)
        .bind(&tags_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        crate::db::query(
            "INSERT INTO vm_disks (id, vm_id, name, size_gib, storage_class) VALUES (?, ?, 'root', 100, 'silver')",
        )
        .bind(Uuid::new_v4())
        .bind(vm_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let task_id = enqueue_task(
            state,
            "vm.apply",
            serde_json::json!({
                "vm_id": vm_id.to_string(),
                "host_id": host_id.to_string(),
            }),
            Some("vm"),
            Some(vm_id),
            Some(host_id),
        )
        .await?;

        vm_tasks.push(StackVmTask {
            name,
            host: hostname,
            task_id: Some(task_id.to_string()),
        });
    }

    let summary = if body.dry_run {
        format!(
            "Preview: would create {} GPU VM(s) for infrastructure phase.",
            vm_tasks.len()
        )
    } else {
        format!(
            "Enqueued {} GPU VM create task(s) for mission stack.",
            vm_tasks.len()
        )
    };

    state.emit_event("ai.mission_stack", summary.clone());

    Ok(MissionStackExecuteResult {
        dry_run: body.dry_run,
        plan,
        vm_tasks,
        summary,
    })
}
