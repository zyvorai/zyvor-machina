// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::AuthUser;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

use super::intent_router::SpotlightIntent;

#[derive(Debug, Serialize)]
pub struct EnvironmentResourcePlan {
    pub label: String,
    pub review: String,
    pub developer_count: i32,
    pub vm_count: i32,
    pub vcpus_per_vm: i32,
    pub memory_gib_per_vm: i32,
    pub total_vcpus: i32,
    pub total_memory_gib: i32,
    pub storage_gib: i32,
    pub network: String,
    pub backup_policy: String,
    pub estimated_monthly_usd: f64,
    pub environment_type: String,
    pub gpu_required: bool,
    pub preview_only: bool,
    pub build_steps: Vec<String>,
    pub spotlight_intents: Vec<SpotlightIntent>,
}

pub fn plan_environment(query: &str, vcpu_rate: f64, gib_rate: f64) -> EnvironmentResourcePlan {
    let ql = query.to_lowercase();
    let gpu_required = ql.contains("gpu")
        || ql.contains("llama")
        || ql.contains("inference")
        || ql.contains("cuda")
        || ql.contains("training");

    let developers = extract_count(
        &ql,
        &[
            "developer",
            "developers",
            "engineer",
            "engineers",
            "user",
            "users",
        ],
    )
    .unwrap_or_else(|| if ql.contains("team") { 10 } else { 5 });

    let env_type = if ql.contains("prod") || ql.contains("production") {
        "production"
    } else if ql.contains("staging") || ql.contains("stage") {
        "staging"
    } else if ql.contains("dev") {
        "development"
    } else {
        "general"
    };

    let size = if ql.contains("large") || ql.contains("gpu cluster") {
        "large"
    } else if ql.contains("small") || ql.contains("minimal") {
        "small"
    } else if ql.contains("medium") {
        "medium"
    } else if developers >= 30 {
        "large"
    } else if developers <= 8 {
        "small"
    } else {
        "medium"
    };

    let (vcpus, mem_gib) = match size {
        "large" => (8, 32),
        "small" => (2, 8),
        _ => (4, 16),
    };

    let vm_count = if gpu_required {
        (developers / 4).clamp(2, 16)
    } else {
        developers.clamp(1, 40)
    };

    let total_vcpus = vm_count * vcpus;
    let total_memory_gib = vm_count * mem_gib;
    let storage_gib = vm_count * if env_type == "production" { 200 } else { 80 };
    let hourly = total_vcpus as f64 * vcpu_rate + total_memory_gib as f64 * gib_rate;
    let estimated_monthly_usd = hourly * 730.0;

    let backup_policy = match env_type {
        "production" => "daily + 7d retention",
        "staging" => "weekly snapshots",
        _ => "on-demand backups",
    };

    let network = if env_type == "production" {
        "isolated prod VLAN + LB segment"
    } else {
        "shared dev/staging network"
    };

    let label = if gpu_required {
        format!("GPU cluster preview for {developers} seats")
    } else {
        format!("{env_type} environment for {developers} developers")
    };

    let review = format!(
        "{vm_count} VMs × {vcpus} vCPU × {mem_gib} GiB, {storage_gib} GiB storage, ~${estimated_monthly_usd:.0}/mo"
    );

    let mut build_steps = vec![
        format!("Provision {vm_count} VMs on {network}"),
        format!("Attach {storage_gib} GiB total storage ({backup_policy})"),
        "Import networks and storage pools if missing".into(),
    ];

    if gpu_required {
        build_steps.push(
            "GPU passthrough / KubeVirt GPU operator (preview — manual validation required)".into(),
        );
        build_steps.push("Deploy inference stack via blueprint (coming soon)".into());
    } else {
        build_steps.push("Apply backup policy via blueprint".into());
    }

    let mut spotlight_intents = Vec::new();
    for i in 0..vm_count.min(5) {
        let name = format!("{env_type}-dev-{:02}", i + 1);
        spotlight_intents.push(super::intent_router::intent(
            &format!("env-vm-{i}"),
            &format!("Create VM {name}"),
            &format!("{vcpus} vCPU, {mem_gib} GiB — part of {label}"),
            "create_vm",
            Some(name),
            None,
            Some(serde_json::json!({
                "name": format!("{env_type}-dev-{:02}", i + 1),
                "os": "ubuntu-24.04",
                "size": size,
                "network": "default",
                "cores": vcpus,
                "memory_gib": mem_gib,
            })),
        ));
    }

    EnvironmentResourcePlan {
        label,
        review,
        developer_count: developers,
        vm_count,
        vcpus_per_vm: vcpus,
        memory_gib_per_vm: mem_gib,
        total_vcpus,
        total_memory_gib,
        storage_gib,
        network: network.into(),
        backup_policy: backup_policy.into(),
        estimated_monthly_usd,
        environment_type: env_type.into(),
        gpu_required,
        preview_only: gpu_required,
        build_steps,
        spotlight_intents,
    }
}

fn extract_count(hay: &str, units: &[&str]) -> Option<i32> {
    for unit in units {
        if let Some(idx) = hay.find(unit) {
            let prefix = hay[..idx].trim();
            let num: String = prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if let Ok(n) = num.parse::<i32>() {
                if n > 0 && n <= 10_000 {
                    return Some(n);
                }
            }
        }
    }
    if let Some(n) = hay.split_whitespace().find_map(|w| w.parse::<i32>().ok()) {
        if n > 0 && n <= 10_000 {
            return Some(n);
        }
    }
    None
}

#[derive(Debug, Deserialize)]
pub struct EnvironmentExecuteBody {
    pub query: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_env_max_vms")]
    pub max_vms: i32,
}

fn default_env_max_vms() -> i32 {
    5
}

#[derive(Debug, Serialize)]
pub struct EnvironmentVmTask {
    pub name: String,
    pub host: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EnvironmentExecuteResult {
    pub dry_run: bool,
    pub plan: EnvironmentResourcePlan,
    pub vm_tasks: Vec<EnvironmentVmTask>,
    pub summary: String,
}

fn env_vm_spec(name: &str, vcpus: i32, memory_gib: i32) -> serde_json::Value {
    serde_json::json!({
        "api_version": "virt.zyvor.dev/v1",
        "kind": "VirtualMachine",
        "metadata": { "name": name, "project": "environment" },
        "spec": {
            "cpu": { "sockets": 1, "cores": vcpus },
            "memory": format!("{memory_gib}Gi"),
            "storage": [{ "name": "root", "size": "80Gi", "class": "silver" }],
            "network": [{ "network": "default", "ip_mode": "dhcp" }],
            "firmware": "bios",
            "graphics": { "type": "vnc", "listen": "127.0.0.1" }
        }
    })
}

pub async fn execute_environment(
    state: &AppState,
    actor: &AuthUser,
    body: &EnvironmentExecuteBody,
) -> Result<EnvironmentExecuteResult, ApiError> {
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let plan = plan_environment(&body.query, rates.0, rates.1);
    if plan.gpu_required {
        return Err(ApiError::bad_request(
            "GPU environments require mission stack — use /api/v1/ai/mission/stack/execute",
        ));
    }

    let create_count = plan.vm_count.min(body.max_vms.max(1));
    let mut vm_tasks = Vec::new();

    for i in 0..create_count {
        let name = format!("{}-dev-{:02}", plan.environment_type, i + 1);
        let host_id = crate::engine::placement::pick_host_for_vm(
            &state.pool,
            &["environment".into(), plan.environment_type.clone()],
            plan.memory_gib_per_vm as i64 * 1024,
        )
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

        let hostname: String = crate::db::query_scalar("SELECT hostname FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if body.dry_run {
            vm_tasks.push(EnvironmentVmTask {
                name,
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
        let mem_mib = plan.memory_gib_per_vm as i64 * 1024;
        let tags: Vec<String> = vec!["environment".into(), plan.environment_type.clone()];
        let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
        let spec_json = env_vm_spec(&name, plan.vcpus_per_vm, plan.memory_gib_per_vm);

        let mut tx = state
            .pool
            .begin()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        crate::db::query(
            "INSERT INTO vms (id, cluster_id, host_id, name, project, spec_json, desired_state, lifecycle_phase, vcpus, memory_mib, tags)
             VALUES (?, ?, ?, ?, 'environment', ?, 'running', 'creating', ?, ?, ?)",
        )
        .bind(vm_id)
        .bind(cluster_id)
        .bind(host_id)
        .bind(&name)
        .bind(&spec_json)
        .bind(plan.vcpus_per_vm)
        .bind(mem_mib)
        .bind(&tags_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        crate::db::query(
            "INSERT INTO vm_disks (id, vm_id, name, size_gib, storage_class) VALUES (?, ?, 'root', 80, 'silver')",
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

        vm_tasks.push(EnvironmentVmTask {
            name,
            host: hostname,
            task_id: Some(task_id.to_string()),
        });
    }

    let summary = if body.dry_run {
        format!("Preview: would create {create_count} VM(s) for environment plan.")
    } else {
        format!("Enqueued {create_count} environment VM task(s).")
    };

    state.emit_event("ai.environment", summary.clone());

    Ok(EnvironmentExecuteResult {
        dry_run: body.dry_run,
        plan,
        vm_tasks,
        summary,
    })
}
