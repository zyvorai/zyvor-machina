// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Standalone qcow2 → KubeVirt bundle and optional cluster exec (hyper2kvm-style disk upload).

use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    routing::{get, post},
    Json, Router,
};
use machina_core::{
    audit, kubevirt_bundle_from_qcow2, AuditEvent, KubeVirtBundle, LibvirtError, LibvirtManager,
    MachinaConfig,
};
use serde::Deserialize;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;
use crate::kubevirt_exec;
use crate::routes::events::{EventBus, MachinaEvent};

fn emit(bus: &Arc<EventBus>, kind: &str, target: &str, status: &str, message: &str) {
    let mut ev = MachinaEvent::now(kind, target, status);
    ev.message = message.chars().take(512).collect();
    bus.emit(ev);
}

fn log_audit(action: &str, target: &str, result: &str) {
    let event = AuditEvent {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        action: action.to_string(),
        target: target.to_string(),
        result: result.to_string(),
        actor: String::new(),
    };
    audit::write_audit_event(&event);
}

#[derive(Debug, Deserialize)]
pub struct Qcow2KubeVirtParams {
    /// Absolute path to `.qcow2` (or `.raw`/`.img`) on the hypervisor.
    pub qcow2_path: String,
    /// `linux`, `windows`, or `auto` (detect from filename).
    #[serde(default)]
    pub guest_os: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub k8s_vm_name: Option<String>,
    #[serde(default)]
    pub datavolume_name: Option<String>,
    #[serde(default)]
    pub storage_gi: Option<u32>,
    #[serde(default)]
    pub storage_class: Option<String>,
    #[serde(default)]
    pub vcpus: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
    /// When omitted: virtio-win CDROM for Windows, omitted for Linux.
    #[serde(default)]
    pub include_virtio_cdrom: Option<bool>,
}

fn validate_qcow2_allowed(path: &str, allowed_prefixes: &[String]) -> Result<(), LibvirtError> {
    if !allowed_prefixes.iter().any(|p| path.starts_with(p)) {
        return Err(LibvirtError::Invalid(format!(
            "Path not in an allowed images directory: {path}"
        )));
    }
    Ok(())
}

fn bundle_from_params(
    p: &Qcow2KubeVirtParams,
    cfg: &machina_core::config::KubeVirtConfig,
) -> Result<KubeVirtBundle, LibvirtError> {
    kubevirt_bundle_from_qcow2(
        &p.qcow2_path,
        cfg,
        p.guest_os.as_deref(),
        p.namespace.as_deref(),
        p.k8s_vm_name.as_deref(),
        p.datavolume_name.as_deref(),
        p.storage_gi,
        p.storage_class.as_deref(),
        p.vcpus,
        p.memory_mb,
        p.include_virtio_cdrom,
    )
}

async fn qcow2_kubevirt_bundle_get(
    State(manager): State<LibvirtManager>,
    Query(p): Query<Qcow2KubeVirtParams>,
) -> Result<Json<KubeVirtBundle>, AppError> {
    if p.qcow2_path.trim().is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "qcow2_path query parameter is required".into(),
        )));
    }
    let path = p.qcow2_path.trim().to_string();
    let prefixes = tokio::task::spawn_blocking({
        let mgr = manager.clone();
        move || mgr.with_conn(machina_core::libvirt::storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    validate_qcow2_allowed(&path, &prefixes)?;

    let cfg = MachinaConfig::load();
    let bundle = bundle_from_params(
        &Qcow2KubeVirtParams {
            qcow2_path: path,
            ..p
        },
        &cfg.kubevirt,
    )?;
    log_audit("kubevirt-qcow2-bundle", &bundle.libvirt_root_disk, "ok");
    Ok(Json(bundle))
}

async fn qcow2_kubevirt_bundle_post(
    State(manager): State<LibvirtManager>,
    Json(p): Json<Qcow2KubeVirtParams>,
) -> Result<Json<KubeVirtBundle>, AppError> {
    if p.qcow2_path.trim().is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "qcow2_path is required".into(),
        )));
    }
    let path = p.qcow2_path.trim().to_string();
    let prefixes = tokio::task::spawn_blocking({
        let mgr = manager.clone();
        move || mgr.with_conn(machina_core::libvirt::storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    validate_qcow2_allowed(&path, &prefixes)?;

    let cfg = MachinaConfig::load();
    let bundle = bundle_from_params(&p, &cfg.kubevirt)?;
    log_audit("kubevirt-qcow2-bundle", &bundle.libvirt_root_disk, "ok");
    Ok(Json(bundle))
}

async fn qcow2_kubevirt_apply(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(p): Json<Qcow2KubeVirtParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let path = p.qcow2_path.trim();
    let prefixes = tokio::task::spawn_blocking({
        let mgr = manager.clone();
        move || mgr.with_conn(machina_core::libvirt::storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    validate_qcow2_allowed(path, &prefixes)?;

    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let bundle = bundle_from_params(&p, &kv)?;
    let tmp = std::env::temp_dir().join(format!(
        "machina-kubevirt-qcow2-{}-{}.yaml",
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
    let path_yaml = tmp.clone();
    let (code, stdout, stderr) = kubevirt_exec::kubectl_apply_yaml(&kv, &path_yaml)
        .await
        .map_err(AppError::from)?;
    let _ = std::fs::remove_file(&tmp);
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-qcow2-apply", &bundle.libvirt_root_disk, audit);
    emit(
        &bus,
        "kubevirt.qcow2.apply",
        &bundle.libvirt_root_disk,
        audit,
        &bundle.virtual_machine_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

async fn qcow2_kubevirt_upload(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(p): Json<Qcow2KubeVirtParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let path = p.qcow2_path.trim();
    let prefixes = tokio::task::spawn_blocking({
        let mgr = manager.clone();
        move || mgr.with_conn(machina_core::libvirt::storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    validate_qcow2_allowed(path, &prefixes)?;

    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let bundle = bundle_from_params(&p, &kv)?;
    let (code, stdout, stderr) = kubevirt_exec::virtctl_image_upload_disk(
        &kv,
        &bundle.datavolume_name,
        bundle.upload_size_gi,
        &bundle.libvirt_root_disk,
        &bundle.namespace,
    )
    .await
    .map_err(AppError::from)?;
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-qcow2-upload", &bundle.libvirt_root_disk, audit);
    emit(
        &bus,
        "kubevirt.qcow2.upload",
        &bundle.libvirt_root_disk,
        audit,
        &bundle.datavolume_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

async fn qcow2_kubevirt_start(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(bus): Extension<Arc<EventBus>>,
    Json(p): Json<Qcow2KubeVirtParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let path = p.qcow2_path.trim();
    let prefixes = tokio::task::spawn_blocking({
        let mgr = manager.clone();
        move || mgr.with_conn(machina_core::libvirt::storage::disk_image_delete_allowed_prefixes)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))??;
    validate_qcow2_allowed(path, &prefixes)?;

    let cfg = MachinaConfig::load();
    let kv = cfg.kubevirt.clone();
    let bundle = bundle_from_params(&p, &kv)?;
    let (code, stdout, stderr) =
        kubevirt_exec::virtctl_start_vm(&kv, &bundle.virtual_machine_name, &bundle.namespace)
            .await
            .map_err(AppError::from)?;
    let audit = if code == 0 { "ok" } else { "error" };
    log_audit("kubevirt-qcow2-start", &bundle.libvirt_root_disk, audit);
    emit(
        &bus,
        "kubevirt.qcow2.start",
        &bundle.libvirt_root_disk,
        audit,
        &bundle.virtual_machine_name,
    );
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

pub fn kubevirt_routes() -> Router<LibvirtManager> {
    Router::new()
        .route(
            "/kubevirt/qcow2-bundle",
            get(qcow2_kubevirt_bundle_get).post(qcow2_kubevirt_bundle_post),
        )
        .route("/kubevirt/qcow2/apply", post(qcow2_kubevirt_apply))
        .route("/kubevirt/qcow2/upload", post(qcow2_kubevirt_upload))
        .route("/kubevirt/qcow2/start", post(qcow2_kubevirt_start))
}
