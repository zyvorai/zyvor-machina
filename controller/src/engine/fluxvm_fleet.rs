// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! FluxVM VMs across the fleet: inventory rows (`inventory_source = 'fluxvm'`)
//! from each host agent, lifecycle through the agent, live migration between
//! two hosts' fluxvm-api (or one host to itself), and HA re-create on another
//! host from the last record seen. Only QEMU VMs on a shared disk (or Ceph RBD
//! in place) can migrate or be re-created; FluxVM refuses the rest.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use machina_agent::pb::VmSummary;
use serde_json::Value;
use uuid::Uuid;

use crate::agent_client;
use crate::db::DbPool;
use crate::engine::migrate_precheck::{MigrateCheck, MigratePrecheckResult};
use crate::engine::vm_lifecycle;
use crate::state::AppState;
use crate::tasks::worker::{host_agent_addr, update_task_progress};

pub const SOURCE: &str = "fluxvm";

const MIGRATION_TIMEOUT: Duration = Duration::from_secs(900);
const HOTPLUGGED_LABEL: &str = "fluxvm.dev/hotplugged";

pub async fn is_fluxvm(pool: &DbPool, vm_id: Uuid) -> bool {
    crate::db::query_scalar::<_, String>(
        "SELECT COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some_and(|s| s == SOURCE)
}

/// Upsert this host's FluxVM VMs. Prunes only when the agent reached fluxvm-api
/// (`reachable`), and never touches a row mid-migration.
pub async fn sync_host(
    state: &AppState,
    host_id: Uuid,
    cluster_id: Uuid,
    vms: &[VmSummary],
    reachable: bool,
) -> anyhow::Result<()> {
    let mut seen: HashSet<String> = HashSet::new();
    for vm in vms {
        seen.insert(vm.uuid.clone());
        let existing: Option<Uuid> = match crate::db::query_scalar(
            "SELECT id FROM vms WHERE cluster_id = ? AND inventory_source = 'fluxvm' AND uuid = ?",
        )
        .bind(cluster_id)
        .bind(&vm.uuid)
        .fetch_optional(&state.pool)
        .await?
        {
            Some(id) => Some(id),
            None => {
                crate::db::query_scalar(
                    "SELECT id FROM vms WHERE cluster_id = ? AND inventory_source = 'fluxvm' AND name = ?
                     AND lifecycle_phase != 'migrating'",
                )
                .bind(cluster_id)
                .bind(&vm.name)
                .fetch_optional(&state.pool)
                .await?
            }
        };
        let ips = serde_json::to_string(&vm.guest_ips).unwrap_or_else(|_| "[]".into());
        if let Some(id) = existing {
            crate::db::query(
                "UPDATE vms SET host_id = ?, name = ?, uuid = ?, observed_state = ?, vcpus = ?, memory_mib = ?,
                 guest_ip = CASE WHEN ? != '' THEN ? ELSE guest_ip END,
                 guest_ips = CASE WHEN ? != '[]' THEN ? ELSE guest_ips END,
                 fluxvm_engine = ?, fluxvm_storage = ?, fluxvm_record_json = ?,
                 desired_state = CASE WHEN desired_state = 'unknown' THEN ? ELSE desired_state END,
                 last_seen_at = datetime('now'), updated_at = datetime('now')
                 WHERE id = ? AND lifecycle_phase != 'migrating'",
            )
            .bind(host_id)
            .bind(&vm.name)
            .bind(&vm.uuid)
            .bind(&vm.state)
            .bind(vm.vcpus as i32)
            .bind(vm.memory_mb as i64)
            .bind(&vm.guest_ip)
            .bind(&vm.guest_ip)
            .bind(&ips)
            .bind(&ips)
            .bind(&vm.fluxvm_engine)
            .bind(&vm.fluxvm_storage)
            .bind(&vm.fluxvm_record_json)
            .bind(desired_from_observed(&vm.state))
            .bind(id)
            .execute(&state.pool)
            .await?;
        } else {
            crate::db::query(
                "INSERT INTO vms (id, cluster_id, host_id, name, spec_json, desired_state, observed_state,
                 uuid, vcpus, memory_mib, managed, lifecycle_phase, inventory_source,
                 fluxvm_engine, fluxvm_storage, fluxvm_record_json, last_seen_at)
                 VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?, ?, FALSE, 'idle', 'fluxvm', ?, ?, ?, datetime('now'))",
            )
            .bind(Uuid::new_v4())
            .bind(cluster_id)
            .bind(host_id)
            .bind(&vm.name)
            .bind(desired_from_observed(&vm.state))
            .bind(&vm.state)
            .bind(&vm.uuid)
            .bind(vm.vcpus as i32)
            .bind(vm.memory_mb as i64)
            .bind(&vm.fluxvm_engine)
            .bind(&vm.fluxvm_storage)
            .bind(&vm.fluxvm_record_json)
            .execute(&state.pool)
            .await?;
            state.emit_event(
                "vm.discovered",
                format!("Discovered FluxVM VM '{}' on host", vm.name),
            );
        }
    }

    if !reachable {
        return Ok(());
    }
    let rows: Vec<(Uuid, String, bool)> = crate::db::query_as(
        "SELECT id, COALESCE(uuid, ''), managed FROM vms
         WHERE host_id = ? AND inventory_source = 'fluxvm' AND lifecycle_phase != 'migrating'",
    )
    .bind(host_id)
    .fetch_all(&state.pool)
    .await?;
    for (id, uuid, managed) in rows {
        if seen.contains(&uuid) {
            continue;
        }
        if managed {
            crate::db::query(
                "UPDATE vms SET observed_state = 'missing', updated_at = datetime('now') WHERE id = ?",
            )
            .bind(id)
            .execute(&state.pool)
            .await?;
        } else {
            crate::db::query("DELETE FROM vms WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await?;
        }
    }
    Ok(())
}

/// Discovered VMs start out wanting whatever they are doing now, so HA keeps a
/// running FluxVM VM running.
fn desired_from_observed(observed: &str) -> &'static str {
    match observed {
        "running" | "paused" => "running",
        "shutoff" => "stopped",
        _ => "unknown",
    }
}

async fn agent(pool: &DbPool, host_id: Uuid) -> anyhow::Result<agent_client::AgentClient> {
    let addr = host_agent_addr(pool, host_id).await?;
    agent_client::connect(&addr).await
}

/// `host` of an agent address like `http://10.0.0.5:50051` — the address a
/// remote source dials to reach that host's migration listener.
pub fn agent_host(addr: &str) -> String {
    let a = agent_client::normalize_agent_addr(addr);
    let a = a.split('/').next().unwrap_or_default();
    if let Some(rest) = a.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default().to_string();
    }
    a.rsplit_once(':').map(|(h, _)| h).unwrap_or(a).to_string()
}

pub async fn power(
    pool: &DbPool,
    host_id: Uuid,
    name: &str,
    action: &str,
) -> anyhow::Result<machina_agent::pb::VmPowerResponse> {
    let mut c = agent(pool, host_id).await?;
    agent_client::fluxvm_power(&mut c, name, action).await
}

pub async fn delete(pool: &DbPool, host_id: Uuid, name: &str) -> anyhow::Result<()> {
    let mut c = agent(pool, host_id).await?;
    agent_client::fluxvm_delete(&mut c, name).await
}

/// Why a FluxVM record can't be re-created on another host; `None` = ok.
fn placement_blocker(record: &Value) -> Option<String> {
    let engine = record["backend"].as_str().unwrap_or_default();
    if engine != "qemu" {
        return Some(format!(
            "FluxVM engine '{engine}' cannot migrate (QEMU only)"
        ));
    }
    let storage = record["request"]["storage"].as_str().unwrap_or("default");
    if storage != "shared" && storage != "ceph-rbd-in-place" {
        return Some(format!(
            "FluxVM storage '{storage}' is local to the host; create the VM with a shared disk"
        ));
    }
    None
}

/// Why a FluxVM record can't live-migrate; `None` = ok.
pub fn mobility_blocker(record: &Value) -> Option<String> {
    if let Some(why) = placement_blocker(record) {
        return Some(why);
    }
    if record["labels"].get(HOTPLUGGED_LABEL).is_some() {
        return Some(
            "VM has hot-plugged CPUs, memory or NICs since it started; restart it before migrating"
                .into(),
        );
    }
    let loaded = record["request"]["cdroms"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| !c["path"].as_str().unwrap_or_default().is_empty());
    if let Some(cd) = loaded {
        return Some(format!(
            "CD-ROM '{}' still holds install media; eject it before migrating",
            cd["name"].as_str().unwrap_or_default()
        ));
    }
    None
}

fn check(name: &str, passed: bool, message: String, remediation: &str) -> MigrateCheck {
    MigrateCheck {
        name: name.into(),
        passed,
        message,
        remediation: (!passed).then(|| remediation.to_string()),
    }
}

/// The VM's state as the source agent sees it now; the inventory row can lag
/// a migration or restart by a sync interval.
async fn live_state(pool: &DbPool, host_id: Uuid, name: &str) -> Option<String> {
    let mut c = agent(pool, host_id).await.ok()?;
    let list = agent_client::list_vms(&mut c).await.ok()?;
    list.vms
        .into_iter()
        .find(|v| v.backend == SOURCE && v.name == name)
        .map(|v| v.state)
}

/// FluxVM migration pre-check. Same source and destination is allowed: the VM
/// moves to a fresh QEMU process on that host.
pub async fn precheck(
    pool: &DbPool,
    vm_id: Uuid,
    dest_host_id: Uuid,
    live: bool,
) -> anyhow::Result<MigratePrecheckResult> {
    let mut checks = Vec::new();
    let row: Option<(String, Option<Uuid>, i64, String)> = crate::db::query_as(
        "SELECT name, host_id, memory_mib, observed_state FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;
    let Some((name, Some(src), memory_mib, observed)) = row else {
        checks.push(check(
            "vm_exists",
            false,
            "FluxVM VM not found or has no host".into(),
            "Wait for the host inventory to pick the VM up",
        ));
        return Ok(MigratePrecheckResult { ok: false, checks });
    };
    checks.push(check("vm_exists", true, format!("FluxVM VM '{name}'"), ""));
    let observed = live_state(pool, src, &name).await.unwrap_or(observed);
    checks.push(check(
        "live_state",
        live && observed == "running",
        if live {
            format!("VM is {observed}")
        } else {
            "FluxVM supports live migration only".into()
        },
        "Start the VM; FluxVM moves running VMs only",
    ));

    let dest: Option<(String, String, bool, i64, i64)> = crate::db::query_as(
        "SELECT hostname, state, maintenance_mode, memory_total_mib, memory_used_mib FROM hosts WHERE id = ?",
    )
    .bind(dest_host_id)
    .fetch_optional(pool)
    .await?;
    let Some((dest_name, dest_state, maint, total, used)) = dest else {
        checks.push(check(
            "dest_host",
            false,
            "Destination host not found".into(),
            "Pick a host from the cluster inventory",
        ));
        return Ok(MigratePrecheckResult { ok: false, checks });
    };
    checks.push(check(
        "dest_online",
        dest_state == "online" && !maint,
        format!(
            "Host '{dest_name}' is {dest_state}{}",
            if maint { " (maintenance)" } else { "" }
        ),
        "Bring the destination online and out of maintenance",
    ));
    if src == dest_host_id {
        checks.push(check(
            "same_host",
            true,
            "Same host: the VM moves to a fresh QEMU process".into(),
            "",
        ));
    } else {
        let headroom = total.saturating_sub(used);
        checks.push(check(
            "dest_memory",
            headroom >= memory_mib,
            format!("{headroom} MiB free for a {memory_mib} MiB VM"),
            "Free memory on the destination or pick another host",
        ));
    }

    match agent(pool, src).await {
        Ok(mut c) => match agent_client::fluxvm_export_record(&mut c, &name).await {
            Ok(r) => {
                let rec: Value = serde_json::from_str(&r.record_json).unwrap_or_default();
                let blocker = mobility_blocker(&rec);
                checks.push(check(
                    "fluxvm_mobility",
                    blocker.is_none(),
                    blocker.unwrap_or_else(|| "QEMU on shared storage".into()),
                    "Recreate the VM with fluxvm_shared_disk on QEMU, restart it after hotplug, or eject its install media",
                ));
            }
            Err(e) => checks.push(check(
                "fluxvm_source",
                false,
                format!("Source host could not export the FluxVM record: {e}"),
                "Check fluxvm-api and MACHINA_FLUXVM_URL on the source host's agent",
            )),
        },
        Err(e) => checks.push(check(
            "source_agent_reachable",
            false,
            format!("Cannot reach the source agent: {e}"),
            "Check machina-agent on the source host",
        )),
    }
    if src != dest_host_id {
        if let Err(e) = agent(pool, dest_host_id).await {
            checks.push(check(
                "dest_agent_reachable",
                false,
                format!("Cannot reach the destination agent: {e}"),
                "Check machina-agent on the destination host",
            ));
        }
    }
    let ok = checks.iter().all(|c| c.passed);
    Ok(MigratePrecheckResult { ok, checks })
}

pub struct MigrateOpts {
    pub bandwidth_mbps: u64,
    pub max_downtime_ms: u64,
}

/// Two-agent live migration: receiver on the target, stream from the source,
/// finish the source, adopt on the target, then point the row at the new id.
pub async fn migrate(
    state: &AppState,
    task_id: Uuid,
    vm_id: Uuid,
    dest_host_id: Uuid,
    opts: MigrateOpts,
) -> anyhow::Result<()> {
    let (name, src_host): (String, Option<Uuid>) =
        crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {vm_id} not found"))?;
    let src_host = src_host.ok_or_else(|| anyhow::anyhow!("vm {vm_id} has no host"))?;
    vm_lifecycle::set_vm_phase(&state.pool, vm_id, vm_lifecycle::PHASE_MIGRATING).await?;

    let mut src = agent(&state.pool, src_host).await?;
    let mut dst = agent(&state.pool, dest_host_id).await?;
    let exported = agent_client::fluxvm_export_record(&mut src, &name).await?;
    let record: Value = serde_json::from_str(&exported.record_json)?;
    if let Some(why) = mobility_blocker(&record) {
        anyhow::bail!("{why}");
    }
    let src_id = exported.id;

    let (listen, advertise) = if src_host == dest_host_id {
        ("127.0.0.1".to_string(), String::new())
    } else {
        let addr = host_agent_addr(&state.pool, dest_host_id).await?;
        ("0.0.0.0".to_string(), agent_host(&addr))
    };
    update_task_progress(&state.pool, task_id, 10, "Preparing FluxVM receiver").await?;
    let recv =
        agent_client::fluxvm_prepare_receiver(&mut dst, &exported.record_json, &listen, &advertise)
            .await?;

    let abort = |e: anyhow::Error| {
        let (src_id, rid) = (src_id.clone(), recv.receiver_id.clone());
        let pool = state.pool.clone();
        async move {
            if let Ok(mut s) = agent(&pool, src_host).await {
                let _ = agent_client::fluxvm_abort(&mut s, &src_id, "").await;
            }
            if let Ok(mut d) = agent(&pool, dest_host_id).await {
                let _ = agent_client::fluxvm_abort(&mut d, "", &rid).await;
            }
            Err::<(), _>(e)
        }
    };

    update_task_progress(&state.pool, task_id, 20, "Streaming memory").await?;
    if let Err(e) = agent_client::fluxvm_start_migration(
        &mut src,
        &src_id,
        &recv.uri,
        opts.bandwidth_mbps,
        opts.max_downtime_ms,
    )
    .await
    {
        return abort(e).await;
    }
    let started = Instant::now();
    loop {
        match agent_client::fluxvm_migration_status(&mut src, &src_id).await {
            Ok(st) if st.progress == "completed" => break,
            Ok(st) if st.progress == "failed" => {
                return abort(anyhow::anyhow!("FluxVM migration failed: {}", st.error)).await
            }
            Ok(_) => {}
            Err(e) => return abort(e).await,
        }
        if started.elapsed() > MIGRATION_TIMEOUT {
            return abort(anyhow::anyhow!(
                "FluxVM migration did not finish within {}s",
                MIGRATION_TIMEOUT.as_secs()
            ))
            .await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    update_task_progress(&state.pool, task_id, 80, "Switching over").await?;
    agent_client::fluxvm_finish_migration(&mut src, &src_id).await?;
    let adopted = agent_client::fluxvm_adopt(&mut dst, &recv.receiver_id, &recv.token).await?;

    crate::db::query(
        "UPDATE vms SET host_id = ?, uuid = ?, observed_state = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(dest_host_id)
    .bind(&adopted.id)
    .bind(&adopted.state)
    .bind(vm_id)
    .execute(&state.pool)
    .await?;
    vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await?;
    state.emit_event(
        "vm.migrate",
        format!("FluxVM VM {name} migrated (id {})", adopted.id),
    );
    update_task_progress(&state.pool, task_id, 100, "migrated").await?;
    Ok(())
}

/// The fluxvm-api create body HA re-creates from: the VM's own request.
pub fn recreate_body(record: &Value) -> anyhow::Result<Value> {
    if let Some(why) = placement_blocker(record) {
        anyhow::bail!("not recoverable on another host: {why}");
    }
    let mut body = record
        .get("request")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(|| anyhow::anyhow!("FluxVM record has no request"))?;
    if let Some(name) = record.get("name") {
        body["name"] = name.clone();
    }
    // FluxVM refuses an empty CD-ROM on create; an ejected drive only exists after eject.
    if let Some(cds) = body.get_mut("cdroms").and_then(Value::as_array_mut) {
        cds.retain(|c| !c["path"].as_str().unwrap_or_default().is_empty());
    }
    Ok(body)
}

/// HA re-create on `host_id` from the record last seen in inventory. Breaks
/// the shared-disk lock (the failed host is fenced or gone). Any instance of
/// the same name on the target is stale (the failed one on the same host, or
/// an earlier home of the VM) and is removed first so the name is free.
pub async fn ha_recreate(
    state: &AppState,
    task_id: Uuid,
    vm_id: Uuid,
    host_id: Uuid,
    previous_host: Option<Uuid>,
    desired: &str,
) -> anyhow::Result<()> {
    let (name, record_json): (String, Option<String>) =
        crate::db::query_as("SELECT name, fluxvm_record_json FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {vm_id} not found"))?;
    let record: Value = serde_json::from_str(record_json.as_deref().unwrap_or("null"))
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| anyhow::anyhow!("no FluxVM record stored for {name}; cannot re-create"))?;
    let body = recreate_body(&record)?;

    // Otherwise the new instance sits next to the stale one and power-by-name
    // starts the stale one, which the new instance's disk lock refuses.
    update_task_progress(&state.pool, task_id, 20, "Removing any stale instance").await?;
    if let Err(e) = delete(&state.pool, host_id, &name).await {
        if previous_host == Some(host_id) {
            tracing::warn!(vm = %name, "fluxvm HA: removing the old instance failed: {e:#}");
        }
    }
    update_task_progress(&state.pool, task_id, 40, "Re-creating on shared disk").await?;
    let mut c = agent(&state.pool, host_id).await?;
    let resp = agent_client::fluxvm_apply(&mut c, &body, true).await?;
    let observed = if desired == "running" {
        match agent_client::fluxvm_power(&mut c, &name, "start").await {
            Ok(r) => r.state,
            Err(e) if e.to_string().contains("running") => "running".into(),
            Err(e) => return Err(e),
        }
    } else {
        "shutoff".into()
    };
    crate::db::query(
        "UPDATE vms SET host_id = ?, uuid = ?, observed_state = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(host_id)
    .bind(&resp.uuid)
    .bind(&observed)
    .bind(vm_id)
    .execute(&state.pool)
    .await?;
    vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await?;
    state.emit_event(
        "ha.recover",
        format!("FluxVM VM {name} re-created (id {})", resp.uuid),
    );
    update_task_progress(&state.pool, task_id, 100, "recovered").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(uuid: &str, name: &str, state: &str) -> VmSummary {
        VmSummary {
            uuid: uuid.into(),
            name: name.into(),
            state: state.into(),
            vcpus: 2,
            memory_mb: 1024,
            backend: SOURCE.into(),
            fluxvm_engine: "qemu".into(),
            fluxvm_storage: "shared".into(),
            fluxvm_record_json: r#"{"name":"x"}"#.into(),
            ..Default::default()
        }
    }

    async fn rows(pool: &DbPool) -> Vec<(String, String, String, String)> {
        crate::db::query_as(
            "SELECT name, COALESCE(uuid, ''), desired_state, observed_state FROM vms
             WHERE inventory_source = 'fluxvm' ORDER BY name",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn sync_inserts_updates_and_prunes_only_when_reachable() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        let host = crate::engine::test_support::seed_host(&state.pool, Uuid::from_u128(1)).await;
        let cluster = Uuid::from_u128(2);
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c1')")
            .bind(cluster)
            .execute(&state.pool)
            .await
            .unwrap();

        let a = summary("u-a", "a", "running");
        let b = summary("u-b", "b", "shutoff");
        sync_host(&state, host, cluster, &[a.clone(), b], true)
            .await
            .unwrap();
        let got = rows(&state.pool).await;
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].2, "running");
        assert_eq!(got[1].2, "stopped");

        // Seen mid-migration first: desired stays unknown until the VM settles.
        let c = summary("u-c", "c", "migrating");
        sync_host(&state, host, cluster, &[a.clone(), c], false)
            .await
            .unwrap();
        assert_eq!(rows(&state.pool).await[2].2, "unknown");
        let c = summary("u-c", "c", "running");
        sync_host(&state, host, cluster, &[a.clone(), c], false)
            .await
            .unwrap();
        assert_eq!(rows(&state.pool).await[2].2, "running");

        // A fresh instance id under the same name (HA re-create) updates the row.
        let a2 = summary("u-a2", "a", "running");
        sync_host(&state, host, cluster, std::slice::from_ref(&a2), false)
            .await
            .unwrap();
        let got = rows(&state.pool).await;
        assert_eq!(got.len(), 3, "unreachable list must not prune");
        assert_eq!(got[0].1, "u-a2");

        sync_host(&state, host, cluster, &[a2], true).await.unwrap();
        let got = rows(&state.pool).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "a");
        assert!(
            is_fluxvm(&state.pool, {
                crate::db::query_scalar::<_, Uuid>("SELECT id FROM vms WHERE name = 'a'")
                    .fetch_one(&state.pool)
                    .await
                    .unwrap()
            })
            .await
        );
    }

    #[test]
    fn agent_host_strips_scheme_and_port() {
        assert_eq!(agent_host("http://10.0.0.5:50051"), "10.0.0.5");
        assert_eq!(agent_host("10.0.0.5:50051"), "10.0.0.5");
        assert_eq!(agent_host("https://[fd00::5]:50051"), "fd00::5");
        assert_eq!(agent_host("host-a"), "host-a");
    }

    #[test]
    fn only_shared_qemu_vms_are_mobile() {
        let ok = json!({"backend": "qemu", "request": {"storage": "shared"}, "labels": {}});
        assert!(mobility_blocker(&ok).is_none());
        let local = json!({"backend": "qemu", "request": {"storage": "default"}});
        assert!(mobility_blocker(&local).unwrap().contains("local"));
        let fc = json!({"backend": "firecracker", "request": {"storage": "shared"}});
        assert!(mobility_blocker(&fc).unwrap().contains("QEMU only"));
        let hot = json!({"backend": "qemu", "request": {"storage": "shared"},
                         "labels": {"fluxvm.dev/hotplugged": "true"}});
        assert!(mobility_blocker(&hot).unwrap().contains("restart"));
        let iso = json!({"backend": "qemu", "labels": {},
                         "request": {"storage": "shared", "cdroms": [{"name": "install", "path": "/iso/w.iso"}]}});
        assert!(mobility_blocker(&iso).unwrap().contains("eject"));
        let ejected = json!({"backend": "qemu", "labels": {},
                             "request": {"storage": "shared", "cdroms": [{"name": "install", "path": ""}]}});
        assert!(mobility_blocker(&ejected).is_none());
    }

    #[test]
    fn recreate_body_is_the_request_and_tolerates_hotplug() {
        let rec = json!({"name": "web-1", "backend": "qemu",
                         "labels": {"fluxvm.dev/hotplugged": "true"},
                         "request": {"name": "web-1", "backend": "qemu", "image": "/srv/w.raw",
                                     "storage": "shared", "vcpus": 2, "memory_mib": 1024}});
        let body = recreate_body(&rec).unwrap();
        assert_eq!(body["image"], "/srv/w.raw");
        assert_eq!(body["storage"], "shared");
        let local = json!({"name": "x", "backend": "qemu", "request": {"storage": "default"}});
        assert!(recreate_body(&local).is_err());
        let cds = json!({"name": "w", "backend": "qemu", "labels": {},
                         "request": {"storage": "shared", "cdroms": [
                             {"name": "install", "path": ""}, {"name": "cd2", "path": "/iso/d.iso"}]}});
        assert_eq!(
            recreate_body(&cds).unwrap()["cdroms"],
            json!([{"name": "cd2", "path": "/iso/d.iso"}])
        );
    }

    #[test]
    fn discovered_running_vms_want_to_run() {
        assert_eq!(desired_from_observed("running"), "running");
        assert_eq!(desired_from_observed("shutoff"), "stopped");
        assert_eq!(desired_from_observed("crashed"), "unknown");
    }
}
