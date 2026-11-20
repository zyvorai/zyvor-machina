// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Extension, Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::stream::{self, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use virt::connect::Connect;

use machina_core::libvirt::domain::UndefineOptions;
use machina_core::libvirt::resize::{CpuTuneInfo, MemTuneInfo};
use machina_core::libvirt::{block_jobs, clone, create, device, domain, resize};
use machina_core::{
    audit, kubevirt_bundle_from_libvirt_vm, AttachDiskRequest, AuditEvent, CloneVmRequest,
    CreateVmRequest, KubeVirtBundle, KubeVirtConfig, LibvirtError, LibvirtManager, MachinaConfig,
    RenameVmRequest, VmCreateBackend, VmDetails, VmInfo,
};

use crate::auth::{effective_linux_user, require_destroy_vm, require_write, RequestActor};
use crate::conn_query::{connection_label, spawn_libvirt_actor, ConnQuery};
use crate::error::{ok_json, AppError, Xml};
use crate::job_registry::JobRegistry;
use crate::kubevirt_exec;
use crate::routes::events::{EventBus, MachinaEvent};
use crate::vm_events;
use std::sync::Arc;

/// Bounded queue between host log producers and the SSE bridge (backpressure; avoids unbounded RAM).
const CREATE_LOG_STD_CAP: usize = 65_536;
const CREATE_LOG_SSE_CAP: usize = 8192;

fn truncate_audit_result(s: impl AsRef<str>) -> String {
    const MAX_CHARS: usize = 480;
    let s = s.as_ref();
    if s.chars().nth(MAX_CHARS).is_none() {
        s.to_string()
    } else {
        s.chars()
            .take(MAX_CHARS.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn validate_create_vm_payload(req: &CreateVmRequest) -> Result<(), AppError> {
    machina_core::validate::validate_create_backend_override(&req.create_backend)?;
    machina_core::validate::validate_template_disk_mode(&req.template_disk_mode)?;
    machina_core::validate::validate_create_vm_disk_image_builders(req)?;
    Ok(())
}

fn log_audit(action: &str, target: &str, result: &str) {
    log_audit_with_actor(None, action, target, result);
}

fn log_audit_with_actor(actor: Option<&str>, action: &str, target: &str, result: &str) {
    let event = AuditEvent {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        action: action.to_string(),
        target: target.to_string(),
        result: result.to_string(),
        actor: actor.unwrap_or("").to_string(),
    };
    audit::write_audit_event(&event);
}

async fn list_vms(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<Vec<VmInfo>>, AppError> {
    if crate::conn_query::impersonation_prefers_session_only(&actor) {
        let conn_q =
            crate::conn_query::apply_impersonation_session_default(&actor, ConnQuery::default());
        let vms = spawn_libvirt_actor(manager, Some(&actor), conn_q, |conn| {
            machina_core::libvirt::domain::list_vms(conn)
        })
        .await?;
        return Ok(Json(super::fluxvm::merge_into(vms).await));
    }
    let result = tokio::task::spawn_blocking(move || manager.list_all_vms())
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;
    Ok(Json(super::fluxvm::merge_into(result?).await))
}

/// `?backend=fluxvm`, or no `backend` given while `[fluxvm]` is enabled and the
/// name exists in FluxVM but not in libvirt (keeps CLI/scripts working).
async fn targets_fluxvm(
    manager: &LibvirtManager,
    actor: &RequestActor,
    conn_q: &ConnQuery,
    name: &str,
) -> bool {
    if conn_q.is_fluxvm() {
        return true;
    }
    if conn_q.backend.is_some() || !MachinaConfig::load().fluxvm.enabled {
        return false;
    }
    let n = name.to_string();
    let in_libvirt = spawn_libvirt_actor(manager.clone(), Some(actor), conn_q.clone(), move |c| {
        Ok(domain::lookup_domain(c, &n).is_ok())
    })
    .await
    .unwrap_or(true);
    if in_libvirt {
        return false;
    }
    match super::fluxvm::client() {
        Ok(c) => c.resolve(name).await.is_ok(),
        Err(_) => false,
    }
}

async fn fluxvm_action(name: &str, verb: &str) -> Result<(), AppError> {
    super::fluxvm::client()?.action(name, verb).await?;
    Ok(())
}

async fn get_vm_details(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<VmDetails>, AppError> {
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        let rec = super::fluxvm::client()?.get(&name).await?;
        return Ok(Json(rec.to_vm_details()));
    }
    let dual = manager.dual_enabled();
    let conn_q = crate::conn_query::apply_impersonation_session_default(&actor, conn_q);
    let target = manager.resolve_query(conn_q.connection.as_deref());
    let label = connection_label(dual, target);
    let name2 = name.clone();
    let mut d = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::get_vm_details(conn, &name2)
    })
    .await?;
    d.libvirt_connection = label;
    Ok(Json(d))
}

async fn get_vm_xml(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Xml, AppError> {
    let name2 = name.clone();
    let xml = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::get_vm_xml(conn, &name2)
    })
    .await?;
    Ok(Xml(xml))
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct KubeVirtBundleParams {
    #[serde(default)]
    namespace: Option<String>,
    /// Kubernetes VM metadata.name (defaults from libvirt name).
    #[serde(default)]
    k8s_vm_name: Option<String>,
    #[serde(default)]
    datavolume_name: Option<String>,
    #[serde(default)]
    storage_gi: Option<u32>,
    #[serde(default)]
    storage_class: Option<String>,
    /// When false, omit virtio-win `containerDisk` CDROM.
    #[serde(default = "default_true")]
    include_virtio_cdrom: bool,
    /// `system` / `session` when `[libvirt] dual_connection` is enabled.
    #[serde(default)]
    connection: Option<String>,
}

async fn kubevirt_bundle_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    State(manager): State<LibvirtManager>,
    Query(q): Query<KubeVirtBundleParams>,
) -> Result<Json<KubeVirtBundle>, AppError> {
    let cfg = MachinaConfig::load();
    let name2 = name.clone();
    let conn_q = ConnQuery {
        connection: q.connection.clone(),
        backend: None,
    };
    let details = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |c| {
        domain::get_vm_details(c, &name2)
    })
    .await?;

    let bundle = kubevirt_bundle_from_libvirt_vm(
        &details,
        &name,
        &cfg.kubevirt,
        q.namespace.as_deref(),
        q.k8s_vm_name.as_deref(),
        q.datavolume_name.as_deref(),
        q.storage_gi,
        q.storage_class.as_deref(),
        q.include_virtio_cdrom,
    )?;
    log_audit("kubevirt-bundle", &name, "ok");
    Ok(Json(bundle))
}

fn kubevirt_bundle_for_deploy(
    manager: &LibvirtManager,
    libvirt_name: &str,
    kv: &KubeVirtConfig,
    p: &KubeVirtBundleParams,
) -> Result<KubeVirtBundle, AppError> {
    let t = manager.resolve_query(p.connection.as_deref());
    let details = manager.with_conn_target(t, |c| domain::get_vm_details(c, libvirt_name))?;
    Ok(kubevirt_bundle_from_libvirt_vm(
        &details,
        libvirt_name,
        kv,
        p.namespace.as_deref(),
        p.k8s_vm_name.as_deref(),
        p.datavolume_name.as_deref(),
        p.storage_gi,
        p.storage_class.as_deref(),
        p.include_virtio_cdrom,
    )?)
}

fn emit_kubevirt(bus: &Arc<EventBus>, kind: &str, target: &str, status: &str, message: &str) {
    let mut ev = MachinaEvent::now(kind, target, status);
    ev.message = message.chars().take(512).collect();
    bus.emit(ev);
}

async fn kubevirt_apply_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    State(manager): State<LibvirtManager>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(params): Json<KubeVirtBundleParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let kv_for_block = kv.clone();
    let mgr = manager.clone();
    let n = name.clone();
    let bundle = tokio::task::spawn_blocking(move || {
        kubevirt_bundle_for_deploy(&mgr, &n, &kv_for_block, &params)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let tmp = std::env::temp_dir().join(format!(
        "machina-kubevirt-{}-{}.yaml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &bundle.yaml).map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "Failed to write temp kubevirt yaml: {e}"
        )))
    })?;
    let path = tmp.clone();
    let (code, stdout, stderr) = kubevirt_exec::kubectl_apply_yaml(&kv, &path)
        .await
        .map_err(AppError::from)?;
    let _ = std::fs::remove_file(&tmp);
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-apply", &name, audit);
    emit_kubevirt(
        &bus,
        "kubevirt.apply",
        &name,
        audit,
        &bundle.virtual_machine_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

async fn kubevirt_upload_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    State(manager): State<LibvirtManager>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(params): Json<KubeVirtBundleParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let kv_for_block = kv.clone();
    let mgr = manager.clone();
    let n = name.clone();
    let bundle = tokio::task::spawn_blocking(move || {
        kubevirt_bundle_for_deploy(&mgr, &n, &kv_for_block, &params)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let img = bundle.libvirt_root_disk.clone();
    let (code, stdout, stderr) = kubevirt_exec::virtctl_image_upload_disk(
        &kv,
        &bundle.datavolume_name,
        bundle.upload_size_gi,
        &img,
        &bundle.namespace,
    )
    .await
    .map_err(AppError::from)?;
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-upload", &name, audit);
    emit_kubevirt(
        &bus,
        "kubevirt.upload",
        &name,
        audit,
        &bundle.datavolume_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

async fn kubevirt_start_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    State(manager): State<LibvirtManager>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(params): Json<KubeVirtBundleParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let kv_for_block = kv.clone();
    let mgr = manager.clone();
    let n = name.clone();
    let bundle = tokio::task::spawn_blocking(move || {
        kubevirt_bundle_for_deploy(&mgr, &n, &kv_for_block, &params)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    let (code, stdout, stderr) =
        kubevirt_exec::virtctl_start_vm(&kv, &bundle.virtual_machine_name, &bundle.namespace)
            .await
            .map_err(AppError::from)?;
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-start", &name, audit);
    emit_kubevirt(
        &bus,
        "kubevirt.start",
        &name,
        audit,
        &bundle.virtual_machine_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

async fn start_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Role-only check bypassed API-token scope (an operator-role token scoped away
    // from vms:write could still start any VM); use require_write like every other
    // mutating handler in this file so both role and token scope are enforced.
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "start").await?;
        log_audit("start", &name, "ok");
        vm_events::emit_vm_started(&name);
        super::bpf::notify_vm_lifecycle();
        return Ok(ok_json("started", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::start_vm(conn, &name2)
    })
    .await?;
    log_audit("start", &name, "ok");
    vm_events::emit_vm_started(&name);
    super::bpf::notify_vm_lifecycle();
    Ok(ok_json("started", &name))
}

async fn stop_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "stop").await?;
        log_audit("stop", &name, "ok");
        vm_events::emit_vm_stopped(&name);
        super::bpf::notify_vm_lifecycle();
        return Ok(ok_json("stopped", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::stop_vm(conn, &name2)
    })
    .await?;
    log_audit("stop", &name, "ok");
    vm_events::emit_vm_stopped(&name);
    super::bpf::notify_vm_lifecycle();
    Ok(ok_json("stopped", &name))
}

async fn shutdown_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "stop").await?;
        log_audit("shutdown", &name, "ok");
        vm_events::emit_vm_shutdown(&name);
        super::bpf::notify_vm_lifecycle();
        return Ok(ok_json("shutting down", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::shutdown_vm(conn, &name2)
    })
    .await?;
    log_audit("shutdown", &name, "ok");
    vm_events::emit_vm_shutdown(&name);
    Ok(ok_json("shutting down", &name))
}

async fn reboot_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "restart").await?;
        vm_events::emit_vm_reboot(&name);
        return Ok(ok_json("rebooting", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::reboot_vm(conn, &name2)
    })
    .await?;
    vm_events::emit_vm_reboot(&name);
    Ok(ok_json("rebooting", &name))
}

async fn pause_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "pause").await?;
        vm_events::emit_vm_paused(&name);
        return Ok(ok_json("paused", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::pause_vm(conn, &name2)
    })
    .await?;
    vm_events::emit_vm_paused(&name);
    Ok(ok_json("paused", &name))
}

async fn resume_vm(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        fluxvm_action(&name, "resume").await?;
        vm_events::emit_vm_resumed(&name);
        return Ok(ok_json("resumed", &name));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::resume_vm(conn, &name2)
    })
    .await?;
    vm_events::emit_vm_resumed(&name);
    Ok(ok_json("resumed", &name))
}

#[derive(Deserialize, Default)]
struct DeleteVmQuery {
    #[serde(default)]
    undefine_managed_save: bool,
    #[serde(default)]
    undefine_snapshots_metadata: bool,
    #[serde(default)]
    undefine_nvram: bool,
    #[serde(default)]
    undefine_keep_nvram: bool,
    #[serde(default)]
    undefine_checkpoints_metadata: bool,
    #[serde(default)]
    undefine_tpm: bool,
    #[serde(default)]
    undefine_keep_tpm: bool,
    /// Also delete backing disk image files from the host filesystem.
    #[serde(default)]
    delete_disks: bool,
}

async fn delete_vm_handler(
    Extension(actor): Extension<RequestActor>,
    State(manager): State<LibvirtManager>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Query(q): Query<DeleteVmQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_destroy_vm(&actor)?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        super::fluxvm::client()?.delete(&name).await?;
        log_audit_with_actor(Some(&actor.username), "delete", &name, "ok");
        vm_events::emit_vm_deleted(&name);
        super::bpf::notify_vm_lifecycle();
        return Ok(ok_json("deleted", &name));
    }
    let opts = UndefineOptions {
        managed_save: q.undefine_managed_save,
        snapshots_metadata: q.undefine_snapshots_metadata,
        nvram: q.undefine_nvram,
        keep_nvram: q.undefine_keep_nvram,
        checkpoints_metadata: q.undefine_checkpoints_metadata,
        tpm: q.undefine_tpm,
        keep_tpm: q.undefine_keep_tpm,
        delete_disks: q.delete_disks,
    };
    let name2 = name.clone();
    let leaked_network_disks = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::delete_vm_with_options(conn, &name2, &opts)
    })
    .await?;
    log_audit_with_actor(Some(&actor.username), "delete", &name, "ok");
    vm_events::emit_vm_deleted(&name);
    super::bpf::notify_vm_lifecycle();
    if leaked_network_disks.is_empty() {
        Ok(ok_json("deleted", &name))
    } else {
        // `delete_disks: true` only ever unlinks local files; a network-backed
        // (e.g. Ceph/RBD) disk isn't a file to unlink, so its backing volume is
        // still there. A bare "deleted" success here previously left the caller
        // with no idea the volume needs its own cleanup.
        Ok(Json(serde_json::json!({
            "status": "deleted",
            "name": name,
            "warning": format!(
                "target(s) {} were network-backed (e.g. Ceph/RBD) and not deleted — \
                 their backing volume still exists and needs separate cleanup",
                leaked_network_disks.join(", ")
            ),
        })))
    }
}

#[derive(Deserialize)]
struct BlockCommitBody {
    disk: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    top: Option<String>,
    #[serde(default)]
    bandwidth: u64,
    #[serde(default)]
    shallow: bool,
    #[serde(default)]
    delete: bool,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    relative: bool,
    #[serde(default)]
    bandwidth_bytes: bool,
}

async fn block_commit_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<BlockCommitBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let flags = block_jobs::block_commit_flags(
        req.shallow,
        req.delete,
        req.active,
        req.relative,
        req.bandwidth_bytes,
    );
    let disk = req.disk.clone();
    let base = req.base.clone();
    let top = req.top.clone();
    let bandwidth = req.bandwidth;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        block_jobs::block_commit(
            conn,
            &name2,
            &disk,
            base.as_deref(),
            top.as_deref(),
            bandwidth,
            flags,
        )
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "block_commit_started", "name": name }),
    ))
}

#[derive(Deserialize)]
struct BlockPullBody {
    disk: String,
    #[serde(default)]
    bandwidth: u64,
    #[serde(default)]
    bandwidth_bytes: bool,
}

async fn block_pull_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<BlockPullBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let flags = block_jobs::block_pull_flags(req.bandwidth_bytes);
    let disk = req.disk.clone();
    let bandwidth = req.bandwidth;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        block_jobs::block_pull(conn, &name2, &disk, bandwidth, flags)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "block_pull_started", "name": name }),
    ))
}

#[derive(Deserialize)]
struct BlockJobQuery {
    disk: String,
    #[serde(default)]
    bandwidth_bytes: bool,
}

async fn block_job_info_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Query(q): Query<BlockJobQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let disk = q.disk.clone();
    let flags = block_jobs::block_job_info_flags(q.bandwidth_bytes);
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        block_jobs::block_job_info(conn, &name2, &disk, flags)
    })
    .await?;
    Ok(Json(serde_json::json!({ "name": name, "job": result })))
}

#[derive(Deserialize)]
struct BlockJobAbortBody {
    disk: String,
    #[serde(default)]
    r#async: bool,
    #[serde(default)]
    pivot: bool,
}

async fn block_job_abort_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<BlockJobAbortBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let flags = block_jobs::block_job_abort_flags(req.r#async, req.pivot);
    let disk = req.disk.clone();
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        block_jobs::block_job_abort(conn, &name2, &disk, flags)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "block_job_abort", "name": name }),
    ))
}

async fn set_memtune_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<MemTuneInfo>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let req2 = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        resize::set_memtune_kb(conn, &name2, &req2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "memtune_updated", "name": name }),
    ))
}

#[derive(Deserialize)]
struct SchedulerTuneBody {
    #[serde(default)]
    cpu_shares: Option<u64>,
    #[serde(default)]
    vcpu_period: Option<u64>,
    #[serde(default)]
    vcpu_quota: Option<i64>,
}

async fn set_scheduler_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<SchedulerTuneBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let cpu_shares = req.cpu_shares;
    let vcpu_period = req.vcpu_period;
    let vcpu_quota = req.vcpu_quota;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        resize::set_cpu_scheduler_partial(conn, &name2, cpu_shares, vcpu_period, vcpu_quota)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "scheduler_updated", "name": name }),
    ))
}

#[derive(Deserialize)]
struct PinVcpuBody {
    cpus: Vec<bool>,
}

async fn pin_vcpu_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, vcpu)): Path<(String, u32)>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<PinVcpuBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let cpus = req.cpus.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        resize::pin_vcpu(conn, &name2, vcpu, &cpus)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "vcpu_pinned", "name": name, "vcpu": vcpu }),
    ))
}

async fn set_autostart(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, enabled)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let autostart = enabled == "true" || enabled == "1";
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::set_autostart(conn, &name2, autostart)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "autostart": autostart }),
    ))
}

async fn clone_vm_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<CloneVmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Every sibling mutating handler in this file gates on this; this one
    // didn't, letting a read-only-role session clone (and copy the full disk
    // of) any VM.
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Cloning a VM requires the operator or admin role.".into(),
        )
        .into());
    }
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        super::fluxvm::client()?
            .clone_vm(&name, &req.new_name)
            .await?;
        log_audit("clone", &format!("{name} -> {}", req.new_name), "ok");
        return Ok(Json(serde_json::json!({
            "status": "cloned", "source": name, "clone": req.new_name, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let new_name = req.new_name.clone();
    let mode = if req.clone_mode.trim().is_empty() {
        "linked".to_string()
    } else {
        req.clone_mode.clone()
    };
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let _uuid = clone::clone_vm_with_disk(conn, &name2, &new_name, &mode)?;
        Ok(())
    })
    .await?;
    log_audit("clone", &format!("{name} -> {}", req.new_name), "ok");
    Ok(Json(
        serde_json::json!({ "status": "cloned", "source": name, "clone": req.new_name }),
    ))
}

fn apply_impersonation_create_defaults(
    cfg: &MachinaConfig,
    actor: &RequestActor,
    req: &mut CreateVmRequest,
) {
    if !cfg.auth.run_as_user.prefer_session_libvirt_on_impersonation {
        return;
    }
    if !cfg.auth.run_as_user.impersonation_active() || !cfg.libvirt.dual_connection {
        return;
    }
    if effective_linux_user(actor).is_none() {
        return;
    }
    if req.libvirt_connection.trim().is_empty() {
        req.libvirt_connection = "session".into();
    }
}

fn libvirt_uri_for_create(cfg: &MachinaConfig, req: &CreateVmRequest) -> String {
    if cfg.libvirt.dual_connection {
        if req.libvirt_connection.trim() == "session" {
            "qemu:///session".into()
        } else {
            "qemu:///system".into()
        }
    } else {
        cfg.libvirt.uri.clone()
    }
}

fn ensure_session_libvirt_identity(
    actor: &RequestActor,
    cfg: &MachinaConfig,
    req: &CreateVmRequest,
) -> Result<(), AppError> {
    let wants_session = if cfg.libvirt.dual_connection {
        req.libvirt_connection.trim() == "session"
    } else {
        cfg.libvirt.uri.trim() == "qemu:///session"
    };
    if !wants_session || !cfg.auth.oidc.require_local_user_for_session_libvirt {
        return Ok(());
    }
    if effective_linux_user(actor).is_some() {
        return Ok(());
    }
    Err(AppError::from(LibvirtError::Forbidden(
        "qemu:///session VM creation requires a mapped local Linux user for this identity".into(),
    )))
}

fn wants_fluxvm_create(req: &CreateVmRequest) -> bool {
    machina_core::fluxvm::is_fluxvm(Some(req.backend.as_str()))
}

/// Create on `fluxvm-api`; refuses a name already used by either backend.
async fn create_fluxvm_vm(
    manager: &LibvirtManager,
    actor: &RequestActor,
    req: &CreateVmRequest,
) -> Result<(), LibvirtError> {
    let name = req.name.clone();
    let n = name.clone();
    let clash = spawn_libvirt_actor(
        manager.clone(),
        Some(actor),
        ConnQuery::default(),
        move |c| Ok(domain::lookup_domain(c, &n).is_ok()),
    )
    .await
    .unwrap_or(false);
    if clash {
        return Err(LibvirtError::Invalid(format!(
            "a libvirt VM named '{name}' already exists"
        )));
    }
    let c = machina_core::fluxvm::FluxvmClient::from_config(&MachinaConfig::load().fluxvm)?;
    if c.resolve(&name).await.is_ok() {
        return Err(LibvirtError::Invalid(format!(
            "a FluxVM VM named '{name}' already exists"
        )));
    }
    match c.create(req).await {
        Ok(_) => {
            log_audit_with_actor(Some(&actor.username), "create", &name, "ok (fluxvm)");
            super::bpf::notify_vm_lifecycle();
            Ok(())
        }
        Err(e) => {
            log_audit_with_actor(
                Some(&actor.username),
                "create",
                &name,
                &truncate_audit_result(format!("fail (fluxvm): {e}")),
            );
            Err(e)
        }
    }
}

async fn create_vm_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(mut req): Json<CreateVmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if wants_fluxvm_create(&req) {
        create_fluxvm_vm(&manager, &actor, &req).await?;
        return Ok(ok_json("created", &req.name));
    }
    validate_create_vm_payload(&req)?;
    let name = req.name.clone();
    let cfg = MachinaConfig::load();
    apply_impersonation_create_defaults(&cfg, &actor, &mut req);
    ensure_session_libvirt_identity(&actor, &cfg, &req)?;
    let backend = match req.create_backend.trim() {
        "virt_install" => VmCreateBackend::VirtInstall,
        "libvirt_xml" => VmCreateBackend::LibvirtXml,
        _ => cfg.libvirt.create_backend,
    };
    let libvirt_uri = libvirt_uri_for_create(&cfg, &req);
    let libvirt_cfg = cfg.libvirt.clone();
    let join_res = tokio::task::spawn_blocking(move || {
        // IMPORTANT: do NOT use LibvirtManager::with_conn here.
        // VM create (virt-install --wait -1, mkosi, virt-builder) can be long-running and would
        // hold the manager's global libvirt mutex, blocking the dashboard.
        let conn = Connect::open(Some(&libvirt_uri)).map_err(|e| {
            LibvirtError::Connection(format!(
                "Failed to connect to libvirt ({}): {e}",
                libvirt_uri
            ))
        })?;
        create::create_vm(&conn, &req, backend, &libvirt_uri, &libvirt_cfg, None)
    })
    .await;

    match join_res {
        Ok(Ok(())) => {
            log_audit("create", &name, "ok");
            Ok(ok_json("created", &name))
        }
        Ok(Err(e)) => {
            log_audit(
                "create",
                &name,
                &truncate_audit_result(format!("fail: {e}")),
            );
            Err(AppError::from(e))
        }
        Err(join_err) => {
            log_audit(
                "create",
                &name,
                &truncate_audit_result(format!("fail: task join: {join_err}")),
            );
            Err(AppError::from(LibvirtError::Internal(format!(
                "Task failed: {join_err}"
            ))))
        }
    }
}

/// Same body as `POST /vms`, but streams subprocess output (mkosi, virt-builder, virt-install, qemu-img)
/// as **SSE** (`text/event-stream`). Final event: `event: complete` with JSON `{"status":"created","name":"..."}`
/// or `event: error` with a plain-text message.
async fn create_vm_stream_handler(
    State(manager): State<LibvirtManager>,
    Extension(jobs): Extension<std::sync::Arc<JobRegistry>>,
    Extension(actor): Extension<RequestActor>,
    Json(mut req): Json<CreateVmRequest>,
) -> Result<axum::response::Response, AppError> {
    require_write(&actor, "vms:write")?;
    if wants_fluxvm_create(&req) {
        let name = req.name.clone();
        let done = match create_fluxvm_vm(&manager, &actor, &req).await {
            Ok(()) => Event::default().event("complete").data(
                serde_json::json!({ "status": "created", "name": name, "backend": "fluxvm" })
                    .to_string(),
            ),
            Err(e) => Event::default().event("error").data(e.to_string()),
        };
        let start = Event::default().data(format!("Creating '{name}' on FluxVM…"));
        return Ok(
            Sse::new(stream::iter([Ok::<Event, Infallible>(start), Ok(done)])).into_response(),
        );
    }
    validate_create_vm_payload(&req)?;

    let name = req.name.clone();
    let cfg = MachinaConfig::load();
    apply_impersonation_create_defaults(&cfg, &actor, &mut req);
    ensure_session_libvirt_identity(&actor, &cfg, &req)?;
    let backend = match req.create_backend.trim() {
        "virt_install" => VmCreateBackend::VirtInstall,
        "libvirt_xml" => VmCreateBackend::LibvirtXml,
        _ => cfg.libvirt.create_backend,
    };
    let libvirt_uri = libvirt_uri_for_create(&cfg, &req);
    let libvirt_cfg = cfg.libvirt.clone();

    let (tok_tx, tok_rx) = tokio::sync::mpsc::channel::<String>(CREATE_LOG_SSE_CAP);
    let (std_tx, std_rx) = std::sync::mpsc::sync_channel::<String>(CREATE_LOG_STD_CAP);

    let job_id = jobs.start_vm_create(&name);
    let jobs_bridge = jobs.clone();
    let _bridge = std::thread::spawn(move || {
        while let Ok(line) = std_rx.recv() {
            jobs_bridge.append_log(job_id, &line);
            if tok_tx.blocking_send(line).is_err() {
                break;
            }
        }
    });

    let handle = tokio::task::spawn_blocking(move || {
        // IMPORTANT: use a dedicated libvirt connection so a long-running create job doesn't
        // block the global LibvirtManager mutex used by dashboard endpoints.
        let conn = Connect::open(Some(&libvirt_uri)).map_err(|e| {
            LibvirtError::Connection(format!(
                "Failed to connect to libvirt ({}): {e}",
                libvirt_uri
            ))
        })?;
        create::create_vm(
            &conn,
            &req,
            backend,
            &libvirt_uri,
            &libvirt_cfg,
            Some(&std_tx),
        )
    });

    let name_done = name.clone();
    let jobs_tail = jobs.clone();
    let tail = stream::once(async move {
        match handle.await {
            Ok(Ok(())) => {
                jobs_tail.complete_vm_create(job_id, &name_done);
                log_audit("create", &name_done, "ok");
                let payload =
                    serde_json::json!({ "status": "created", "name": name_done }).to_string();
                Ok::<Event, Infallible>(Event::default().event("complete").data(payload))
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                jobs_tail.fail(job_id, &msg);
                log_audit(
                    "create",
                    &name_done,
                    &truncate_audit_result(format!("fail: {msg}")),
                );
                Ok(Event::default().event("error").data(msg))
            }
            Err(e) => {
                let msg = format!("create task failed: {e}");
                jobs_tail.fail(job_id, &msg);
                log_audit("create", &name_done, &truncate_audit_result(&msg));
                Ok(Event::default().event("error").data(msg))
            }
        }
    });

    let job_id_str = job_id.to_string();
    let job_head = stream::once(async move {
        let payload = serde_json::json!({ "id": job_id_str }).to_string();
        Ok::<Event, Infallible>(Event::default().event("job").data(payload))
    });

    let stream = job_head
        .chain(ReceiverStream::new(tok_rx).map(|line| Ok(Event::default().data(line))))
        .chain(tail);

    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(20))
                .text("keepalive"),
        )
        .into_response())
}

async fn set_vcpus(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, count)): Path<(String, u32)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    machina_core::validate::validate_vcpus(count)?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        return Ok(Json(super::fluxvm::resize(&name, Some(count), None).await?));
    }
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        resize::set_vcpus(conn, &name2, count)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "vcpus": count }),
    ))
}

async fn set_memory(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, mb)): Path<(String, u64)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    machina_core::validate::validate_memory_mb(mb)?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        return Ok(Json(super::fluxvm::resize(&name, None, Some(mb)).await?));
    }
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        resize::set_memory(conn, &name2, mb)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "name": name,
        "memory_mb": mb,
        "live_applied": outcome.live_applied,
    })))
}

async fn attach_disk_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<AttachDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let target = req.target.clone();
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        let path = req.source.trim();
        super::fluxvm::client()?
            .attach_disk(
                &name,
                &target,
                (!path.is_empty()).then_some(path),
                Some(req.size_gb).filter(|g| *g > 0),
            )
            .await?;
        return Ok(Json(serde_json::json!({
            "status": "attached", "name": name, "target": target, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let req2 = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device::attach_disk(conn, &name2, &req2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "attached", "name": name, "target": target }),
    ))
}

async fn detach_disk_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, target)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        super::fluxvm::client()?
            .detach_disk(&name, &target)
            .await?;
        return Ok(Json(serde_json::json!({
            "status": "detached",
            "name": name,
            "target": target,
            "live_removed": true,
            "requires_restart": false,
            "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let target2 = target.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device::detach_disk(conn, &name2, &target2)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "detached",
        "name": name,
        "target": target,
        "live_removed": outcome.live_removed,
        "requires_restart": !outcome.live_removed,
    })))
}

async fn rename_vm_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<RenameVmRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        super::fluxvm::client()?
            .rename(&name, &req.new_name)
            .await?;
        return Ok(Json(serde_json::json!({
            "status": "renamed", "old_name": name, "new_name": req.new_name, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let new_name = req.new_name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        domain::rename_vm(conn, &name2, &new_name)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "renamed", "old_name": name, "new_name": req.new_name }),
    ))
}

#[derive(serde::Deserialize)]
struct ResizeDiskRequest {
    size_gb: u64,
}

async fn resize_disk_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, target)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<ResizeDiskRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        // FluxVM uses "root" for the boot disk; Machina details show it as vda.
        let disk = if target == "vda" { "root" } else { target.as_str() };
        super::fluxvm::client()?
            .resize_disk(&name, disk, req.size_gb)
            .await?;
        return Ok(Json(serde_json::json!({
            "status": "resized", "name": name, "target": target, "size_gb": req.size_gb, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let target2 = target.clone();
    let size_gb = req.size_gb;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device::resize_block_device(conn, &name2, &target2, size_gb)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "resized", "name": name, "target": target, "size_gb": req.size_gb }),
    ))
}

#[derive(serde::Deserialize)]
struct AttachInterfaceRequest {
    network: String,
    #[serde(default = "default_nic_model")]
    model: String,
}
fn default_nic_model() -> String {
    "virtio".to_string()
}

async fn attach_interface_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(req): Json<AttachInterfaceRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        let mac = super::fluxvm::client()?
            .hotplug_nic(&name, &req.network, None)
            .await?;
        return Ok(Json(serde_json::json!({
            "status": "attached", "name": name, "network": req.network, "mac": mac, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let network = req.network.clone();
    let model = req.model.clone();
    let net = req.network.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device::attach_interface(conn, &name2, &net, &model)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "attached", "name": name, "network": network }),
    ))
}

async fn detach_interface_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path((name, mac)): Path<(String, String)>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        super::fluxvm::client()?.unplug_nic(&name, &mac).await?;
        return Ok(Json(serde_json::json!({
            "status": "detached", "name": name, "mac": mac,
            "live_removed": true, "requires_restart": false, "backend": "fluxvm"
        })));
    }
    let name2 = name.clone();
    let mac2 = mac.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device::detach_interface(conn, &name2, &mac2)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "status": "detached",
        "name": name,
        "mac": mac,
        "live_removed": outcome.live_removed,
        "requires_restart": !outcome.live_removed,
    })))
}

// ── VM Tags ────────────────────────────────────────────────────────

async fn get_vm_tags_handler(
    State(_m): State<LibvirtManager>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tags = machina_core::libvirt::extras::get_vm_tags(&name);
    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(serde::Deserialize)]
struct SetTagsRequest {
    tags: Vec<String>,
}

async fn set_vm_tags_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(req): Json<SetTagsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    machina_core::libvirt::extras::set_vm_tags(&name, req.tags.clone())?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "tags": req.tags }),
    ))
}

async fn get_vm_logs(
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Validate name has no path separators
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(LibvirtError::Invalid("Invalid VM name".into()).into());
    }
    let lines: usize = params
        .get("lines")
        .and_then(|l| l.parse().ok())
        .unwrap_or(500)
        .min(5000);
    let conn_q = ConnQuery {
        connection: params.get("connection").cloned(),
        backend: params.get("backend").cloned(),
    };
    if targets_fluxvm(&manager, &actor, &conn_q, &name).await {
        let content = super::fluxvm::client()?.logs(&name, lines).await?;
        return Ok(Json(serde_json::json!({
            "vm_name": name,
            "log_path": "fluxvm",
            "content": content,
            "backend": "fluxvm",
        })));
    }
    let log_path = format!("/var/log/libvirt/qemu/{}.log", name);
    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => {
            let all_lines: Vec<&str> = c.lines().collect();
            let start = all_lines.len().saturating_sub(lines);
            all_lines[start..].join("\n")
        }
        Err(_) => String::new(),
    };
    Ok(Json(serde_json::json!({
        "vm_name": name,
        "log_path": log_path,
        "content": content,
    })))
}

async fn get_cputune_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> Result<Json<CpuTuneInfo>, AppError> {
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |c| {
        resize::get_cputune(c, &name2)
    })
    .await?;
    Ok(Json(result))
}

async fn get_memtune_handler(
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> Result<Json<MemTuneInfo>, AppError> {
    let name2 = name.clone();
    let result = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |c| {
        resize::get_memtune(c, &name2)
    })
    .await?;
    Ok(Json(result))
}

async fn convert_spice_to_vnc_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    // `name` is a bare positional argv element to the Python-argparse-based `virt-xml`
    // (see graphics_convert::virt_xml_argv), with no `--` separator ahead of it — a name
    // like "--start" would be parsed as a virt-xml flag instead of the domain name.
    machina_core::validate::validate_name(&name)?;
    let cq = conn_q.connection.clone();
    let mgr = manager.clone();
    let name2 = name.clone();
    let out = tokio::task::spawn_blocking(move || {
        let t = mgr.resolve_query(cq.as_deref());
        let uri = mgr.virt_uri_for_target(t);
        machina_core::libvirt::graphics_convert::virt_xml_convert_spice_to_vnc(&uri, &name2)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "name": name,
        "virt_xml_stdout": out
    })))
}

pub fn vm_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/vms", get(list_vms))
        .route("/vms", post(create_vm_handler))
        .route("/vms/stream", post(create_vm_stream_handler))
        .route("/vms/{name}", get(get_vm_details))
        .route("/vms/{name}", delete(delete_vm_handler))
        .route("/vms/{name}/xml", get(get_vm_xml))
        .route("/vms/{name}/kubevirt-bundle", get(kubevirt_bundle_handler))
        .route("/vms/{name}/kubevirt/apply", post(kubevirt_apply_handler))
        .route("/vms/{name}/kubevirt/upload", post(kubevirt_upload_handler))
        .route("/vms/{name}/kubevirt/start", post(kubevirt_start_handler))
        .route("/vms/{name}/start", post(start_vm))
        .route("/vms/{name}/stop", post(stop_vm))
        .route("/vms/{name}/shutdown", post(shutdown_vm))
        .route("/vms/{name}/reboot", post(reboot_vm))
        .route("/vms/{name}/pause", post(pause_vm))
        .route("/vms/{name}/resume", post(resume_vm))
        .route("/vms/{name}/clone", post(clone_vm_handler))
        .route("/vms/{name}/autostart/{enabled}", post(set_autostart))
        .route("/vms/{name}/vcpus/{count}", post(set_vcpus))
        .route("/vms/{name}/memory/{mb}", post(set_memory))
        .route("/vms/{name}/rename", post(rename_vm_handler))
        .route("/vms/{name}/disk/attach", post(attach_disk_handler))
        .route(
            "/vms/{name}/disk/detach/{target}",
            post(detach_disk_handler),
        )
        .route(
            "/vms/{name}/disk/resize/{target}",
            post(resize_disk_handler),
        )
        .route("/vms/{name}/nic/attach", post(attach_interface_handler))
        .route(
            "/vms/{name}/nic/detach/{mac}",
            post(detach_interface_handler),
        )
        .route(
            "/vms/{name}/tags",
            get(get_vm_tags_handler).post(set_vm_tags_handler),
        )
        .route("/vms/{name}/logs", get(get_vm_logs))
        .route("/vms/{name}/cputune", get(get_cputune_handler))
        .route(
            "/vms/{name}/memtune",
            get(get_memtune_handler).post(set_memtune_handler),
        )
        .route("/vms/{name}/scheduler", post(set_scheduler_handler))
        .route("/vms/{name}/vcpu/{vcpu}/pin", post(pin_vcpu_handler))
        .route("/vms/{name}/block/commit", post(block_commit_handler))
        .route("/vms/{name}/block/pull", post(block_pull_handler))
        .route("/vms/{name}/block/job", get(block_job_info_handler))
        .route("/vms/{name}/block/job/abort", post(block_job_abort_handler))
        .route(
            "/vms/{name}/graphics/convert-to-vnc",
            post(convert_spice_to_vnc_handler),
        )
        .route("/vms/{name}/rdp-info", get(rdp_info_handler))
}

async fn rdp_info_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name2 = name.clone();
    let (host, port) = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        machina_core::libvirt::rdp::resolve_rdp_endpoint(conn, &name2)
    })
    .await?;
    Ok(Json(serde_json::json!({
        "host": host,
        "port": port,
        "ws_path": format!("/ws/v1/rdp/{name}"),
        "builtin": true
    })))
}
