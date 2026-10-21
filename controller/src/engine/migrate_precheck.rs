// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Serialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct MigrateCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigratePrecheckResult {
    pub ok: bool,
    pub checks: Vec<MigrateCheck>,
}

pub async fn run_migrate_precheck(
    pool: &DbPool,
    vm_id: Uuid,
    dest_host_id: Uuid,
    live: bool,
) -> anyhow::Result<MigratePrecheckResult> {
    if crate::engine::fluxvm_fleet::is_fluxvm(pool, vm_id).await {
        return crate::engine::fluxvm_fleet::precheck(pool, vm_id, dest_host_id, live).await;
    }
    let mut checks = Vec::new();

    let vm_row: Option<(String, Option<Uuid>, i64, i32, String)> = crate::db::query_as(
        "SELECT name, host_id, memory_mib, vcpus, desired_state FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;

    let Some((vm_name, source_host_id, memory_mib, vcpus, desired_state)) = vm_row else {
        checks.push(fail(
            "vm_exists",
            "VM not found in inventory",
            "Create or sync the VM in platform inventory before migrating",
        ));
        return Ok(MigratePrecheckResult { ok: false, checks });
    };
    checks.push(pass("vm_exists", &format!("VM '{vm_name}' found")));

    // A vGPU partition lives on one host's GPU: the VM cannot leave it.
    if crate::api::vgpu::is_pinned(pool, vm_id).await {
        checks.push(fail(
            "vgpu_pinned",
            "The VM holds a vGPU partition on its current host",
            "Detach the partition (DELETE /api/v1/vms/{id}/mdev/{uuid}) before migrating, then attach one on the destination",
        ));
    } else {
        checks.push(pass("vgpu_pinned", "No vGPU partition pins the VM to its host"));
    }

    let source_host_id = match source_host_id {
        Some(id) => id,
        None => {
            checks.push(fail(
                "source_host",
                "VM has no assigned host",
                "Assign a host to the VM or re-apply the VM spec",
            ));
            return Ok(MigratePrecheckResult { ok: false, checks });
        }
    };

    if source_host_id == dest_host_id {
        checks.push(fail(
            "different_host",
            "Source and destination must differ",
            "Pick a different destination host for migration",
        ));
    } else {
        checks.push(pass("different_host", "Destination is a different host"));
    }

    let dest: Option<(String, String, bool, i64, i64, f32)> = crate::db::query_as(
        "SELECT hostname, state, maintenance_mode, memory_total_mib, memory_used_mib, cpu_percent
         FROM hosts WHERE id = ?",
    )
    .bind(dest_host_id)
    .fetch_optional(pool)
    .await?;

    let Some((dest_name, dest_state, maint, mem_total, mem_used, cpu)) = dest else {
        checks.push(fail(
            "dest_host",
            "Destination host not found",
            "Verify the destination host ID in the cluster inventory",
        ));
        return Ok(MigratePrecheckResult { ok: false, checks });
    };

    if dest_state != "online" {
        checks.push(fail(
            "dest_online",
            &format!("Host '{dest_name}' is {dest_state}"),
            "Wait for host validation to pass or clear maintenance before migrating",
        ));
    } else {
        checks.push(pass(
            "dest_online",
            &format!("Host '{dest_name}' is online"),
        ));
    }

    if maint {
        checks.push(fail(
            "dest_maintenance",
            "Destination host is in maintenance mode",
            "Exit maintenance mode on the destination host",
        ));
    } else {
        checks.push(pass("dest_maintenance", "Destination not in maintenance"));
    }

    let headroom = mem_total.saturating_sub(mem_used);
    if headroom < memory_mib {
        checks.push(fail(
            "dest_memory",
            &format!("Need {memory_mib} MiB free, only {headroom} MiB available"),
            "Free memory on the destination host or pick a less loaded host",
        ));
    } else {
        checks.push(pass(
            "dest_memory",
            &format!("{headroom} MiB memory headroom for {memory_mib} MiB VM"),
        ));
    }

    if cpu > 90.0 {
        checks.push(fail(
            "dest_cpu",
            &format!("Destination CPU at {cpu:.0}%"),
            "Wait for load to drop or choose another host via placement recommendations",
        ));
    } else {
        checks.push(pass("dest_cpu", &format!("Destination CPU at {cpu:.0}%")));
    }

    let dest_uri: Option<String> =
        crate::db::query_scalar("SELECT COALESCE(NULLIF(libvirt_uri, ''), '') FROM hosts WHERE id = ?")
            .bind(dest_host_id)
            .fetch_optional(pool)
            .await?;

    if dest_uri.as_deref().unwrap_or("").is_empty() {
        checks.push(fail(
            "dest_libvirt",
            "Destination libvirt URI not configured",
            "Set libvirt_uri to qemu:///system on the destination host record",
        ));
    } else {
        checks.push(pass("dest_libvirt", "Destination libvirt URI configured"));
    }

    if live && desired_state != "running" {
        checks.push(fail(
            "live_state",
            "Live migration requires VM desired state running",
            "Start the VM before live migration or use offline migration",
        ));
    } else if live {
        checks.push(pass("live_state", "VM is eligible for live migration"));
    } else {
        checks.push(pass("live_state", "Offline migration path"));
    }

    let _ = vcpus;

    match host_addrs_for_precheck(pool, source_host_id, dest_host_id).await {
        Ok((source_addr, dest_cpu, dest_lv)) => {
            // Fail closed here too: `host_addrs_for_precheck` above already proved this
            // source host row exists, so a failure on this second read is a real (if
            // transient) DB error, not "no data". Silently defaulting to an empty
            // string previously made `cpu_compatible` auto-pass (it treats an empty
            // source model as always compatible) — i.e. the exact fail-open bug this
            // function was fixed to close, just one query deeper.
            match crate::db::query_scalar::<_, String>(
                "SELECT COALESCE(cpu_model, '') FROM hosts WHERE id = ?",
            )
            .bind(source_host_id)
            .fetch_one(pool)
            .await
            {
                Ok(source_cpu) => {
                    let matrix: serde_json::Value = crate::db::query_scalar(
                        "SELECT cpu_compat_matrix FROM clusters ORDER BY created_at LIMIT 1",
                    )
                    .fetch_one(pool)
                    .await
                    .unwrap_or(serde_json::json!([]));
                    let rules: Vec<crate::api::cpu_compat::CpuCompatRule> =
                        serde_json::from_value(matrix).unwrap_or_default();
                    if crate::api::cpu_compat::cpu_compatible(&rules, &source_cpu, &dest_cpu) {
                        checks.push(pass(
                            "cpu_compat",
                            &format!("CPU {source_cpu} compatible with {dest_cpu}"),
                        ));
                    } else {
                        checks.push(fail(
                            "cpu_compat",
                            &format!("CPU {source_cpu} not compatible with {dest_cpu}"),
                            "Use offline migration with CPU baseline or update the CPU compatibility matrix",
                        ));
                    }
                }
                Err(e) => {
                    checks.push(fail(
                        "cpu_compat",
                        &format!(
                            "Could not read source host CPU model to verify compatibility: {e}"
                        ),
                        "Verify the source host record in cluster inventory and retry",
                    ));
                }
            }

            // Fail closed: if we can't reach the source host's agent, or it can't run
            // its own pre-check, we have NOT verified migration safety on the source
            // side (disk space, block-migration support, active snapshots, etc.) — a
            // missing check here must not be silently treated as a passing one.
            match crate::agent_client::connect(&source_addr).await {
                Ok(mut client) => {
                    match crate::agent_client::precheck_migrate(
                        &mut client,
                        &vm_name,
                        &dest_cpu,
                        &dest_lv,
                    )
                    .await
                    {
                        Ok(agent_pre) => {
                            for c in agent_pre.checks {
                                if c.passed {
                                    checks.push(pass(&c.name, &c.message));
                                } else {
                                    checks.push(fail(
                                        &c.name,
                                        &c.message,
                                        "See agent migration pre-check details on the source host",
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            checks.push(fail(
                                "source_agent_precheck",
                                &format!(
                                    "Source host agent could not run its migration pre-check: {e}"
                                ),
                                "Check machina-agent logs on the source host and retry",
                            ));
                        }
                    }
                }
                Err(e) => {
                    checks.push(fail(
                        "source_agent_reachable",
                        &format!("Cannot reach source host agent to verify migration safety: {e}"),
                        "Verify machina-agent is running and reachable on the source host and retry",
                    ));
                }
            }
        }
        Err(e) => {
            checks.push(fail(
                "source_host_lookup",
                &format!("Could not look up source/destination host details: {e}"),
                "Verify source and destination host records in cluster inventory",
            ));
        }
    }

    let ok = checks.iter().all(|c| c.passed);
    Ok(MigratePrecheckResult { ok, checks })
}

async fn host_addrs_for_precheck(
    pool: &DbPool,
    source_host_id: Uuid,
    dest_host_id: Uuid,
) -> anyhow::Result<(String, String, String)> {
    let source: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(source_host_id)
        .fetch_one(pool)
        .await?;
    let dest: (String, String) = crate::db::query_as(
        "SELECT COALESCE(cpu_model, ''), COALESCE(libvirt_version, '') FROM hosts WHERE id = ?",
    )
    .bind(dest_host_id)
    .fetch_one(pool)
    .await?;
    Ok((source, dest.0, dest.1))
}

fn pass(name: &str, message: &str) -> MigrateCheck {
    MigrateCheck {
        name: name.into(),
        passed: true,
        message: message.into(),
        remediation: None,
    }
}

fn fail(name: &str, message: &str, remediation: &str) -> MigrateCheck {
    MigrateCheck {
        name: name.into(),
        passed: false,
        message: message.into(),
        remediation: Some(remediation.into()),
    }
}
