// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use machina_spec::VirtualMachine;
use crate::db::DbPool;
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

use crate::agent_client;
use crate::config::ControllerConfig;
use crate::engine::{time_travel, vm_lifecycle};
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;
use crate::tasks::TaskMessage;

/// Max tasks executing concurrently across the whole controller. Bounds the fan-out
/// of long agent RPCs (backup/migrate) and keeps SQLite-writer contention sane
/// (pool max is 4). Tasks for the *same* resource still run strictly one at a time.
const MAX_CONCURRENT_TASKS: usize = 8;

/// Serialization key for a task: two tasks with the same key never run
/// concurrently (preserving per-VM / per-host ordering and avoiding races), while
/// different keys run in parallel. Derived from the payload since TaskMessage has
/// no dedicated resource field. Tasks with no known resource key on their own
/// task_id → full concurrency.
fn resource_key(msg: &TaskMessage) -> String {
    for field in ["vm_id", "host_id", "cluster_id"] {
        if let Some(v) = msg.payload.get(field).and_then(|v| v.as_str()) {
            if !v.is_empty() {
                return format!("{field}:{v}");
            }
        }
    }
    format!("task:{}", msg.task_id)
}

/// Pure scheduling state: which resource keys are executing, and the FIFO backlog
/// of tasks waiting behind an active same-key task. Extracted so the ordering
/// logic is unit-testable independently of the async runtime.
#[derive(Default)]
struct Scheduler {
    active: HashSet<String>,
    queued: HashMap<String, VecDeque<TaskMessage>>,
}

impl Scheduler {
    /// A task arrived. Returns Some(msg) to dispatch now, or None if it was queued
    /// behind an already-running task for the same key.
    fn on_arrival(&mut self, msg: TaskMessage) -> Option<TaskMessage> {
        let key = resource_key(&msg);
        if self.active.contains(&key) {
            self.queued.entry(key).or_default().push_back(msg);
            None
        } else {
            self.active.insert(key);
            Some(msg)
        }
    }

    /// A task for `key` finished. Returns the next queued task for that key to
    /// dispatch (key stays active), or None if the key is now idle.
    fn on_complete(&mut self, key: &str) -> Option<TaskMessage> {
        if let Some(q) = self.queued.get_mut(key) {
            if let Some(next) = q.pop_front() {
                if q.is_empty() {
                    self.queued.remove(key);
                }
                return Some(next);
            }
            self.queued.remove(key);
        }
        self.active.remove(key);
        None
    }
}

pub fn spawn(state: AppState, mut rx: mpsc::UnboundedReceiver<TaskMessage>) {
    tokio::spawn(async move {
        // Process tasks CONCURRENTLY (up to MAX_CONCURRENT_TASKS) instead of one
        // at a time. Previously the loop awaited each spawned task to completion,
        // so a single multi-minute backup/migration blocked ALL task processing
        // cluster-wide (including the host.inventory sweep). Tasks for the same
        // resource key are still serialized in FIFO order to avoid races.
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));
        let mut sched = Scheduler::default();
        let (done_tx, mut done_rx) = mpsc::unbounded_channel::<String>();
        loop {
            tokio::select! {
                biased;
                // Drain completions first so queued same-key work dispatches promptly.
                Some(key) = done_rx.recv() => {
                    if let Some(next) = sched.on_complete(&key) {
                        run_task(&state, &sem, next, done_tx.clone());
                    }
                }
                maybe = rx.recv() => {
                    match maybe {
                        Some(msg) => {
                            if let Some(m) = sched.on_arrival(msg) {
                                run_task(&state, &sem, m, done_tx.clone());
                            }
                        }
                        None => break, // task bus closed → shutdown
                    }
                }
            }
        }
    });
}

/// Spawn one task: acquire a concurrency permit, run it with panic isolation, then
/// always signal completion of its resource key (even on panic) so the next queued
/// same-key task can proceed.
fn run_task(
    state: &AppState,
    sem: &Arc<Semaphore>,
    msg: TaskMessage,
    done_tx: mpsc::UnboundedSender<String>,
) {
    let st = state.clone();
    let sem = sem.clone();
    let key = resource_key(&msg);
    tokio::spawn(async move {
        let _permit = sem.acquire_owned().await;
        // Inner spawn captures a handler panic as a JoinError instead of unwinding,
        // guaranteeing we still send the completion signal below (else the key would
        // stay active forever and wedge all same-key tasks).
        let st_run = st.clone();
        let m = msg.clone();
        match tokio::spawn(async move { process_one(&st_run, &m).await }).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!(task_id = %msg.task_id, op = %msg.operation, "task failed: {e:#}");
                // Pass the FULL error chain ({e:#}) so on_task_failure can see a
                // connect-level cause (e.g. "connection refused") that the top-level
                // context message alone would hide.
                on_task_failure(&st, &msg, &format!("{e:#}")).await;
            }
            Err(join_err) => {
                tracing::error!(task_id = %msg.task_id, op = %msg.operation, "task handler panicked: {join_err} — worker recovered");
                on_task_failure(&st, &msg, "task handler panicked").await;
            }
        }
        let _ = done_tx.send(key);
    });
}

async fn process_one(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    if !claim_task(&state.pool, msg.task_id, &state.config.controller_id).await? {
        tracing::debug!(task_id = %msg.task_id, "task already claimed; skipping");
        return Ok(());
    }
    match msg.operation.as_str() {
        "vm.apply" => vm_apply(state, msg).await?,
        "vm.power" => vm_power(state, msg).await?,
        "vm.delete" => vm_delete(state, msg).await?,
        "vm.migrate" => vm_migrate(state, msg).await?,
        "vm.clone" => vm_clone(state, msg).await?,
        "vm.restore_point" => time_travel::task_restore_point(state, msg).await?,
        "vm.rewind" => time_travel::task_rewind(state, msg).await?,
        "vm.fork" => time_travel::task_fork(state, msg).await?,
        "vm.fork.detach" => time_travel::task_detach(state, msg).await?,
        "host.inventory" => host_inventory(state, msg).await?,
        "kubevirt.inventory" => kubevirt_inventory_task(state, msg).await?,
        "host.maintenance" => host_maintenance(state, msg).await?,
        "host.validate" => host_validate_task(state, msg).await?,
        "host.enforcement.apply" => host_enforcement_apply(state, msg).await?,
        "host.linux.package_upgrade" => host_linux_package_upgrade(state, msg).await?,
        "host.linux.reboot" => host_linux_reboot(state, msg).await?,
        "host.agent.upgrade" => host_agent_upgrade(state, msg).await?,
        "storage.pool.provision" => storage_pool_provision(state, msg).await?,
        "network.provision" => network_provision(state, msg).await?,
        "cloud.subnet.provision" => crate::engine::cloud::provision_subnet(state, msg).await?,
        "ha.recover" => ha_recover(state, msg).await?,
        "vm.snapshot" => vm_snapshot(state, msg).await?,
        "vm.snapshot.delete" => vm_snapshot_delete(state, msg).await?,
        "vm.snapshot.revert" => vm_snapshot_revert(state, msg).await?,
        "vm.snapshot.clone" => vm_snapshot_clone(state, msg).await?,
        "vm.backup" => vm_backup(state, msg).await?,
        "vm.backup.restore" => vm_backup_restore(state, msg).await?,
        "vm.backup.delete" => vm_backup_delete(state, msg).await?,
        "vm.disk.attach" => vm_disk_attach(state, msg).await?,
        "vm.disk.detach" => vm_disk_detach(state, msg).await?,
        "vm.disk.resize" => vm_disk_resize(state, msg).await?,
        "vm.nic.attach" => vm_nic_attach(state, msg).await?,
        "vm.nic.detach" => vm_nic_detach(state, msg).await?,
        "vm.autostart" => vm_autostart(state, msg).await?,
        "vm.resize" => vm_resize(state, msg).await?,
        "vm.change_type" => vm_change_type(state, msg).await?,
        "vm.guest_tools.install" => vm_guest_tools_install(state, msg).await?,
        "vm.install" => vm_install(state, msg).await?,
        "templates.prefetch_missing" => templates_prefetch_missing(state, msg).await?,
        other => anyhow::bail!("unknown operation: {other}"),
    }
    if let Some(vm_id) = vm_id_from_payload(msg) {
        let _ = vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await;
    }
    mark_task_completed(&state.pool, msg.task_id).await?;
    Ok(())
}

async fn vm_apply(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    vm_lifecycle::set_vm_phase_clear_error(&state.pool, vm_id, vm_lifecycle::PHASE_CREATING)
        .await?;
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    crate::api::cloud::check_vm_host(&state.pool, vm_id, host_id)
        .await
        .map_err(|e| anyhow::anyhow!("cloud host placement: {e:?}"))?;

    let row: (String, serde_json::Value) =
        crate::db::query_as("SELECT name, spec_json FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let vm: VirtualMachine = serde_json::from_value(row.1)?;
    let disk_path = disk_path_for(&state.config, &row.0);

    let template_source = if let Some(ref tr) = vm.spec.template_ref {
        let disk = crate::engine::template::resolve_template_disk(&state.pool, tr).await?;
        let (tname, tver) = crate::engine::template::parse_template_ref(tr);
        update_task_progress(
            &state.pool,
            msg.task_id,
            10,
            "Checking golden image on host",
        )
        .await?;
        if crate::engine::template_image_fetch::ensure_template_disk(
            &state.pool,
            host_id,
            &disk,
            &tname,
            &tver,
        )
        .await?
        {
            update_task_progress(&state.pool, msg.task_id, 35, "Downloaded golden image").await?;
        }
        Some(disk)
    } else {
        None
    };

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let spec_json = serde_json::to_string(&vm)?;
    let resp = agent_client::apply_vm(
        &mut client,
        &spec_json,
        &disk_path,
        template_source.as_deref(),
        "",
        "",
        "",
    )
    .await?;

    // Persist the created domain's identity IMMEDIATELY, before attempting to start it.
    // If the guest fails to start (a non-transient error that `on_task_failure` does
    // not retry), the domain still exists on the host — persisting the uuid only after
    // a successful start would strand this row in 'creating' with an empty uuid,
    // recoverable only by a later inventory sweep re-adopting it by name.
    crate::db::query(
        "UPDATE vms SET uuid = ?, observed_state = 'defined', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&resp.uuid)
    .bind(vm_id)
    .execute(&state.pool)
    .await?;

    let desired_state: String = crate::db::query_scalar("SELECT desired_state FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found after apply", vm_id))?;

    let needs_start = desired_state == "running";
    if needs_start {
        vm_lifecycle::set_vm_phase(&state.pool, vm_id, vm_lifecycle::PHASE_STARTING).await?;
        agent_client::vm_power(&mut client, &row.0, "start", None).await?;
        crate::db::query(
            "UPDATE vms SET observed_state = 'running', updated_at = datetime('now') WHERE id = ?",
        )
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    }

    vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await?;

    state.emit_event("vm.apply", format!("VM {} applied on host", row.0));

    if let Err(e) = enqueue_task(
        state,
        "vm.guest_tools.install",
        serde_json::json!({ "vm_id": vm_id.to_string() }),
        Some("vm"),
        Some(vm_id),
        Some(host_id),
    )
    .await
    {
        tracing::warn!(vm_id = %vm_id, "guest_tools.install enqueue failed: {e:?}");
    }

    update_task_progress(&state.pool, msg.task_id, 100, "VM defined").await?;
    Ok(())
}

async fn vm_power(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let action = msg.payload["action"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("action missing"))?
        .to_string();
    let power_mode = msg.payload["mode"].as_str().filter(|s| !s.is_empty());

    let phase = match action.as_str() {
        "start" | "reboot" => vm_lifecycle::PHASE_STARTING,
        "stop" => vm_lifecycle::PHASE_STOPPING,
        _ => vm_lifecycle::PHASE_IDLE,
    };
    vm_lifecycle::set_vm_phase_clear_error(&state.pool, vm_id, phase).await?;

    let row: (String, Option<Uuid>, String) =
        crate::db::query_as("SELECT name, host_id, observed_state FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let prior_observed_state = row.2;
    let resp = if crate::engine::fluxvm_fleet::is_fluxvm(&state.pool, vm_id).await {
        crate::engine::fluxvm_fleet::power(&state.pool, host_id, &row.0, &action).await?
    } else {
        let agent_addr = host_agent_addr(&state.pool, host_id).await?;
        let mut client = agent_client::connect(&agent_addr).await?;
        agent_client::vm_power(&mut client, &row.0, &action, power_mode).await?
    };

    let desired = match action.as_str() {
        "start" | "resume" | "reboot" | "reset" => "running",
        "stop" | "shutdown" => "stopped",
        "pause" => "paused",
        "managedsave" => "sleeping",
        _ => "running",
    };
    crate::db::query(
        "UPDATE vms SET desired_state = ?, observed_state = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(desired)
    .bind(&resp.state)
    .bind(vm_id)
    .execute(&state.pool)
    .await?;
    if action == "managedsave" {
        let preempt = msg.payload["preempt"].as_bool() == Some(true);
        crate::db::query(
            "UPDATE vms SET slept_at = datetime('now'),
             preempted_at = CASE WHEN ? THEN datetime('now') ELSE NULL END WHERE id = ?",
        )
        .bind(preempt)
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
        let reason = match msg.payload["idle_minutes"].as_i64() {
            _ if preempt => msg.payload["reason"]
                .as_str()
                .unwrap_or("preempted")
                .to_string(),
            Some(m) if msg.payload["auto_sleep"].as_bool() == Some(true) => {
                format!("idle {m} min")
            }
            _ => "manual".to_string(),
        };
        crate::engine::vm_sleep::record(&state.pool, vm_id, "sleep", &reason).await;
    } else if desired == "running" {
        let was_sleeping: bool =
            crate::db::query_scalar("SELECT slept_at IS NOT NULL FROM vms WHERE id = ?")
                .bind(vm_id)
                .fetch_one(&state.pool)
                .await
                .unwrap_or(false);
        crate::db::query(
            "UPDATE vms SET slept_at = NULL, preempted_at = NULL, last_active_at = datetime('now') WHERE id = ?",
        )
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
        if was_sleeping && action == "start" {
            let reason = if msg.payload["resume_preempted"].as_bool() == Some(true) {
                "capacity freed"
            } else {
                "manual"
            };
            crate::engine::vm_sleep::record(&state.pool, vm_id, "wake", reason).await;
        }
    }
    if action == "managedsave" || action == "start" {
        if let Err(e) = crate::engine::vm_sleep::sync_host(state, host_id).await {
            tracing::warn!(vm = %row.0, error = %e, "wake set sync failed");
        }
    }

    // Derive lifecycle_phase from the new desired/observed states so the VM
    // doesn't stay stuck in "starting" or "stopping" after the action completes.
    vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await?;

    // Only notify on an actual transition — a task that re-runs against a VM stuck
    // in the same observed state (e.g. hung "shutting down") would otherwise spam
    // an identical event every time it's re-enqueued.
    if prior_observed_state != resp.state {
        state.emit_event("vm.power", format!("VM {} -> {}", row.0, resp.state));
    }
    update_task_progress(&state.pool, msg.task_id, 100, &resp.state).await?;
    Ok(())
}

async fn vm_install(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    vm_lifecycle::set_vm_phase_clear_error(&state.pool, vm_id, vm_lifecycle::PHASE_STARTING)
        .await?;

    let row: (String, String, Option<Uuid>) =
        crate::db::query_as("SELECT name, spec_json, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .2
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let vm: machina_spec::VirtualMachine = serde_json::from_str(&row.1)?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::vm_libvirt_invoke(
        &mut client,
        &row.0,
        "domain.install",
        &serde_json::json!({ "spec": vm }),
    )
    .await?;

    crate::db::query(
        "UPDATE vms SET desired_state = 'running', observed_state = 'defined', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(vm_id)
    .execute(&state.pool)
    .await?;

    state.emit_event("vm.install", format!("VM {} install started", row.0));
    update_task_progress(&state.pool, msg.task_id, 100, "install started").await?;
    Ok(())
}

async fn vm_delete(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    vm_lifecycle::set_vm_phase(&state.pool, vm_id, vm_lifecycle::PHASE_DELETING).await?;

    let row: (String, Option<Uuid>, String, Option<String>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt'), k8s_namespace, observed_state FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;

    if row.4 == "missing" {
        // Domain already absent from hypervisor inventory — drop the stale DB row only.
    } else if row.2 == "kubevirt" {
        let ns = row.3.unwrap_or_else(|| "default".into());
        crate::engine::kubevirt_inventory::delete_kubevirt_vm(
            &state.config.daemon_base_url,
            &ns,
            &row.0,
        )
        .await?;
    } else if row.2 == crate::engine::fluxvm_fleet::SOURCE {
        if let Some(host_id) = row.1 {
            crate::engine::fluxvm_fleet::delete(&state.pool, host_id, &row.0).await?;
        }
    } else if let Some(host_id) = row.1 {
        let agent_addr = host_agent_addr(&state.pool, host_id).await?;
        let mut client = agent_client::connect(&agent_addr).await?;
        let delete_disks = msg.payload["delete_disks"].as_bool().unwrap_or(false);
        agent_client::delete_vm(&mut client, &row.0, delete_disks).await?;
    } else {
        // No host assigned — inventory row only.
    }

    // Best-effort: delete any Atlas backend volumes bound to this VM before the
    // vms row (and its bindings, via CASCADE) is removed, to avoid orphaning
    // storage on the Atlas side.
    if let Ok(client) = crate::engine::atlas_bridge::require_client(&state.config) {
        let bound = crate::engine::atlas_vm::list_vm_volumes(&state.pool, vm_id)
            .await
            .unwrap_or_default();
        for v in bound {
            if let Err(e) = client.delete_volume(&v.volume_id).await {
                tracing::warn!(volume_id = %v.volume_id, error = %e, "atlas volume delete failed on vm delete");
            }
        }
    }

    // Leave a tombstone so EC2-style clients still see the instance as `terminated` for a while.
    let _ = crate::db::query(
        "INSERT INTO terminated_instances (id, name, instance_type, project, vcpus, memory_mib) \
         SELECT v.id, v.name, f.name, v.project, v.vcpus, v.memory_mib FROM vms v LEFT JOIN flavors f ON f.id = v.flavor_id WHERE v.id = ? \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, instance_type = excluded.instance_type, project = excluded.project, \
             vcpus = excluded.vcpus, memory_mib = excluded.memory_mib, terminated_at = CURRENT_TIMESTAMP",
    )
    .bind(vm_id)
    .execute(&state.pool)
    .await;
    crate::api::volumes::purge_terminating_volumes(state, vm_id, row.1).await;
    crate::db::query("DELETE FROM vm_forks WHERE fork_vm_id = ?1 OR source_vm_id = ?1")
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    crate::db::query("DELETE FROM vm_restore_points WHERE vm_id = ?")
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    crate::db::query("DELETE FROM vms WHERE id = ?")
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    state.emit_event("vm.delete", format!("VM {} deleted", row.0));
    update_task_progress(&state.pool, msg.task_id, 100, "deleted").await?;
    Ok(())
}

const EMPTY_SCANS_BEFORE_PRUNE: u32 = 3;

fn empty_scans() -> &'static std::sync::Mutex<HashMap<Uuid, u32>> {
    static EMPTY: std::sync::OnceLock<std::sync::Mutex<HashMap<Uuid, u32>>> =
        std::sync::OnceLock::new();
    EMPTY.get_or_init(Default::default)
}

/// Counts consecutive empty libvirt scans for a host; returns the new count.
fn record_empty_scan(host_id: Uuid) -> u32 {
    let mut m = empty_scans().lock().unwrap_or_else(|e| e.into_inner());
    let n = m.entry(host_id).or_insert(0);
    *n = n.saturating_add(1);
    *n
}

fn clear_empty_scans(host_id: Uuid) {
    empty_scans()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&host_id);
}

async fn host_inventory(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    // Cap the inventory RPCs: a host whose libvirtd is wedged accepts the TCP
    // connection but never returns from heartbeat/list_vms, which would otherwise
    // block this single serial worker (and thus every other host's tasks) forever.
    let inv = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut client = agent_client::connect(&agent_addr).await?;
        let hb = agent_client::heartbeat(&mut client, &host_id.to_string()).await?;
        let list = agent_client::list_vms(&mut client).await?;
        let info = agent_client::get_host_info(&mut client).await.ok();
        // Best-effort: an agent that predates ListSprites, or whose co-located
        // daemon is down, must not fail the whole inventory tick over a
        // purely informational sprite listing.
        let sprites = agent_client::list_sprites(&mut client).await.ok();
        Ok::<_, anyhow::Error>((hb, list, info, sprites))
    })
    .await
    .map_err(|_| anyhow::anyhow!("agent inventory RPC timed out for host {host_id}"))??;
    let (hb, list, info, sprites) = inv;

    let sprite_snapshot = match sprites {
        Some(resp) => crate::engine::sprite_inventory::HostSpriteSnapshot {
            host_id,
            fetched_at: chrono::Utc::now(),
            reachable: resp.daemon_reachable,
            error: (!resp.daemon_reachable && !resp.error.is_empty()).then_some(resp.error),
            sprites: resp.sprites,
        },
        None => crate::engine::sprite_inventory::HostSpriteSnapshot {
            host_id,
            fetched_at: chrono::Utc::now(),
            reachable: false,
            error: Some("agent does not support ListSprites (upgrade machina-agent)".into()),
            sprites: vec![],
        },
    };
    state.sprite_inventory.update(sprite_snapshot).await;

    crate::db::query(
        // Clear any stale fence flag: a host that just heartbeated is alive and
        // reachable, so a future failure must be fenced afresh before HA recovers it.
        "UPDATE hosts SET vm_count = ?, state = ?, last_heartbeat_at = datetime('now'), fenced = FALSE,
         cpu_percent = ?, memory_used_mib = ?, memory_total_mib = ?,
         cpu_model = COALESCE(?, cpu_model),
         libvirt_version = COALESCE(?, libvirt_version),
         qemu_version = COALESCE(?, qemu_version)
         WHERE id = ?",
    )
    .bind(list.vms.len() as i32)
    .bind(&hb.state)
    .bind(hb.cpu_percent)
    .bind(hb.memory_used_mib as i64)
    .bind(hb.memory_total_mib as i64)
    .bind(
        info.as_ref()
            .map(|i| i.cpu_model.as_str())
            .filter(|s| !s.is_empty()),
    )
    .bind(
        info.as_ref()
            .map(|i| i.libvirt_version.as_str())
            .filter(|s| !s.is_empty()),
    )
    .bind(
        info.as_ref()
            .map(|i| i.qemu_version.as_str())
            .filter(|s| !s.is_empty()),
    )
    .bind(host_id)
    .execute(&state.pool)
    .await?;

    // cluster_id is nullable; select as Option so a NULL decodes to None (a
    // non-Option Uuid would raise a ColumnDecode error) — then .flatten() folds
    // "no row" and "NULL cluster" into the same not-found error.
    let cluster_id: Uuid =
        crate::db::query_scalar::<_, Option<Uuid>>("SELECT cluster_id FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("host {} not found or has no cluster", host_id))?;

    let mut seen_names: HashSet<String> = HashSet::new();
    let fluxvm_ok = list.fluxvm_ok;
    let (flux_vms, libvirt_vms): (Vec<_>, Vec<_>) = list
        .vms
        .into_iter()
        .partition(|v| v.backend == crate::engine::fluxvm_fleet::SOURCE);

    for vm in libvirt_vms {
        seen_names.insert(vm.name.clone());
        // Match on the libvirt UUID (the VM's stable identity) when reported, so
        // a migrated VM updates its own row and two same-name VMs on different
        // hosts stay distinct. Fall back to name only when no uuid is available.
        let existing: Option<(Uuid, bool, Option<Uuid>)> = if !vm.uuid.trim().is_empty() {
            crate::db::query_as(
                "SELECT id, managed, host_id FROM vms
                 WHERE cluster_id = ? AND uuid = ? AND inventory_source = 'libvirt'",
            )
            .bind(cluster_id)
            .bind(vm.uuid.trim())
            .fetch_optional(&state.pool)
            .await?
        } else {
            crate::db::query_as(
                "SELECT id, managed, host_id FROM vms
                 WHERE cluster_id = ? AND name = ? AND inventory_source = 'libvirt'",
            )
            .bind(cluster_id)
            .bind(&vm.name)
            .fetch_optional(&state.pool)
            .await?
        };

        if let Some((id, _managed, current_host)) = existing {
            // Post-migration leftover guard: after a live migration with
            // undefine_source=false, the SOURCE host still has an inactive domain with
            // the same uuid, while the VM's authoritative host_id is already the dest
            // (the migrate task sets it). Without this, the source's next inventory tick
            // would set host_id back to the source and observed_state to its 'shutoff',
            // making the VM flap between hosts and appear stopped while it's really
            // running on the dest. If the domain here is inactive AND belongs to a
            // DIFFERENT host than the VM currently records, treat it as a stale leftover:
            // note we saw it (so reconcile won't mark it missing) but don't steal the VM
            // back or clobber its state. The dest host always matches current_host, so
            // legitimate (including cold-migrated shutoff) VMs are unaffected.
            let domain_active = matches!(
                vm.state.as_str(),
                "running" | "blocked" | "paused" | "pmsuspended"
            );
            let is_migration_leftover =
                matches!(current_host, Some(h) if h != host_id) && !domain_active;
            if is_migration_leftover {
                crate::db::query("UPDATE vms SET last_seen_at = datetime('now') WHERE id = ?")
                    .bind(id)
                    .execute(&state.pool)
                    .await?;
                continue;
            }
            // Refresh `name` too: we now match on uuid, so a domain renamed in
            // place (same uuid, new name) must adopt the new name — otherwise the
            // name-keyed reconcile below would treat the stale name as absent and
            // mark this VM 'missing' / prune it.
            crate::db::query(
                "UPDATE vms SET host_id = ?, name = ?, observed_state = ?, uuid = COALESCE(NULLIF(?, ''), uuid),
                 vcpus = ?, memory_mib = ?, guest_ip = CASE WHEN ? != '' THEN ? ELSE guest_ip END,
                 guest_ips = CASE WHEN ? != '[]' THEN ? ELSE guest_ips END,
                 last_seen_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
            )
            .bind(host_id)
            .bind(&vm.name)
            .bind(&vm.state)
            .bind(&vm.uuid)
            .bind(vm.vcpus as i32)
            .bind(vm.memory_mb as i64)
            .bind(&vm.guest_ip)
            .bind(&vm.guest_ip)
            .bind(serde_json::to_string(&vm.guest_ips).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(&vm.guest_ips).unwrap_or_else(|_| "[]".into()))
            .bind(id)
            .execute(&state.pool)
            .await?;

            if domain_active {
                crate::engine::vm_sleep::observe_activity(
                    &state.pool,
                    id,
                    vm.cpu_percent,
                    vm.net_bytes,
                )
                .await;
            }
            let metrics_result = crate::db::query(
                "INSERT INTO vm_metrics (vm_id, cpu_percent, memory_used_mib, disk_read_iops, disk_write_iops, net_bytes, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
                 ON CONFLICT (vm_id) DO UPDATE SET
                   cpu_percent = EXCLUDED.cpu_percent,
                   memory_used_mib = EXCLUDED.memory_used_mib,
                   disk_read_iops = EXCLUDED.disk_read_iops,
                   disk_write_iops = EXCLUDED.disk_write_iops,
                   net_bytes = EXCLUDED.net_bytes,
                   updated_at = datetime('now')",
            )
            .bind(id)
            .bind(vm.cpu_percent)
            .bind(vm.memory_used_mib as i64)
            .bind(vm.disk_read_iops as i64)
            .bind(vm.disk_write_iops as i64)
            .bind(vm.net_bytes as i64)
            .execute(&state.pool)
            .await;
            if let Err(e) = metrics_result {
                tracing::warn!(vm_id = %id, "vm_metrics upsert failed: {e:#}");
            }

            if vm.state == "running" {
                crate::engine::vm_health::sync_guest_tools(&state.pool, id, &vm.name, host_id)
                    .await;
            }
        } else {
            let new_id = Uuid::new_v4();
            crate::db::query(
                "INSERT INTO vms (id, cluster_id, host_id, name, spec_json, desired_state, observed_state,
                 uuid, vcpus, memory_mib, managed, lifecycle_phase)
                 VALUES (?, ?, ?, ?, '{}', 'unknown', ?, ?, ?, ?, FALSE, 'idle')",
            )
            .bind(new_id)
            .bind(cluster_id)
            .bind(host_id)
            .bind(&vm.name)
            .bind(&vm.state)
            .bind(&vm.uuid)
            .bind(vm.vcpus as i32)
            .bind(vm.memory_mb as i64)
            .execute(&state.pool)
            .await?;
            state.emit_event(
                "vm.discovered",
                format!("Discovered unmanaged VM '{}' on host", vm.name),
            );
        }
    }

    if let Err(e) =
        crate::engine::fluxvm_fleet::sync_host(state, host_id, cluster_id, &flux_vms, fluxvm_ok)
            .await
    {
        tracing::warn!(%host_id, "FluxVM inventory sync: {e:#}");
    }

    // Treat a successful-but-empty scan as inconclusive rather than authoritative:
    // libvirtd can return an empty list right after a reconnect/restart, and a
    // hard RPC failure already errored out above. Reconciling on empty would
    // DELETE every unmanaged VM and mark managed ones 'missing' on one bad tick
    // (the KubeVirt path guards the same way). Only after EMPTY_SCANS_BEFORE_PRUNE
    // consecutive empty scans is the host taken to really have no domains;
    // otherwise its last VMs would stay listed forever.
    if seen_names.is_empty() && record_empty_scan(host_id) < EMPTY_SCANS_BEFORE_PRUNE {
        tracing::warn!(%host_id, "libvirt inventory returned no VMs — skipping prune/mark-missing this tick");
    } else {
        if !seen_names.is_empty() {
            clear_empty_scans(host_id);
        }
        crate::engine::vm_inventory::reconcile_libvirt_host(
            state,
            host_id,
            cluster_id,
            &seen_names,
        )
        .await?;
    }

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    if let Err(e) =
        crate::engine::network_sync::sync_host_networks(&state.pool, host_id, &agent_addr).await
    {
        tracing::warn!(%host_id, "network sync during inventory: {e:#}");
    }
    if let Err(e) =
        crate::engine::storage_sync::sync_host_storage(&state.pool, host_id, &agent_addr).await
    {
        tracing::warn!(%host_id, "storage sync during inventory: {e:#}");
    }
    if let Err(e) = crate::engine::zeus_firewall::sync::sync_host_posture(
        &state.pool,
        &state.config,
        host_id,
        &agent_addr,
    )
    .await
    {
        tracing::warn!(%host_id, "firewall posture sync during inventory: {e:#}");
    }
    match crate::engine::bpf::policies::sync_hosts(&state.pool, Some(&[host_id.to_string()])).await
    {
        Ok(r) if r.iter().any(|x| x["ok"] != true) => {
            let bpfd_absent = r.iter().filter(|x| x["ok"] != true).all(|x| {
                x["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("machina-bpfd not reachable"))
            });
            if bpfd_absent {
                tracing::debug!(%host_id, "machina-bpfd not running; native policy sync skipped");
            } else {
                tracing::warn!(%host_id, "native eBPF policy sync during inventory: {r:?}");
            }
        }
        Err(e) => tracing::warn!(%host_id, "native eBPF policy sync during inventory: {e:#}"),
        _ => {}
    }

    update_task_progress(&state.pool, msg.task_id, 100, "inventory synced").await?;
    Ok(())
}

async fn vm_migrate(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let dest_host_id: Uuid = msg.payload["dest_host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("dest_host_id missing"))?;
    crate::api::cloud::check_vm_host(&state.pool, vm_id, dest_host_id)
        .await
        .map_err(|e| anyhow::anyhow!("cloud host placement: {e:?}"))?;
    let live = msg.payload["live"].as_bool().unwrap_or(true);
    let bandwidth_mib = msg.payload["bandwidth_mib"].as_u64().unwrap_or(0);
    let postcopy = msg.payload["postcopy"].as_bool().unwrap_or(false);
    // Default true: an absent field (payload built by a path other than the
    // API's MigrateVmBody, which already defaults true) should still avoid
    // leaving the VM split-brain-defined on both hosts unless explicitly
    // told not to undefine the source.
    let undefine_source = msg.payload["undefine_source"].as_bool().unwrap_or(true);
    let tunnelled = msg.payload["tunnelled"].as_bool().unwrap_or(false);
    let migrate_disks: Vec<String> = msg.payload["migrate_disks"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let disks_uri = msg.payload["disks_uri"].as_str().map(str::to_string);
    let copy_storage = msg.payload["copy_storage"].as_bool().unwrap_or(false);

    vm_lifecycle::set_vm_phase(&state.pool, vm_id, vm_lifecycle::PHASE_MIGRATING).await?;

    let pre = crate::engine::migrate_precheck::run_migrate_precheck(
        &state.pool,
        vm_id,
        dest_host_id,
        live,
    )
    .await?;
    if !pre.ok {
        let msg = pre
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("{}: {}", c.name, c.message))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::bail!("migration pre-check failed: {msg}");
    }
    if crate::engine::fluxvm_fleet::is_fluxvm(&state.pool, vm_id).await {
        let mbps = msg.payload["bandwidth_mbps"]
            .as_u64()
            .unwrap_or(bandwidth_mib.saturating_mul(8));
        return crate::engine::fluxvm_fleet::migrate(
            state,
            msg.task_id,
            vm_id,
            dest_host_id,
            crate::engine::fluxvm_fleet::MigrateOpts {
                bandwidth_mbps: mbps,
                max_downtime_ms: msg.payload["max_downtime_ms"].as_u64().unwrap_or(0),
            },
        )
        .await;
    }

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let source_host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;

    let dest_uri: String =
        crate::db::query_scalar("SELECT COALESCE(NULLIF(libvirt_uri, ''), ?) FROM hosts WHERE id = ?")
            .bind(&state.config.default_libvirt_uri)
            .bind(dest_host_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("destination host {} not found", dest_host_id))?;

    let agent_addr = host_agent_addr(&state.pool, source_host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::migrate_vm(
        &mut client,
        &row.0,
        &dest_uri,
        live,
        bandwidth_mib,
        postcopy,
        undefine_source,
        tunnelled,
        migrate_disks,
        disks_uri,
        copy_storage,
    )
    .await?;

    {
        let mut tx = state.pool.begin().await?;
        crate::db::query("UPDATE vms SET host_id = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(dest_host_id)
            .bind(vm_id)
            .execute(&mut *tx)
            .await?;
        let job_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO migration_jobs (id, vm_id, source_host_id, dest_host_id, live, status, progress, precheck)
             VALUES (?, ?, ?, ?, ?, 'completed', 100, ?)",
        )
        .bind(job_id)
        .bind(vm_id)
        .bind(source_host_id)
        .bind(dest_host_id)
        .bind(live)
        .bind(serde_json::to_value(&pre)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }

    vm_lifecycle::sync_phase_from_observed(&state.pool, vm_id).await?;

    state.emit_event("vm.migrate", format!("VM {} migrated", row.0));
    update_task_progress(&state.pool, msg.task_id, 100, "migrated").await?;
    Ok(())
}

async fn vm_clone(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let new_name = msg.payload["new_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("new_name missing"))?
        .to_string();
    let clone_mode = msg.payload["clone_mode"]
        .as_str()
        .unwrap_or("linked")
        .to_string();

    let row: (String, Option<Uuid>, Uuid, serde_json::Value, i32, i64) = crate::db::query_as(
        "SELECT name, host_id, cluster_id, spec_json, vcpus, memory_mib FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;

    // Insert the DB record first so a hypervisor clone success always has a matching row.
    // The row starts with a placeholder uuid that is updated once the hypervisor responds.
    let new_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vms (id, cluster_id, host_id, name, spec_json, desired_state, observed_state, vcpus, memory_mib)
         VALUES (?, ?, ?, ?, ?, 'stopped', 'creating', ?, ?)",
    )
    .bind(new_id)
    .bind(row.2)
    .bind(host_id)
    .bind(&new_name)
    .bind(&row.3)
    .bind(row.4)
    .bind(row.5)
    .execute(&state.pool)
    .await?;

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let resp = match async {
        let mut client = agent_client::connect(&agent_addr).await?;
        agent_client::clone_vm(&mut client, &row.0, &new_name, &clone_mode).await
    }
    .await
    {
        Ok(r) => r,
        Err(e) => {
            // Remove the placeholder row so a failed clone doesn't leave a VM
            // stuck in observed_state='creating' forever (nothing else clears it).
            let _ = crate::db::query("DELETE FROM vms WHERE id = ? AND observed_state = 'creating'")
                .bind(new_id)
                .execute(&state.pool)
                .await;
            return Err(e);
        }
    };

    crate::db::query("UPDATE vms SET uuid = ?, observed_state = 'defined' WHERE id = ?")
        .bind(&resp.uuid)
        .bind(new_id)
        .execute(&state.pool)
        .await?;

    state.emit_event("vm.clone", format!("Cloned {} -> {}", row.0, new_name));
    update_task_progress(&state.pool, msg.task_id, 100, "cloned").await?;
    Ok(())
}

async fn host_maintenance(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    let action = msg.payload["action"]
        .as_str()
        .unwrap_or("enter")
        .to_string();
    let evacuate = msg.payload["evacuate"].as_bool().unwrap_or(true);

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::maintenance(&mut client, &action, evacuate).await?;

    if action == "enter" {
        crate::db::query("UPDATE hosts SET maintenance_mode = TRUE WHERE id = ?")
            .bind(host_id)
            .execute(&state.pool)
            .await?;

        if evacuate {
            let vms: Vec<(Uuid, String, i64)> = crate::db::query_as(
                "SELECT id, name, memory_mib FROM vms WHERE host_id = ? AND desired_state = 'running'",
            )
            .bind(host_id)
            .fetch_all(&state.pool)
            .await?;

            // Capacity-aware destinations: least-loaded first, each with its free
            // memory headroom. Reuses HA recovery's picker so evacuation spreads VMs
            // across hosts by real capacity instead of piling every VM onto the
            // single least-loaded host and overcommitting it (the old behavior).
            let candidates: Vec<(Uuid, i64)> = crate::db::query_as(
                "SELECT id, (memory_total_mib - memory_used_mib) AS headroom FROM hosts
                 WHERE id != ? AND state = 'online' AND maintenance_mode = FALSE AND schedulable = TRUE
                 ORDER BY vm_count, memory_used_mib",
            )
            .bind(host_id)
            .fetch_all(&state.pool)
            .await?;

            // Plan every VM's destination up front (with per-batch reservations) and
            // only enqueue if ALL fit — a partial evacuation still leaves the host
            // un-drainable, so fail loudly instead so the operator doesn't pull it.
            let mut reserved: std::collections::HashMap<Uuid, i64> =
                std::collections::HashMap::new();
            let mut plan: Vec<(Uuid, Uuid)> = Vec::new();
            let mut stranded: Vec<String> = Vec::new();
            for (vm_id, vm_name, mem) in &vms {
                match crate::engine::ha::pick_ha_dest(&candidates, &reserved, *mem) {
                    Some(dest) => {
                        *reserved.entry(dest).or_insert(0) += *mem;
                        plan.push((*vm_id, dest));
                    }
                    None => stranded.push(vm_name.clone()),
                }
            }
            if !stranded.is_empty() {
                anyhow::bail!(
                    "cannot evacuate host {host_id}: no other online host has capacity for VM(s): {} — do NOT power down",
                    stranded.join(", ")
                );
            }

            for (vm_id, dest_id) in plan {
                if let Err(e) = enqueue_task(
                    state,
                    "vm.migrate",
                    serde_json::json!({
                        "vm_id": vm_id.to_string(),
                        "dest_host_id": dest_id.to_string(),
                        "live": true,
                        // Source host is going down for maintenance; undefine on
                        // success so the VM can't autostart here while running on the
                        // destination (split-brain).
                        "undefine_source": true,
                    }),
                    Some("vm"),
                    Some(vm_id),
                    Some(host_id),
                )
                .await
                {
                    tracing::warn!(vm_id = %vm_id, host_id = %host_id, "vm.migrate enqueue failed during maintenance evacuation: {e:?}");
                }
            }

            // Confirm the drain actually completed before reporting success. The
            // migrations above are async/best-effort; returning success here let an
            // operator see the host "in maintenance" and power it down while VMs
            // were still running on it. Block until no running VM remains, or fail
            // so the operator knows NOT to pull the host.
            if !vms.is_empty() {
                confirm_host_drained(state, host_id, msg.task_id).await?;
            }
        }
    } else {
        crate::db::query("UPDATE hosts SET maintenance_mode = FALSE WHERE id = ?")
            .bind(host_id)
            .execute(&state.pool)
            .await?;
    }

    update_task_progress(&state.pool, msg.task_id, 100, &action).await?;
    Ok(())
}

/// Wait until the host has no running VMs left (evacuation actually finished), so
/// the maintenance task only reports success once the host is genuinely safe to
/// take down. Bounded by MACHINA_MAINTENANCE_DRAIN_TIMEOUT_SECS (default 900s); on
/// timeout it errors — the operator must NOT power down a host that still runs VMs.
async fn confirm_host_drained(
    state: &AppState,
    host_id: Uuid,
    task_id: Uuid,
) -> anyhow::Result<()> {
    let timeout_secs: i64 = std::env::var("MACHINA_MAINTENANCE_DRAIN_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(900);
    let poll = std::time::Duration::from_secs(5);
    let mut waited: i64 = 0;
    loop {
        let remaining: i64 = crate::db::query_scalar(
            "SELECT COUNT(*) FROM vms
             WHERE host_id = ? AND desired_state = 'running'
               AND observed_state IN ('running', 'blocked', 'paused')",
        )
        .bind(host_id)
        .fetch_one(&state.pool)
        .await?;
        if remaining == 0 {
            return Ok(());
        }
        if waited >= timeout_secs {
            anyhow::bail!(
                "host {host_id} drain INCOMPLETE: {remaining} running VM(s) did not evacuate \
                 within {timeout_secs}s — do NOT power down the host; check the vm.migrate tasks"
            );
        }
        let pct = 40 + ((waited * 55) / timeout_secs).clamp(0, 55) as i16;
        update_task_progress(
            &state.pool,
            task_id,
            pct,
            &format!("draining: {remaining} running VM(s) remaining"),
        )
        .await?;
        tokio::time::sleep(poll).await;
        waited += 5;
    }
}

async fn ha_recover(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    crate::api::cloud::check_vm_host(&state.pool, vm_id, host_id)
        .await
        .map_err(|e| anyhow::anyhow!("cloud host placement: {e:?}"))?;
    let desired = msg.payload["desired_state"].as_str().unwrap_or("running");
    if crate::engine::fluxvm_fleet::is_fluxvm(&state.pool, vm_id).await {
        let previous = msg.payload["recovered_from_host_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok());
        return crate::engine::fluxvm_fleet::ha_recreate(
            state,
            msg.task_id,
            vm_id,
            host_id,
            previous,
            desired,
        )
        .await;
    }

    let row: (String, serde_json::Value) =
        crate::db::query_as("SELECT name, spec_json FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let vm: VirtualMachine = serde_json::from_value(row.1)?;
    let disk_path = disk_path_for(&state.config, &row.0);
    let template_source = if let Some(ref tr) = vm.spec.template_ref {
        let disk = crate::engine::template::resolve_template_disk(&state.pool, tr).await?;
        let (tname, tver) = crate::engine::template::parse_template_ref(tr);
        crate::engine::template_image_fetch::ensure_template_disk(
            &state.pool,
            host_id,
            &disk,
            &tname,
            &tver,
        )
        .await?;
        Some(disk)
    } else {
        None
    };

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let spec_json = serde_json::to_string(&vm)?;
    let resp = agent_client::apply_vm(
        &mut client,
        &spec_json,
        &disk_path,
        template_source.as_deref(),
        "",
        "",
        "",
    )
    .await?;

    // Power-on RPC BEFORE opening the write transaction. SQLite has a single
    // writer + 5s busy_timeout, so holding an open write tx across a multi-second
    // gRPC start would block every other writer — including the 5s leader-lease
    // renewal — risking spurious leadership loss. Both UPDATEs below are fast local
    // writes, so the tx is held only briefly. (Mirrors vm_apply / vm_migrate.)
    if desired == "running" {
        agent_client::vm_power(&mut client, &row.0, "start", None).await?;
    }

    let mut tx = state.pool.begin().await?;
    crate::db::query(
        "UPDATE vms SET uuid = ?, host_id = ?, observed_state = 'defined', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&resp.uuid)
    .bind(host_id)
    .bind(vm_id)
    .execute(&mut *tx)
    .await?;

    if desired == "running" {
        if let Err(e) = crate::db::query("UPDATE vms SET observed_state = 'running' WHERE id = ?")
            .bind(vm_id)
            .execute(&mut *tx)
            .await
        {
            tracing::warn!(vm_id = %vm_id, "ha_recover: failed to set observed_state=running after power-on: {e:#}");
        }
    }
    tx.commit().await?;

    state.emit_event("ha.recover", format!("VM {} recovered on new host", row.0));
    update_task_progress(&state.pool, msg.task_id, 100, "recovered").await?;
    Ok(())
}

/// Compensate a terminally-failed `ha.recover` task: `engine/ha.rs::recover_vms`
/// moves the VM's `host_id` to the destination BEFORE the recovery task actually
/// applies/starts it there (so the next 45s scan doesn't re-select the same
/// victim while recovery is in flight). If the task then fails for good — agent
/// unreachable, apply_vm error, etc. — that DB write is a "false success": the VM
/// row points at a host it was never actually created on, observed_state is
/// untouched (still whatever stale value it had), so reconcile's
/// `desired != observed` guard never fires either, and the VM is silently
/// orphaned forever. Revert host_id back to the failed source host (only if
/// nothing else has since moved it) and give back the recovery attempt so the
/// next HA scan can retry — mirroring the enqueue-failure compensation already
/// done inline in `recover_vms`.
pub(crate) async fn revert_failed_ha_recovery(
    pool: &DbPool,
    msg: &TaskMessage,
) -> anyhow::Result<()> {
    let Some(vm_id) = vm_id_from_payload(msg) else {
        return Ok(());
    };
    let Some(dest_host_id) = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return Ok(());
    };
    // Older in-flight payloads (enqueued before this field existed) won't carry
    // it; nothing safe to revert to in that case.
    let Some(source_host_id) = msg.payload["recovered_from_host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return Ok(());
    };

    let result = crate::db::query(
        "UPDATE vms SET host_id = ?, ha_recovery_count = MAX(ha_recovery_count - 1, 0),
         updated_at = datetime('now') WHERE id = ? AND host_id = ?",
    )
    .bind(source_host_id)
    .bind(vm_id)
    .bind(dest_host_id)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        tracing::warn!(vm_id = %vm_id, dest_host = %dest_host_id, source_host = %source_host_id,
            "ha.recover failed terminally — reverted host_id so HA can retry recovery instead of leaving the VM stuck pointing at a host it was never created on");
        let _ = crate::db::query(
            "INSERT INTO ha_events (id, vm_id, host_id, action, message) VALUES (?, ?, ?, 'ha.recover_failed', ?)",
        )
        .bind(Uuid::new_v4())
        .bind(vm_id)
        .bind(source_host_id)
        .bind(format!(
            "Recovery onto host {dest_host_id} failed and was reverted; will retry once eligible"
        ))
        .execute(pool)
        .await;
    }
    Ok(())
}

pub(crate) async fn host_agent_addr(pool: &DbPool, host_id: Uuid) -> anyhow::Result<String> {
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("host {} not found", host_id))?;
    Ok(addr)
}

fn disk_path_for(cfg: &ControllerConfig, name: &str) -> String {
    cfg.disk_image_dir
        .join(format!("{name}.qcow2"))
        .to_string_lossy()
        .into_owned()
}

async fn claim_task(pool: &DbPool, id: Uuid, owner: &str) -> anyhow::Result<bool> {
    let claimed: Option<Uuid> = crate::db::query_scalar(
        "UPDATE tasks SET status = 'running', claimed_by = ?, updated_at = datetime('now')
         WHERE id = ? AND status = 'pending'
         RETURNING id",
    )
    .bind(owner)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(claimed.is_some())
}

async fn mark_task_completed(pool: &DbPool, id: Uuid) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE tasks SET status = 'completed', progress = 100, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_task_failed(pool: &DbPool, id: Uuid, message: &str) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE tasks SET status = 'failed', message = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(message)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn update_task_progress(
    pool: &DbPool,
    id: Uuid,
    progress: i16,
    message: &str,
) -> anyhow::Result<()> {
    crate::db::query(
        "UPDATE tasks SET progress = ?, message = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(progress)
    .bind(message)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn vm_snapshot(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let record_id: Uuid = msg.payload["snapshot_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("snapshot_id missing"))?;
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    vm_lifecycle::set_vm_phase(&state.pool, vm_id, vm_lifecycle::PHASE_SNAPSHOTTING).await?;

    // Atlas-backed VMs snapshot their backend volumes through the Atlas control
    // plane instead of a libvirt snapshot on the agent.
    if crate::engine::atlas_vm::vm_is_atlas_backed(&state.pool, vm_id).await {
        let snap_name = msg.payload["name"].as_str().map(str::to_string);
        let jobs = crate::engine::atlas_vm::snapshot_vm(state, vm_id, snap_name.as_deref()).await?;
        // `snapshot_vm` only enqueues the Atlas job(s) (202 Accepted); poll each
        // to a terminal state here before recording 'completed' — persisting
        // completion off the bare "accepted" response would be a false success
        // if the snapshot later fails (or is still running) on the Atlas side.
        let client = crate::engine::atlas_bridge::require_client(&state.config)?;
        let mut terminal = Vec::with_capacity(jobs.len());
        for job in jobs {
            let job = match job.job_id() {
                Some(jid) => {
                    client
                        .wait_for_job(jid, std::time::Duration::from_secs(600))
                        .await?
                }
                None => job,
            };
            if job.state == "failed" {
                crate::db::query(
                    "UPDATE snapshot_records SET status = 'failed', message = ? WHERE id = ?",
                )
                .bind(job.error.as_deref().unwrap_or("Atlas snapshot failed"))
                .bind(record_id)
                .execute(&state.pool)
                .await?;
                anyhow::bail!(
                    "Atlas snapshot failed: {}",
                    job.error.as_deref().unwrap_or("unknown error")
                );
            }
            if !job.is_terminal() {
                crate::db::query(
                    "UPDATE snapshot_records SET status = 'failed', message = ? WHERE id = ?",
                )
                .bind("Atlas snapshot did not finish within the wait budget")
                .bind(record_id)
                .execute(&state.pool)
                .await?;
                anyhow::bail!(
                    "Atlas snapshot job {} did not reach a terminal state in time",
                    job.job_id().unwrap_or("?")
                );
            }
            terminal.push(job);
        }
        let ids: Vec<&str> = terminal.iter().filter_map(|j| j.job_id()).collect();
        let summary = ids.join(",");
        crate::db::query(
            "UPDATE snapshot_records SET status = 'completed', message = ?, snapshot_path = ? WHERE id = ?",
        )
        .bind(format!("Atlas snapshot job(s): {summary}"))
        .bind(&summary)
        .bind(record_id)
        .execute(&state.pool)
        .await?;
        state.emit_event(
            "vm.snapshot",
            format!("Atlas snapshot ({} job(s)) for VM {vm_id}", ids.len()),
        );
        return Ok(());
    }

    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT v.name, v.host_id, s.name FROM vms v JOIN snapshot_records s ON s.id = ? AND s.vm_id = v.id",
    )
    .bind(record_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("snapshot record {} not found or vm mismatch", record_id))?;
    let host_id = row.1.ok_or_else(|| anyhow::anyhow!("vm has no host"))?;

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let snap_name = msg.payload["name"].as_str().unwrap_or(&row.2).to_string();
    let description = msg.payload["description"]
        .as_str()
        .unwrap_or("machina platform snapshot")
        .to_string();
    let disk_only = msg.payload["disk_only"].as_bool().unwrap_or(false);
    let quiesce = msg.payload["quiesce"].as_bool().unwrap_or(false);
    let storage_mode = msg.payload["storage_mode"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let resp = agent_client::create_snapshot(
        &mut client,
        &row.0,
        &snap_name,
        &description,
        disk_only,
        quiesce,
        &storage_mode,
    )
    .await?;

    if resp.ok {
        crate::db::query(
            "UPDATE snapshot_records SET status = 'completed', message = ?, snapshot_path = ? WHERE id = ?",
        )
        .bind(&resp.message)
        .bind(&resp.disk_path)
        .bind(record_id)
            .execute(&state.pool)
            .await?;
        state.emit_event("vm.snapshot", format!("Snapshot {} on {}", row.2, row.0));

        // Prune old scheduled snapshots when the schedule has a retention limit.
        if let Some(keep) = msg.payload["retention"].as_i64().filter(|&r| r > 0) {
            let excess: Vec<(Uuid, String)> = crate::db::query_as(
                "SELECT id, name FROM snapshot_records
                 WHERE vm_id = ? AND name LIKE 'sched-%' AND status = 'completed'
                   AND id NOT IN (
                     SELECT id FROM snapshot_records
                     WHERE vm_id = ? AND name LIKE 'sched-%' AND status = 'completed'
                     ORDER BY created_at DESC LIMIT ?
                   )",
            )
            .bind(vm_id)
            .bind(vm_id)
            .bind(keep)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for (_old_id, old_name) in excess {
                // Enqueue the delete only. The vm.snapshot.delete handler removes
                // the snapshot_records row *after* libvirt confirms the deletion.
                // Deleting the record eagerly here orphaned the libvirt snapshot
                // (and its overlay files) whenever the delete task later failed —
                // untracked, so retention counts drifted and it leaked forever.
                let _ = enqueue_task(
                    state,
                    "vm.snapshot.delete",
                    serde_json::json!({ "vm_id": vm_id.to_string(), "snapshot_name": old_name }),
                    Some("vm"),
                    Some(vm_id),
                    Some(host_id),
                )
                .await;
            }
        }
    } else {
        crate::db::query("UPDATE snapshot_records SET status = 'failed', message = ? WHERE id = ?")
            .bind(&resp.message)
            .bind(record_id)
            .execute(&state.pool)
            .await?;
        anyhow::bail!("snapshot failed: {}", resp.message);
    }
    update_task_progress(&state.pool, msg.task_id, 100, "snapshot created").await?;
    Ok(())
}

async fn vm_snapshot_delete(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let snap_name = msg.payload["snapshot_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("snapshot_name missing"))?
        .to_string();

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    match agent_client::delete_snapshot(&mut client, &row.0, &snap_name).await {
        Ok(_) => {}
        Err(e) => {
            let msg_str = e.to_string().to_lowercase();
            // If libvirt already deleted the snapshot (e.g. after a revert that restructured the
            // snapshot chain), treat "not found" as success and just clean up the controller record.
            if !msg_str.contains("not found") && !msg_str.contains("notfound") {
                return Err(e);
            }
            tracing::warn!(snap = %snap_name, "snapshot not found in libvirt during delete — cleaning up controller record only");
        }
    }
    crate::db::query("DELETE FROM snapshot_records WHERE vm_id = ? AND name = ?")
        .bind(vm_id)
        .bind(&snap_name)
        .execute(&state.pool)
        .await?;
    update_task_progress(&state.pool, msg.task_id, 100, "snapshot deleted").await?;
    Ok(())
}

async fn vm_backup(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let record_id: Uuid = msg.payload["backup_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("backup_id missing"))?;
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;

    // Clear sticky last_error from a prior failed backup so the VM detail banner
    // doesn't keep showing the old failure while this retry is in flight.
    vm_lifecycle::set_vm_phase_clear_error(&state.pool, vm_id, vm_lifecycle::PHASE_BACKING_UP)
        .await?;

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let backup_type: String =
        crate::db::query_scalar("SELECT backup_type FROM backup_records WHERE id = ?")
            .bind(record_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("backup record {} not found", record_id))?;

    // Atlas-backed VMs back up their backend volumes to an Atlas RGW bucket
    // (RBD export-diff → S3) instead of a local qcow2 copy on the agent.
    if crate::engine::atlas_vm::vm_is_atlas_backed(&state.pool, vm_id).await {
        let mode = msg.payload["mode"].as_str().unwrap_or("data");
        let keep = msg.payload["keep"].as_i64().unwrap_or(0);
        let bucket = msg.payload["bucket_id"].as_str();
        let jobs = crate::engine::atlas_vm::backup_vm(state, vm_id, bucket, mode, keep).await?;
        // `backup_vm` only enqueues the Atlas backup job(s) (202 Accepted) — the
        // `resource.backup_id` a restore needs is not guaranteed to be populated
        // until the job is terminal. Poll each job here before recording
        // 'completed'; the previous code stored whatever (possibly empty)
        // backup id came back immediately, which both mis-reported an
        // in-progress/failed backup as done and could leave `backup_path` with
        // no usable id for a later restore.
        let client = crate::engine::atlas_bridge::require_client(&state.config)?;
        let mut terminal = Vec::with_capacity(jobs.len());
        for job in jobs {
            let job = match job.job_id() {
                Some(jid) => {
                    client
                        .wait_for_job(jid, std::time::Duration::from_secs(1800))
                        .await?
                }
                None => job,
            };
            if job.state == "failed" {
                crate::db::query(
                    "UPDATE backup_records SET status = 'failed', message = ? WHERE id = ?",
                )
                .bind(job.error.as_deref().unwrap_or("Atlas backup failed"))
                .bind(record_id)
                .execute(&state.pool)
                .await?;
                anyhow::bail!(
                    "Atlas backup failed: {}",
                    job.error.as_deref().unwrap_or("unknown error")
                );
            }
            if !job.is_terminal() {
                crate::db::query(
                    "UPDATE backup_records SET status = 'failed', message = ? WHERE id = ?",
                )
                .bind("Atlas backup did not finish within the wait budget")
                .bind(record_id)
                .execute(&state.pool)
                .await?;
                anyhow::bail!(
                    "Atlas backup job {} did not reach a terminal state in time",
                    job.job_id().unwrap_or("?")
                );
            }
            terminal.push(job);
        }
        let job_ids: Vec<&str> = terminal.iter().filter_map(|j| j.job_id()).collect();
        // Persist the Atlas backup id(s) (not the job id) so restore can target
        // them; comma-joined when a VM has multiple Atlas volumes.
        let backup_ids: Vec<String> = terminal
            .iter()
            .filter_map(|j| j.resource_backup_id())
            .collect();
        let stored = backup_ids.join(",");
        crate::db::query(
            "UPDATE backup_records SET status = 'completed', message = ?, backup_path = ? WHERE id = ?",
        )
        .bind(format!("Atlas backup job(s): {}", job_ids.join(",")))
        .bind(&stored)
        .bind(record_id)
        .execute(&state.pool)
        .await?;
        state.emit_event(
            "vm.backup",
            format!("Atlas backup ({} job(s)) for VM {vm_id}", job_ids.len()),
        );
        update_task_progress(&state.pool, msg.task_id, 100, "atlas backup complete").await?;
        return Ok(());
    }

    // `record_id` (unique per backup row) is included, not just a 1-second-resolution
    // timestamp: two backups of the same VM started in the same wall-clock second
    // (a manual "backup now" racing the scheduler, or a double-click) previously
    // produced the identical path, so both `backup_records` rows pointed at one
    // file — and the retention GC deleting the older row's "duplicate" path then
    // destroyed the still-current backup along with it.
    let dest = state
        .config
        .backup_dir
        .join(format!(
            "{}-{}-{}.qcow2",
            row.0,
            chrono::Utc::now().format("%Y%m%d%H%M%S"),
            record_id.simple(),
        ))
        .to_string_lossy()
        .into_owned();

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let resp = if backup_type == "incremental" {
        run_incremental_backup(&state.pool, vm_id, &row.0, &dest, &agent_addr).await?
    } else {
        agent_client::backup_vm(&mut client, &row.0, &dest).await?
    };
    if resp.ok && msg.payload["export"].as_bool() == Some(true) {
        let check_path = resp.path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            std::process::Command::new("qemu-img")
                .args(["check", &check_path])
                .output()
        })
        .await;
    }

    if resp.ok {
        crate::db::query("UPDATE backup_records SET status = 'completed', backup_path = ?, message = ? WHERE id = ?")
            .bind(&resp.path)
            .bind(&resp.message)
            .bind(record_id)
            .execute(&state.pool)
            .await?;

        if let Some(tid) = msg.payload["target_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            if let Ok(Some((kind, cfg))) = crate::db::query_as::<_, (String, serde_json::Value)>(
                "SELECT kind, config_json FROM backup_targets WHERE id = ?",
            )
            .bind(tid)
            .fetch_optional(&state.pool)
            .await
            {
                if kind == "s3" {
                    let bucket = cfg["bucket"].as_str().unwrap_or("");
                    // Strip leading dashes from prefix so it cannot become an AWS CLI flag
                    // (e.g. "--no-sign-request" in the prefix field of a malicious config).
                    let raw_prefix = cfg["prefix"].as_str().unwrap_or("machina");
                    let prefix = raw_prefix.trim_start_matches('-');
                    let prefix = if prefix.is_empty() { "machina" } else { prefix };
                    if !bucket.is_empty() {
                        let key = format!(
                            "{prefix}/{}",
                            std::path::Path::new(&resp.path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("backup.qcow2")
                        );
                        let dest = format!("s3://{bucket}/{key}");
                        // Same flag-injection class as `prefix` above: the AWS CLI is a Python
                        // argparse tool, and `endpoint_url` is passed as a bare argv element
                        // immediately after its own `--endpoint-url` flag, so an unvalidated
                        // value starting with '-' could be parsed as a different aws CLI flag
                        // (e.g. "--no-verify-ssl") instead of the endpoint URL.
                        let raw_endpoint = cfg["endpoint_url"].as_str().unwrap_or("");
                        let endpoint = raw_endpoint.trim_start_matches('-').to_string();
                        let src_path = resp.path.clone();
                        let dest_clone = dest.clone();
                        let aws_result = tokio::task::spawn_blocking(move || {
                            let mut aws_args =
                                vec!["s3".to_string(), "cp".to_string(), src_path, dest_clone];
                            if !endpoint.is_empty() {
                                aws_args.push("--endpoint-url".to_string());
                                aws_args.push(endpoint);
                            }
                            std::process::Command::new("aws").args(&aws_args).output()
                        })
                        .await;
                        match aws_result {
                            Ok(Ok(out)) if out.status.success() => {
                                crate::db::query("UPDATE backup_records SET message = ? WHERE id = ?")
                                    .bind(format!("{}; uploaded to {dest}", resp.message))
                                    .bind(record_id)
                                    .execute(&state.pool)
                                    .await?;
                            }
                            Ok(Ok(out)) => {
                                tracing::warn!(
                                    "S3 upload failed: {}",
                                    String::from_utf8_lossy(&out.stderr)
                                );
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("aws cli not available for S3 upload: {e}")
                            }
                            Err(e) => tracing::warn!("S3 upload task panicked: {e}"),
                        }
                    }
                }
            }
        }

        state.emit_event("vm.backup", format!("Backup {} -> {}", row.0, resp.path));
    } else {
        crate::db::query("UPDATE backup_records SET status = 'failed', message = ? WHERE id = ?")
            .bind(&resp.message)
            .bind(record_id)
            .execute(&state.pool)
            .await?;
        anyhow::bail!("backup failed: {}", resp.message);
    }
    update_task_progress(&state.pool, msg.task_id, 100, "backup complete").await?;
    Ok(())
}

async fn vm_snapshot_revert(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let snap_name = msg.payload["snapshot_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("snapshot_name missing"))?
        .to_string();

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let resp = agent_client::revert_snapshot(&mut client, &row.0, &snap_name).await?;
    if !resp.ok {
        anyhow::bail!("revert failed: {}", resp.message);
    }
    state.emit_event(
        "vm.snapshot.revert",
        format!("Reverted {} to {}", row.0, snap_name),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "snapshot reverted").await?;
    Ok(())
}

async fn vm_snapshot_clone(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let snap_name = msg.payload["snapshot_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("snapshot_name missing"))?
        .to_string();
    let new_name = msg.payload["new_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("new_name missing"))?
        .to_string();
    let revert_source = msg.payload["revert_source"].as_bool().unwrap_or(false);
    let dest_host_id = msg.payload["dest_host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok());

    let row: (String, Option<Uuid>, Uuid, serde_json::Value, i32, i64) = crate::db::query_as(
        "SELECT name, host_id, cluster_id, spec_json, vcpus, memory_mib FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let new_disk_path = disk_path_for(&state.config, &new_name);
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;

    let label = if revert_source {
        "revert + clone"
    } else {
        "clone at snapshot point"
    };
    update_task_progress(&state.pool, msg.task_id, 30, label).await?;
    let resp = agent_client::clone_from_snapshot(
        &mut client,
        &row.0,
        &snap_name,
        &new_name,
        &new_disk_path,
        revert_source,
    )
    .await?;
    if !resp.ok {
        anyhow::bail!("clone from snapshot failed: {}", resp.message);
    }

    let new_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vms (id, cluster_id, host_id, name, spec_json, desired_state, observed_state, uuid, vcpus, memory_mib)
         VALUES (?, ?, ?, ?, ?, 'stopped', 'defined', ?, ?, ?)",
    )
    .bind(new_id)
    .bind(row.2)
    .bind(host_id)
    .bind(&new_name)
    .bind(&row.3)
    .bind(&resp.uuid)
    .bind(row.4)
    .bind(row.5)
    .execute(&state.pool)
    .await?;

    if let Some(dest) = dest_host_id {
        if dest != host_id {
            update_task_progress(
                &state.pool,
                msg.task_id,
                80,
                "queueing migration to dest host",
            )
            .await?;
            let live_migrate = msg.payload["live"].as_bool().unwrap_or(true);
            let source_running: String =
                crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
                    .bind(vm_id)
                    .fetch_optional(&state.pool)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!("vm {} disappeared before migration could start", vm_id)
                    })?;
            let use_live = live_migrate && source_running == "running";
            if use_live {
                vm_lifecycle::set_vm_phase(&state.pool, new_id, vm_lifecycle::PHASE_STARTING)
                    .await?;
                agent_client::vm_power(&mut client, &new_name, "start", None).await?;
                crate::db::query("UPDATE vms SET desired_state = 'running', observed_state = 'running' WHERE id = ?")
                    .bind(new_id)
                    .execute(&state.pool)
                    .await?;
            }
            if let Err(e) = enqueue_task(
                state,
                "vm.migrate",
                serde_json::json!({
                    "vm_id": new_id.to_string(),
                    "dest_host_id": dest.to_string(),
                    "live": use_live,
                }),
                Some("vm"),
                Some(new_id),
                Some(host_id),
            )
            .await
            {
                tracing::warn!(vm_id = %new_id, dest = %dest, "vm.migrate enqueue failed after snapshot clone: {}; resetting desired_state to stopped", e.message);
                let _ = crate::db::query("UPDATE vms SET desired_state = 'stopped' WHERE id = ?")
                    .bind(new_id)
                    .execute(&state.pool)
                    .await;
            }
        }
    }

    state.emit_event(
        "vm.snapshot.clone",
        format!(
            "Cloned {} from snapshot {} as {}",
            row.0, snap_name, new_name
        ),
    );
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        "clone from snapshot complete",
    )
    .await?;
    Ok(())
}

/// Retention GC: delete a backup's on-disk file (via the host agent) or its Atlas backup, then
/// drop the catalog row. Enqueued by fleet_backup_scheduler so backup storage is reclaimed.
async fn vm_backup_delete(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let record_id: Uuid = msg.payload["backup_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("backup_id missing"))?;

    let row: Option<(String, Option<Uuid>, Uuid)> = crate::db::query_as(
        "SELECT COALESCE(br.backup_path, ''), v.host_id, br.vm_id
         FROM backup_records br JOIN vms v ON v.id = br.vm_id WHERE br.id = ?",
    )
    .bind(record_id)
    .fetch_optional(&state.pool)
    .await?;

    if let Some((path, host_id, vm_id)) = row {
        if !path.is_empty() {
            if crate::engine::atlas_vm::vm_is_atlas_backed(&state.pool, vm_id).await {
                // Atlas-backed: backup_path holds the Atlas backup id(s); delete via control plane.
                if let Ok(client) = crate::engine::atlas_bridge::require_client(&state.config) {
                    for bid in path.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        let _ = client.delete_backup(bid).await;
                    }
                }
            } else if let Some(hid) = host_id {
                // Local file backup: the host agent removes the file (guarded to the backup dir).
                if let Ok(addr) = host_agent_addr(&state.pool, hid).await {
                    if let Ok(mut client) = agent_client::connect(&addr).await {
                        let _ = agent_client::delete_backup(&mut client, &path).await;
                    }
                }
            }
        }
    }
    // Remove the catalog row regardless — the file may already be gone, and retention must converge.
    crate::db::query("DELETE FROM backup_records WHERE id = ?")
        .bind(record_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn vm_backup_restore(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let record_id: Uuid = msg.payload["backup_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("backup_id missing"))?;
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;

    // SAFETY: require the backup to belong to THIS vm and to be in a completed
    // state. Without the vm_id match, a caller could restore VM A's image onto
    // VM B — destroying B's disk and cross-loading another tenant's data. Without
    // the status filter, a partial/failed record's truncated image could be
    // converted over a live disk.
    let backup_path: String = crate::db::query_scalar(
        "SELECT backup_path FROM backup_records
         WHERE id = ? AND vm_id = ? AND status IN ('completed', 'succeeded')",
    )
    .bind(record_id)
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        anyhow::anyhow!(
            "backup record {} not found for VM {}, or not in a completed state",
            record_id,
            vm_id
        )
    })?;
    // Atlas-backed VMs restore from an Atlas backup (`backup_path` holds the
    // Atlas backup id(s)) via the control plane, provisioning a new volume.
    if crate::engine::atlas_vm::vm_is_atlas_backed(&state.pool, vm_id).await {
        let backup_id = backup_path
            .split(',')
            .map(str::trim)
            .find(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("backup record has no Atlas backup id"))?;
        let mode = msg.payload["mode"].as_str().unwrap_or("snapshot");
        crate::db::query("UPDATE backup_records SET restore_status = 'running' WHERE id = ?")
            .bind(record_id)
            .execute(&state.pool)
            .await?;
        let client = crate::engine::atlas_bridge::require_client(&state.config)?;
        // `restore_backup` only enqueues the Atlas restore job (202 Accepted);
        // treating that acceptance as `restore_status = 'completed'` (the
        // previous behavior) was a false success — a restore that later fails,
        // or is still copying data, was reported as done. Poll to terminal first.
        let result = async {
            let job = client.restore_backup(backup_id, None, mode).await?;
            let job = match job.job_id() {
                Some(jid) => {
                    client
                        .wait_for_job(jid, std::time::Duration::from_secs(1800))
                        .await?
                }
                None => job,
            };
            if job.state == "failed" {
                anyhow::bail!(
                    "Atlas restore failed: {}",
                    job.error.as_deref().unwrap_or("unknown error")
                );
            }
            if !job.is_terminal() {
                anyhow::bail!(
                    "Atlas restore job {} did not reach a terminal state in time",
                    job.job_id().unwrap_or("?")
                );
            }
            Ok::<_, anyhow::Error>(job)
        }
        .await;
        match result {
            Ok(job) => {
                crate::db::query("UPDATE backup_records SET restore_status = 'completed' WHERE id = ?")
                    .bind(record_id)
                    .execute(&state.pool)
                    .await?;
                state.emit_event(
                    "vm.backup.restore",
                    format!(
                        "Atlas restore job {} for VM {vm_id}",
                        job.job_id().unwrap_or("?")
                    ),
                );
            }
            Err(e) => {
                crate::db::query("UPDATE backup_records SET restore_status = 'failed' WHERE id = ?")
                    .bind(record_id)
                    .execute(&state.pool)
                    .await?;
                anyhow::bail!("atlas restore failed: {e}");
            }
        }
        update_task_progress(&state.pool, msg.task_id, 100, "atlas restore complete").await?;
        return Ok(());
    }

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;

    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    crate::db::query("UPDATE backup_records SET restore_status = 'running' WHERE id = ?")
        .bind(record_id)
        .execute(&state.pool)
        .await?;
    let resp = agent_client::restore_vm_backup(&mut client, &row.0, &backup_path).await?;
    if resp.ok {
        crate::db::query("UPDATE backup_records SET restore_status = 'completed' WHERE id = ?")
            .bind(record_id)
            .execute(&state.pool)
            .await?;
        state.emit_event(
            "vm.backup.restore",
            format!("Restored {} from backup", row.0),
        );
    } else {
        crate::db::query("UPDATE backup_records SET restore_status = 'failed' WHERE id = ?")
            .bind(record_id)
            .execute(&state.pool)
            .await?;
        anyhow::bail!("restore failed: {}", resp.message);
    }
    update_task_progress(&state.pool, msg.task_id, 100, "backup restored").await?;
    Ok(())
}

async fn templates_prefetch_missing(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id: Uuid = if let Some(s) = msg.payload["host_id"].as_str() {
        Uuid::parse_str(s)?
    } else {
        crate::db::query_scalar("SELECT id FROM hosts WHERE state = 'online' ORDER BY hostname LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no online hosts"))?
    };
    update_task_progress(
        &state.pool,
        msg.task_id,
        5,
        "Listing missing marketplace golden images",
    )
    .await?;
    let missing =
        crate::engine::template_readiness::list_missing_marketplace_images(&state.pool).await?;
    let targets: Vec<_> = missing.into_iter().filter(|m| m.auto_fetch).collect();
    if targets.is_empty() {
        update_task_progress(
            &state.pool,
            msg.task_id,
            100,
            "All auto-fetch images present",
        )
        .await?;
        return Ok(());
    }
    let total = targets.len();
    let mut errors = Vec::new();
    for (i, item) in targets.iter().enumerate() {
        let pct = 10 + ((i + 1) * 85 / total) as i16;
        update_task_progress(
            &state.pool,
            msg.task_id,
            pct,
            &format!("Fetching {}@{}", item.name, item.version),
        )
        .await?;
        if let Err(e) = crate::engine::template_image_fetch::ensure_template_disk(
            &state.pool,
            host_id,
            &item.source_disk,
            &item.name,
            &item.version,
        )
        .await
        {
            errors.push(format!("{}@{}: {e:#}", item.name, item.version));
        }
    }
    if !errors.is_empty() {
        anyhow::bail!(
            "{} of {} download(s) failed: {}",
            errors.len(),
            total,
            errors.join("; ")
        );
    }
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        &format!("Downloaded {total} golden image(s)"),
    )
    .await?;
    state.emit_event(
        "templates.prefetch",
        format!("Prefetched {total} marketplace golden image(s)"),
    );
    Ok(())
}

fn vm_id_from_payload(msg: &TaskMessage) -> Option<Uuid> {
    msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Max total attempts (initial + retries) before a task fails terminally.
const MAX_TASK_ATTEMPTS: i64 = 3;

/// Only connection-ESTABLISHMENT failures are retried: if we never reached the
/// agent, the operation provably did not run, so a retry is safe even for
/// destructive ops (vm.delete/migrate). Deliberately does NOT match mid-call
/// transport drops or app-level errors (ambiguous — the op may have partly run),
/// nor a handler panic (a bug, not transient).
fn is_transient_connect_error(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    const MARKERS: [&str; 7] = [
        "tcp connect error",
        "connection refused",
        "error trying to connect",
        "dns error",
        "no route to host",
        "failed to lookup address",
        "network is unreachable",
    ];
    MARKERS.iter().any(|m| e.contains(m))
}

/// SQLite `SQLITE_BUSY`/`SQLITE_LOCKED` (code 5/6) from contention on the shared
/// controller.db under concurrent writers (reconcile, channel_worker, webhook_worker,
/// per-request tracing, ...). The write never committed, so — unlike a mid-call
/// transport drop — there's no ambiguity about a destructive op partially applying;
/// it's always safe to retry. Without this, a task that loses the race against the
/// pool's busy_timeout gets stamped as a permanent VM-level error for a purely
/// infra/DB-contention hiccup.
fn is_transient_db_busy_error(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    // SQLite: busy/locked (codes 5/6). PostgreSQL: deadlock_detected (40P01) and serialization_failure (40001).
    e.contains("database is locked")
        || e.contains("code: 5")
        || e.contains("code: 6")
        || e.contains("deadlock detected")
        || e.contains("could not serialize access")
        || e.contains("40p01")
        || e.contains("40001")
}

async fn on_task_failure(state: &AppState, msg: &TaskMessage, err: &str) {
    // Count this attempt. Retry transient connect failures with backoff instead of
    // failing terminally, so a momentary agent restart / network blip during a
    // user op (vm.power/migrate) doesn't permanently fail it.
    let attempts: i64 = crate::db::query_scalar(
        "UPDATE tasks SET attempts = attempts + 1, updated_at = datetime('now')
         WHERE id = ? RETURNING attempts",
    )
    .bind(msg.task_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(MAX_TASK_ATTEMPTS);

    if attempts < MAX_TASK_ATTEMPTS
        && (is_transient_connect_error(err) || is_transient_db_busy_error(err))
    {
        // Reset to pending (clearing the owner) and re-publish after a linear
        // backoff. The failed run already released its scheduler key, so the
        // re-published task dispatches cleanly on its next arrival.
        let reset = crate::db::query(
            "UPDATE tasks SET status = 'pending', claimed_by = NULL, message = ?, updated_at = datetime('now')
             WHERE id = ? AND status = 'running'",
        )
        .bind(format!(
            "retry {attempts}/{MAX_TASK_ATTEMPTS} after transient error: {err}"
        ))
        .bind(msg.task_id)
        .execute(&state.pool)
        .await;
        if reset.is_ok() {
            let backoff = std::time::Duration::from_secs(5 * attempts as u64);
            let bus = state.task_bus.clone();
            let pool = state.pool.clone();
            let msg = msg.clone();
            tracing::warn!(task_id = %msg.task_id, op = %msg.operation,
                "transient failure; retry {attempts}/{MAX_TASK_ATTEMPTS} scheduled in {backoff:?}");
            tokio::spawn(async move {
                tokio::time::sleep(backoff).await;
                if let Err(e) = bus.publish("machina.tasks", &msg).await {
                    // Re-publish failed → no worker will ever pick this task back up,
                    // so this IS the terminal failure for it. Route through the same
                    // finalize path as every other terminal failure (mark_task_failed,
                    // set_vm_error, ha.recover compensation, webhook) — a bare status
                    // update here previously skipped revert_failed_ha_recovery, leaving
                    // an ha.recover task's premature host_id write un-reverted whenever
                    // the task bus itself (not the agent) was the thing that failed.
                    finalize_terminal_task_failure(
                        &pool,
                        &msg,
                        &format!("retry re-publish failed: {e}"),
                    )
                    .await;
                }
            });
            return;
        }
        // Fall through to terminal failure if the reset UPDATE itself failed.
    }

    finalize_terminal_task_failure(&state.pool, msg, err).await;
}

/// Terminal-failure bookkeeping shared by every path that gives up on a task for
/// good: the direct (non-retried) failure fallthrough above, the delayed
/// retry-exhaustion path where re-publishing itself fails, the publish-failure
/// branch in `enqueue_task` (task never even reached a worker), and the
/// startup orphan-task reaper (`db::ensure_bootstrap` / main.rs) for tasks left
/// 'running'/'pending' by a controller that crashed mid-task. Keeping these in
/// one place ensures operation-specific compensation (e.g. ha.recover's host_id
/// revert) always runs, regardless of which failure mode produced the terminal
/// state. Takes a bare pool rather than `&AppState` so it can be called from
/// call sites (enqueue, startup reap) that run before or without a full
/// `AppState`.
pub async fn finalize_terminal_task_failure(pool: &DbPool, msg: &TaskMessage, err: &str) {
    let _ = mark_task_failed(pool, msg.task_id, err).await;
    if let Some(vm_id) = vm_id_from_payload(msg) {
        let _ = vm_lifecycle::set_vm_error(pool, vm_id, err).await;
    }
    if msg.operation == "ha.recover" {
        if let Err(e) = revert_failed_ha_recovery(pool, msg).await {
            tracing::error!(task_id = %msg.task_id, "ha.recover: compensation after terminal failure also failed: {e:#}");
        }
    }
    crate::engine::webhooks::dispatch_webhooks(
        pool,
        "alert.task_failed",
        serde_json::json!({
            "severity": "error",
            "task_id": msg.task_id.to_string(),
            "operation": msg.operation,
            "error": err,
            "vm_id": msg.payload.get("vm_id"),
        }),
    )
    .await;
}

async fn host_validate_task(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    // The periodic re-check of failed hosts (engine/host_revalidate.rs) sets `quiet`: it must not
    // write into the join log of a host that joined long ago.
    let quiet = msg.payload["quiet"].as_bool().unwrap_or(false);
    update_task_progress(&state.pool, msg.task_id, 10, "running validation checklist").await?;
    if !quiet {
        crate::api::join_events::record_for_host(
            &state.pool,
            host_id,
            "info",
            "validate",
            "running the validation checklist against the agent",
        )
        .await;
    }
    let report = crate::engine::host_validate::validate_host(&state.pool, host_id).await?;
    crate::engine::host_validate::persist_validation(&state.pool, host_id, &report).await?;
    if !quiet {
        for c in &report.checks {
            crate::api::join_events::record_for_host(
                &state.pool,
                host_id,
                if c.passed { "ok" } else { "error" },
                "check",
                &format!("{}: {}", c.name, c.message),
            )
            .await;
        }
        crate::api::join_events::record_for_host(
            &state.pool,
            host_id,
            if report.ok { "ok" } else { "error" },
            "validate",
            if report.ok {
                "validation passed: the host is part of the fleet; collecting inventory"
            } else {
                "validation failed: see the failed checks above and the host page"
            },
        )
        .await;
    }
    if report.ok {
        // Networks and pools named on the enrollment token (no-op for other hosts).
        if let Err(e) = crate::engine::host_post_join::apply(state, host_id).await {
            tracing::warn!(host_id = %host_id, "post-join provisioning failed to queue: {e:#}");
        }
        if let Err(e) = enqueue_task(
            state,
            "host.inventory",
            serde_json::json!({ "host_id": host_id.to_string() }),
            Some("host"),
            Some(host_id),
            Some(host_id),
        )
        .await
        {
            tracing::warn!(host_id = %host_id, "host.inventory enqueue failed after validation: {}", e.message);
        }
    }
    let summary = if report.ok {
        "validation passed"
    } else {
        "validation failed — see host detail"
    };
    update_task_progress(&state.pool, msg.task_id, 100, summary).await?;
    Ok(())
}

async fn kubevirt_inventory_task(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    // Only accept an explicit cluster_id — a missing one falls through to the
    // first-cluster default below. (Previously fell back to payload["host_id"]
    // parsed as a cluster UUID, which would sync a nonexistent/wrong cluster.)
    let cluster_id: Uuid = msg.payload["cluster_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or(Uuid::nil());

    let cluster_id = if cluster_id.is_nil() {
        crate::db::query_scalar("SELECT id FROM clusters ORDER BY created_at LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no cluster configured"))?
    } else {
        cluster_id
    };

    let outcome = crate::engine::kubevirt_inventory::sync_cluster(state, cluster_id).await?;
    let progress_msg = if outcome.synced {
        "kubevirt inventory synced".to_string()
    } else {
        format!(
            "kubevirt inventory sync skipped: {}",
            outcome.reason.as_deref().unwrap_or("unknown reason")
        )
    };
    update_task_progress(&state.pool, msg.task_id, 100, &progress_msg).await?;
    Ok(())
}

async fn host_linux_package_upgrade(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id = msg
        .payload
        .get("host_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    update_task_progress(
        &state.pool,
        msg.task_id,
        20,
        "applying distro package upgrades on hypervisor",
    )
    .await?;
    let result = crate::engine::host_os::apply_linux_package_upgrade(
        &state.pool,
        &state.config,
        host_id,
        false,
    )
    .await?;
    let summary = result
        .get("stdout")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| "Package upgrade completed".into());
    update_task_progress(&state.pool, msg.task_id, 100, &summary).await?;
    Ok(())
}

async fn host_linux_reboot(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id = msg
        .payload
        .get("host_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    update_task_progress(&state.pool, msg.task_id, 30, "initiating hypervisor reboot").await?;
    crate::engine::host_os::reboot_linux_host(&state.pool, &state.config, host_id).await?;
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        "Reboot command sent — host may go offline briefly",
    )
    .await?;
    Ok(())
}

async fn host_enforcement_apply(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id = msg
        .payload
        .get("host_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("host_id missing or empty"))?;
    update_task_progress(
        &state.pool,
        msg.task_id,
        30,
        "reconciling native eBPF policies",
    )
    .await?;
    let results =
        crate::engine::bpf::policies::sync_hosts(&state.pool, Some(&[host_id.to_string()])).await?;
    let Some(r) = results.first() else {
        anyhow::bail!("host {host_id} is not online");
    };
    if r["ok"] != true {
        anyhow::bail!(
            "native policy sync failed on {host_id}: {}",
            r["error"].as_str().unwrap_or("unknown error")
        );
    }
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        &format!("Native eBPF policies reconciled on {host_id}"),
    )
    .await?;
    Ok(())
}

async fn host_agent_upgrade(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;
    let target = msg.payload["target_version"]
        .as_str()
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    update_task_progress(&state.pool, msg.task_id, 20, "upgrade queued").await?;

    // NOTE: the agent gRPC protocol has no "upgrade" RPC, so the controller has no
    // channel to actually trigger a machina-agent binary upgrade — this task can only
    // ask an operator to restart the agent out-of-band. `HeartbeatResponse` does carry
    // an `agent_version` field self-reported by the running binary (via
    // `env!("CARGO_PKG_VERSION")`), so once the agent is reachable we can compare what
    // it actually reports against `target` and only record a confirmed upgrade when
    // they match. If the agent isn't even answering, don't touch the recorded version
    // at all. If it answers but still reports the old version, that means the operator
    // hasn't restarted it yet — report that plainly instead of claiming success.
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let heartbeat_result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut client = agent_client::connect(&agent_addr).await?;
        agent_client::heartbeat(&mut client, &host_id.to_string()).await
    })
    .await;

    let reported_version = match heartbeat_result {
        Ok(Ok(resp)) => Some(resp.agent_version),
        _ => None,
    };

    let Some(reported_version) = reported_version else {
        // Return Err (not Ok) so this ends up 'failed', not 'completed' — `process_one`
        // unconditionally marks a handler that returns Ok(()) as 'completed', and a
        // fleet-upgrade orchestrator polling `status` (not parsing this message) must
        // not see a false success. The wording deliberately avoids
        // `is_transient_connect_error`'s markers (tcp/dns/refused/etc.) — this isn't a
        // connect-establishment failure the retry path should special-case; it should
        // just fail once, not retry pointlessly against a host that will keep
        // reporting the same non-confirmed state.
        let detail = format!(
            "agent unreachable at {agent_addr} — upgrade to {target} NOT recorded; \
             verify the host and retry"
        );
        update_task_progress(&state.pool, msg.task_id, 100, &detail).await?;
        return Err(anyhow::anyhow!(detail));
    };

    if reported_version != target {
        // Same reasoning as above: reachable-but-unconfirmed must not read as
        // 'completed' either.
        let detail = format!(
            "agent reachable but still running v{reported_version} (target v{target}) — \
             restart machina-agent on the host to complete the upgrade"
        );
        update_task_progress(&state.pool, msg.task_id, 100, &detail).await?;
        return Err(anyhow::anyhow!(detail));
    }

    crate::db::query("UPDATE hosts SET agent_version = ? WHERE id = ?")
        .bind(target)
        .bind(host_id)
        .execute(&state.pool)
        .await?;
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        &format!("upgrade to {target} confirmed — agent self-reports v{reported_version}"),
    )
    .await?;
    Ok(())
}

async fn storage_pool_provision(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let pool_id: Uuid = msg.payload["pool_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("pool_id missing"))?;
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;

    let row: (String, String, Option<String>) =
        crate::db::query_as("SELECT name, backend, path FROM storage_pools WHERE id = ?")
            .bind(pool_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("storage pool {} not found", pool_id))?;
    let path = row
        .2
        .ok_or_else(|| anyhow::anyhow!("storage pool path required"))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let res = agent_client::provision_storage_pool(&mut client, &row.0, &row.1, &path).await;
    // The join log of a joined host says whether what its token asked for really happened.
    crate::api::join_events::record_for_host(
        &state.pool,
        host_id,
        if res.is_ok() { "ok" } else { "error" },
        "provision",
        &match &res {
            Ok(_) => format!("storage pool '{}' is ready on this host", row.0),
            Err(e) => format!("storage pool '{}' could not be set up: {e:#}", row.0),
        },
    )
    .await;
    res?;
    crate::db::query("UPDATE storage_pools SET path = COALESCE(path, ?) WHERE id = ?")
        .bind(&path)
        .bind(pool_id)
        .execute(&state.pool)
        .await?;
    update_task_progress(
        &state.pool,
        msg.task_id,
        100,
        "storage pool provisioned on host",
    )
    .await?;
    state.emit_event(
        "storage.pool.provision",
        format!("Pool {} ({}) provisioned", row.0, row.1),
    );
    Ok(())
}

async fn network_provision(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let network_id: Uuid = msg.payload["network_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("network_id missing"))?;
    let host_id: Uuid = msg.payload["host_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("host_id missing"))?;

    let row: (String, String, Option<i32>, Option<String>) =
        crate::db::query_as("SELECT name, backend, vlan_id, bridge FROM networks WHERE id = ?")
            .bind(network_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("network {} not found", network_id))?;
    let bridge = row.3.unwrap_or_else(|| "virbr0".into());
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let res =
        agent_client::provision_network(&mut client, &row.0, &row.1, row.2.unwrap_or(0), &bridge)
            .await;
    crate::api::join_events::record_for_host(
        &state.pool,
        host_id,
        if res.is_ok() { "ok" } else { "error" },
        "provision",
        &match &res {
            Ok(_) => format!("network '{}' is ready on this host (bridge {bridge})", row.0),
            Err(e) => format!("network '{}' could not be set up: {e:#}", row.0),
        },
    )
    .await;
    res?;
    update_task_progress(&state.pool, msg.task_id, 100, "network provisioned").await?;
    Ok(())
}

async fn vm_disk_attach(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let disk_path = msg.payload["disk_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("disk_path missing"))?
        .to_string();
    let target_dev = msg.payload["target_dev"]
        .as_str()
        .unwrap_or("vdb")
        .to_string();

    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::attach_disk(&mut client, &row.0, &disk_path, &target_dev).await?;
    state.emit_event("vm.disk.attach", format!("Attached disk to {}", row.0));
    update_task_progress(&state.pool, msg.task_id, 100, "disk attached").await?;
    Ok(())
}

async fn vm_host_row(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<(String, Uuid)> {
    let row: Option<(String, Option<Uuid>)> =
        crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await?;
    let (name, host_id_opt) = row.ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = host_id_opt.ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    Ok((name, host_id))
}

async fn vm_disk_detach(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let target_dev = msg.payload["target_dev"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("target_dev missing"))?
        .to_string();
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::detach_disk(&mut client, &name, &target_dev).await?;
    state.emit_event(
        "vm.disk.detach",
        format!("Detached disk {target_dev} from {name}"),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "disk detached").await?;
    Ok(())
}

async fn vm_disk_resize(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let target_dev = msg.payload["target_dev"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("target_dev missing"))?
        .to_string();
    let size_gb = msg.payload["size_gb"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("size_gb missing"))?;
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::resize_disk(&mut client, &name, &target_dev, size_gb).await?;
    state.emit_event(
        "vm.disk.resize",
        format!("Resized disk {target_dev} on {name} to {size_gb} GiB"),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "disk resized").await?;
    Ok(())
}

async fn vm_nic_attach(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let network = msg.payload["network"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("network missing"))?
        .to_string();
    let model = msg.payload["model"]
        .as_str()
        .unwrap_or("virtio")
        .to_string();
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::attach_nic(&mut client, &name, &network, &model).await?;
    state.emit_event(
        "vm.nic.attach",
        format!("Attached NIC on {network} to {name}"),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "nic attached").await?;
    Ok(())
}

async fn vm_nic_detach(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let mac = msg.payload["mac_address"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("mac_address missing"))?
        .to_string();
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::detach_nic(&mut client, &name, &mac).await?;
    state.emit_event("vm.nic.detach", format!("Detached NIC {mac} from {name}"));
    update_task_progress(&state.pool, msg.task_id, 100, "nic detached").await?;
    Ok(())
}

async fn vm_autostart(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let enabled = msg.payload["enabled"].as_bool().unwrap_or(false);
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::set_autostart(&mut client, &name, enabled).await?;
    state.emit_event(
        "vm.autostart",
        format!(
            "Autostart {} for {name}",
            if enabled { "enabled" } else { "disabled" }
        ),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "autostart updated").await?;
    Ok(())
}

async fn vm_resize(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let kind = msg.payload["kind"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("kind missing"))?;
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    match kind {
        "vcpus" => {
            let count = msg.payload["count"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("count missing"))? as u32;
            agent_client::set_vcpus(&mut client, &name, count).await?;
            crate::db::query("UPDATE vms SET vcpus = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(count as i64)
                .bind(vm_id)
                .execute(&state.pool)
                .await?;
            state.emit_event("vm.resize", format!("Set {name} vCPUs to {count}"));
        }
        "memory" => {
            let memory_mb = msg.payload["memory_mb"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("memory_mb missing"))?;
            agent_client::set_memory(&mut client, &name, memory_mb).await?;
            crate::db::query("UPDATE vms SET memory_mib = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(memory_mb as i64)
                .bind(vm_id)
                .execute(&state.pool)
                .await?;
            state.emit_event("vm.resize", format!("Set {name} memory to {memory_mb} MiB"));
        }
        other => anyhow::bail!("unknown resize kind: {other}"),
    }
    update_task_progress(&state.pool, msg.task_id, 100, "resize complete").await?;
    Ok(())
}

/// Change a machine's instance type: clean shutdown if it is running, resize to the flavor, start again, record the flavor.
/// A guest that does not shut down within 2.5 minutes is left running and the task fails; nothing is changed in that case.
async fn vm_change_type(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    let flavor_id: Uuid = msg.payload["flavor_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("flavor_id missing"))?;
    let (new_vcpus, new_memory): (i32, i64) =
        crate::db::query_as("SELECT vcpus, memory_mib FROM flavors WHERE id = ?")
            .bind(flavor_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("flavor not found"))?;
    let (cur_vcpus, cur_memory): (i32, i64) =
        crate::db::query_as("SELECT vcpus, memory_mib FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_one(&state.pool)
            .await?;
    let (name, host_id) = vm_host_row(&state.pool, vm_id).await?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;

    let resize_needed = new_vcpus != cur_vcpus || new_memory != cur_memory;
    if resize_needed {
        let running = agent_client::list_vms(&mut client)
            .await?
            .vms
            .iter()
            .any(|v| v.name == name && v.state == "running");
        if running {
            update_task_progress(&state.pool, msg.task_id, 15, "shutting the machine down").await?;
            agent_client::vm_power(&mut client, &name, "shutdown", None).await?;
            let mut stopped = false;
            for _ in 0..50 {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                let still = agent_client::list_vms(&mut client)
                    .await?
                    .vms
                    .iter()
                    .any(|v| v.name == name && v.state == "running");
                if !still {
                    stopped = true;
                    break;
                }
            }
            anyhow::ensure!(
                stopped,
                "the guest did not shut down within 2.5 minutes; nothing was changed"
            );
        }
        update_task_progress(&state.pool, msg.task_id, 50, "resizing").await?;
        agent_client::set_vcpus(&mut client, &name, new_vcpus as u32).await?;
        agent_client::set_memory(&mut client, &name, new_memory as u64).await?;
        crate::db::query("UPDATE vms SET vcpus = ?, memory_mib = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(new_vcpus)
            .bind(new_memory)
            .bind(vm_id)
            .execute(&state.pool)
            .await?;
        if running {
            update_task_progress(&state.pool, msg.task_id, 80, "starting").await?;
            agent_client::vm_power(&mut client, &name, "start", None).await?;
        }
    }
    crate::db::query("UPDATE vms SET flavor_id = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(flavor_id)
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    state.emit_event(
        "vm.change_type",
        format!("{name} is now {new_vcpus} vCPU / {new_memory} MiB"),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "instance type changed").await?;
    Ok(())
}

async fn vm_guest_tools_install(state: &AppState, msg: &TaskMessage) -> anyhow::Result<()> {
    let vm_id: Uuid = msg.payload["vm_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| anyhow::anyhow!("vm_id missing"))?;
    // Clear sticky last_error from a prior failed attach so success doesn't leave
    // the VM detail banner stuck on the old failure.
    vm_lifecycle::set_vm_phase_clear_error(&state.pool, vm_id, vm_lifecycle::PHASE_IDLE).await?;
    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vm {} not found", vm_id))?;
    let host_id = row
        .1
        .ok_or_else(|| anyhow::anyhow!("vm {} has no host", vm_id))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    agent_client::install_guest_tools(&mut client, &row.0).await?;
    crate::engine::vm_health::sync_guest_tools(&state.pool, vm_id, &row.0, host_id).await;
    crate::db::query(
        "UPDATE vms SET guest_tools_status = 'installed', last_error = '', updated_at = datetime('now') WHERE id = ?",
    )
    .bind(vm_id)
    .execute(&state.pool)
    .await?;
    state.emit_event(
        "vm.guest_tools",
        format!("Guest tools channel attached for {}", row.0),
    );
    update_task_progress(&state.pool, msg.task_id, 100, "guest tools install queued").await?;
    Ok(())
}

async fn run_incremental_backup(
    pool: &DbPool,
    vm_id: Uuid,
    vm_name: &str,
    dest: &str,
    agent_addr: &str,
) -> anyhow::Result<machina_agent::pb::BackupVmResponse> {
    let prior: Option<String> = crate::db::query_scalar(
        "SELECT backup_path FROM backup_records
         WHERE vm_id = ? AND status = 'completed' AND backup_path != ''
         ORDER BY datetime(created_at) DESC LIMIT 1",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await?;
    let mut client = agent_client::connect(agent_addr).await?;
    let resp = agent_client::backup_vm(&mut client, vm_name, dest).await?;
    if resp.ok {
        if let Some(base) = prior.filter(|p| std::path::Path::new(p).exists()) {
            let base_owned = base.clone();
            let path_owned = resp.path.clone();
            let rebase_result = tokio::task::spawn_blocking(move || {
                std::process::Command::new("qemu-img")
                    .args(["rebase", "-u", "-b", &base_owned, &path_owned])
                    .output()
            })
            .await;
            match rebase_result {
                Ok(Err(e)) => tracing::warn!("qemu-img rebase unavailable: {e}"),
                Ok(Ok(out)) if !out.status.success() => {
                    tracing::warn!(
                        "qemu-img rebase failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                }
                Err(e) => tracing::warn!("qemu-img rebase task panicked: {e}"),
                _ => {}
            }
        }
    }
    Ok(resp)
}

#[cfg(test)]
mod scheduler_tests {
    use super::{resource_key, Scheduler, TaskMessage};
    use serde_json::json;
    use uuid::Uuid;

    fn msg(op: &str, payload: serde_json::Value) -> TaskMessage {
        TaskMessage {
            task_id: Uuid::new_v4(),
            operation: op.into(),
            payload,
        }
    }

    #[test]
    fn resource_key_prefers_vm_then_host_then_cluster() {
        assert_eq!(
            resource_key(&msg("vm.power", json!({"vm_id": "abc"}))),
            "vm_id:abc"
        );
        assert_eq!(
            resource_key(&msg("host.inventory", json!({"host_id": "h1"}))),
            "host_id:h1"
        );
        assert_eq!(
            resource_key(&msg("kubevirt.inventory", json!({"cluster_id": "c1"}))),
            "cluster_id:c1"
        );
        // Empty id is ignored; falls through to a per-task unique key.
        let m = msg("vm.power", json!({"vm_id": ""}));
        assert_eq!(resource_key(&m), format!("task:{}", m.task_id));
    }

    #[test]
    fn same_key_serializes_fifo_different_keys_parallel() {
        let mut s = Scheduler::default();
        let a1 = msg("vm.power", json!({"vm_id": "A"}));
        let a2 = msg("vm.stop", json!({"vm_id": "A"}));
        let b1 = msg("vm.power", json!({"vm_id": "B"}));

        // First task for A dispatches immediately.
        assert!(s.on_arrival(a1.clone()).is_some());
        // Second task for A is held (A active).
        assert!(s.on_arrival(a2.clone()).is_none());
        // A different resource B runs in parallel.
        assert!(s.on_arrival(b1.clone()).is_some());

        // When A's first task finishes, the queued A task dispatches next (FIFO).
        let next = s
            .on_complete("vm_id:A")
            .expect("queued A task should dispatch");
        assert_eq!(next.task_id, a2.task_id);
        // A still active (running a2); completing again idles it.
        assert!(s.on_complete("vm_id:A").is_none());
        // B idles independently.
        assert!(s.on_complete("vm_id:B").is_none());
    }

    #[test]
    fn completing_unknown_key_is_noop() {
        let mut s = Scheduler::default();
        assert!(s.on_complete("vm_id:ghost").is_none());
    }

    #[test]
    fn transient_classifier_matches_only_connect_failures() {
        use super::is_transient_connect_error;
        // Connection-establishment failures → retry (op never ran).
        assert!(is_transient_connect_error(
            "connect to agent: transport error: tcp connect error: Connection refused (os error 111)"
        ));
        assert!(is_transient_connect_error(
            "error trying to connect: dns error"
        ));
        assert!(is_transient_connect_error("Network is unreachable"));
        // App-level / ambiguous failures → do NOT retry (op may have run).
        assert!(!is_transient_connect_error(
            "status: NotFound, message: domain 'x' not found"
        ));
        assert!(!is_transient_connect_error("task handler panicked"));
        assert!(!is_transient_connect_error("invalid memory spec"));
        assert!(!is_transient_connect_error(
            "status: DeadlineExceeded, message: timed out"
        ));
    }

    #[test]
    fn db_busy_classifier_matches_sqlite_lock_errors() {
        use super::is_transient_db_busy_error;
        assert!(is_transient_db_busy_error(
            "error returned from database: (code: 5) database is locked: (code: 5) database is locked"
        ));
        assert!(is_transient_db_busy_error("database is locked"));
        assert!(!is_transient_db_busy_error(
            "status: NotFound, message: domain 'x' not found"
        ));
        assert!(!is_transient_db_busy_error("task handler panicked"));
    }

    #[test]
    fn deep_queue_drains_in_order() {
        let mut s = Scheduler::default();
        let first = msg("vm.power", json!({"vm_id": "A"}));
        assert!(s.on_arrival(first).is_some());
        let queued: Vec<_> = (0..3)
            .map(|_| msg("vm.power", json!({"vm_id": "A"})))
            .collect();
        for m in &queued {
            assert!(s.on_arrival(m.clone()).is_none());
        }
        for expected in &queued {
            let next = s.on_complete("vm_id:A").expect("should drain queued task");
            assert_eq!(next.task_id, expected.task_id);
        }
        // Backlog empty → next completion idles the key.
        assert!(s.on_complete("vm_id:A").is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scans_count_until_cleared() {
        let host = Uuid::new_v4();
        assert_eq!(record_empty_scan(host), 1);
        assert_eq!(record_empty_scan(host), 2);
        assert_eq!(record_empty_scan(host), EMPTY_SCANS_BEFORE_PRUNE);
        clear_empty_scans(host);
        assert_eq!(record_empty_scan(host), 1);
    }
}
