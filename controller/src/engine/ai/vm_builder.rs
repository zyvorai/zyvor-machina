// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Natural-language single-VM sizing for the platform AI VM builder UI.

use serde::{Deserialize, Serialize};
use crate::db::DbPool;

use super::environment_intent;

#[derive(Debug, Deserialize)]
pub struct VmBuilderBody {
    pub prompt: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VmBuilderResult {
    pub summary: String,
    pub vm_name: String,
    pub vm_spec: serde_json::Value,
    pub vcpus: i32,
    pub memory_gib: i32,
    pub estimated_monthly_usd: f64,
    pub network: String,
    pub os_hint: String,
}

pub async fn build(pool: &DbPool, body: &VmBuilderBody) -> anyhow::Result<VmBuilderResult> {
    let rates: (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    let plan = environment_intent::plan_environment(body.prompt.trim(), rates.0, rates.1);
    let ql = body.prompt.to_lowercase();
    let os_hint = crate::engine::template_catalog::default_template_for_natural_language(&ql);
    let vm_name = body
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("{}-01", plan.environment_type));
    let vm_spec = environment_vm_spec(&vm_name, plan.vcpus_per_vm, plan.memory_gib_per_vm);
    let hourly = plan.vcpus_per_vm as f64 * rates.0 + plan.memory_gib_per_vm as f64 * rates.1;
    Ok(VmBuilderResult {
        summary: format!(
            "{} — {} vCPU, {} GiB RAM, ~${:.0}/mo (FinOps estimate)",
            plan.label,
            plan.vcpus_per_vm,
            plan.memory_gib_per_vm,
            hourly * 730.0
        ),
        vm_name,
        vm_spec,
        vcpus: plan.vcpus_per_vm,
        memory_gib: plan.memory_gib_per_vm,
        estimated_monthly_usd: hourly * 730.0,
        network: plan.network.clone(),
        os_hint: os_hint.into(),
    })
}

fn environment_vm_spec(name: &str, vcpus: i32, memory_gib: i32) -> serde_json::Value {
    serde_json::json!({
        "api_version": "virt.zyvor.dev/v1",
        "kind": "VirtualMachine",
        "metadata": { "name": name, "project": "default" },
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
