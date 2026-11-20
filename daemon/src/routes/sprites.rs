// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! `POST/GET/DELETE /v1/sprites` — instant, disposable microVM sandboxes.
//!
//! Deliberately outside `vms.rs`: sprites don't go through
//! `create::create_vm()` (see `machina_core::libvirt::sprite`'s module doc
//! for why), aren't tracked in any database, and have their own TTL-based
//! reaper instead of the controller's desired-state reconciler. See the
//! sprites design doc for the full rationale.
//!
//! Backend-agnostic: `req.backend` picks libvirt/QEMU
//! (`machina_core::libvirt::sprite`, the original path), Cloud Hypervisor
//! (`machina_core::cloud_hypervisor::sprite`), or Firecracker
//! (`machina_core::firecracker::sprite`) to boot the sprite on.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use virt::connect::Connect;

use machina_core::cloud_hypervisor::sprite::{
    boot_sprite_chv, pause_sprite_chv, resize_sprite_chv, restore_sprite_chv, resume_sprite_chv,
    snapshot_sprite_chv, stop_sprite_chv_for_suspend, ChvBootRequest, ChvRestoreRequest,
};
use machina_core::firecracker::sprite::{
    boot_sprite_fc, pause_sprite_fc, restore_sprite_fc, resume_sprite_fc, snapshot_sprite_fc,
    stop_sprite_fc_for_suspend, FcBootRequest, FcRestoreRequest,
};
use machina_core::libvirt::sprite::{
    boot_sprite, list_golden_images, resolve_golden_image, SpriteBootRequest,
};
use machina_core::{audit, AuditEvent, LibvirtError, LibvirtManager};
use machina_spec::{
    sprite_domain_name, SpriteBackend, SpriteCreateRequest, SpriteHandle, SpriteResizeRequest,
    SpriteRestoreRequest,
};

use crate::auth::{require_write, RequestActor};
use crate::error::{ok_json, AppError};
use crate::sprite_registry::{
    teardown_sprite, BackendState, SpriteBackendHandle, SpriteRegistry, SpriteSnapshotArtifacts,
};

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

async fn create_sprite(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Json(req): Json<SpriteCreateRequest>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    req.validate()
        .map_err(|e| AppError::from(LibvirtError::Invalid(e.to_string())))?;

    let sprite_id = uuid::Uuid::new_v4().to_string();

    let (backend_handle, vsock_cid, audit_target) = match req.backend {
        SpriteBackend::Libvirt => {
            // Dedicated connection, not manager.with_conn(): mirrors
            // create_vm_handler's own reasoning in vms.rs — don't hold the
            // daemon's single shared libvirt mutex for anything that isn't
            // guaranteed-instant, which matters even more here since
            // concurrent sprite bursts are the whole point of this path
            // (serializing them through one mutex would directly undermine
            // "instant").
            let domain_name = sprite_domain_name(&sprite_id);
            let golden_image = req.golden_image.clone();
            let vcpus = req.vcpus;
            let memory_mb = req.memory_mb;
            let network_egress = req.network_egress;

            let libvirt_uri = manager.virt_uri_for_target(manager.default_target());
            let domain_name_for_boot = domain_name.clone();
            let boot_result = tokio::task::spawn_blocking(move || -> Result<_, LibvirtError> {
                let conn = Connect::open(Some(&libvirt_uri)).map_err(|e| {
                    LibvirtError::Connection(format!(
                        "Failed to connect to libvirt ({libvirt_uri}): {e}"
                    ))
                })?;
                let golden_image_path = resolve_golden_image(&golden_image)?;
                boot_sprite(
                    &conn,
                    &SpriteBootRequest {
                        domain_name: &domain_name_for_boot,
                        golden_image_path: &golden_image_path,
                        vcpus,
                        memory_mb,
                        network_egress,
                    },
                )
            })
            .await
            .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))));

            let boot_result = match boot_result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    log_audit(
                        "sprite_create",
                        &domain_name,
                        &format!("fail: {e}").chars().take(500).collect::<String>(),
                    );
                    return Err(AppError::from(e));
                }
                Err(e) => {
                    log_audit("sprite_create", &domain_name, "fail: task join error");
                    return Err(e);
                }
            };

            (
                SpriteBackendHandle::Libvirt {
                    domain_name: domain_name.clone(),
                },
                boot_result.vsock_cid,
                domain_name,
            )
        }
        SpriteBackend::CloudHypervisor => {
            // Cloud Hypervisor requires an explicit guest CID (unlike
            // libvirt's `<cid auto='yes'/>`) — allocate one from the
            // registry's host-wide counter before booting so it can't
            // collide with a concurrently-running sprite of either backend.
            let vsock_cid = registry.next_vsock_cid();
            let audit_target = sprite_domain_name(&sprite_id);

            let golden_image_path = match resolve_golden_image(&req.golden_image) {
                Ok(p) => p,
                Err(e) => {
                    log_audit(
                        "sprite_create",
                        &audit_target,
                        &format!("fail: {e}").chars().take(500).collect::<String>(),
                    );
                    return Err(AppError::from(e));
                }
            };

            let boot_result = boot_sprite_chv(&ChvBootRequest {
                sprite_id: &sprite_id,
                golden_image_path: &golden_image_path,
                vcpus: req.vcpus,
                memory_mb: req.memory_mb,
                vsock_cid,
                network_egress: req.network_egress,
            })
            .await;

            let boot_result = match boot_result {
                Ok(r) => r,
                Err(e) => {
                    log_audit(
                        "sprite_create",
                        &audit_target,
                        &format!("fail: {e}").chars().take(500).collect::<String>(),
                    );
                    return Err(AppError::from(e));
                }
            };

            (
                SpriteBackendHandle::CloudHypervisor {
                    pid: boot_result.pid,
                    api_socket: boot_result.api_socket,
                    disk_path: boot_result.disk_path,
                    vsock_socket: boot_result.vsock_socket,
                    tap_name: boot_result.tap_name,
                },
                Some(vsock_cid),
                audit_target,
            )
        }
        SpriteBackend::Firecracker => {
            // Same reasoning as Cloud Hypervisor: Firecracker requires an
            // explicit guest CID, allocated from the same host-wide,
            // backend-agnostic counter so it can't collide with a
            // concurrently-running sprite of any backend.
            let vsock_cid = registry.next_vsock_cid();
            let audit_target = sprite_domain_name(&sprite_id);

            let golden_image_path = match resolve_golden_image(&req.golden_image) {
                Ok(p) => p,
                Err(e) => {
                    log_audit(
                        "sprite_create",
                        &audit_target,
                        &format!("fail: {e}").chars().take(500).collect::<String>(),
                    );
                    return Err(AppError::from(e));
                }
            };

            let boot_result = boot_sprite_fc(&FcBootRequest {
                sprite_id: &sprite_id,
                golden_image_path: &golden_image_path,
                vcpus: req.vcpus,
                memory_mb: req.memory_mb,
                vsock_cid,
                network_egress: req.network_egress,
            })
            .await;

            let boot_result = match boot_result {
                Ok(r) => r,
                Err(e) => {
                    log_audit(
                        "sprite_create",
                        &audit_target,
                        &format!("fail: {e}").chars().take(500).collect::<String>(),
                    );
                    return Err(AppError::from(e));
                }
            };

            (
                SpriteBackendHandle::Firecracker {
                    pid: boot_result.pid,
                    api_socket: boot_result.api_socket,
                    disk_path: boot_result.disk_path,
                    vsock_socket: boot_result.vsock_socket,
                    tap_name: boot_result.tap_name,
                },
                Some(vsock_cid),
                audit_target,
            )
        }
    };

    let handle = registry
        .register(
            sprite_id.clone(),
            backend_handle,
            req.ttl_seconds,
            vsock_cid,
            req.network_egress,
            req.vcpus,
            req.memory_mb,
        )
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;

    log_audit("sprite_create", &audit_target, "ok");
    Ok(Json(handle))
}

async fn list_sprites(
    Extension(registry): Extension<Arc<SpriteRegistry>>,
) -> Json<Vec<SpriteHandle>> {
    Json(registry.list())
}

/// Bare filenames (no `.qcow2`) of golden images available to boot a sprite
/// from — lets a caller (the web UI's sprite-creation form) offer a picker
/// instead of requiring the golden image key to already be known.
async fn list_golden_images_handler() -> Result<Json<Vec<String>>, AppError> {
    Ok(Json(list_golden_images()?))
}

async fn get_sprite(
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<SpriteHandle>, AppError> {
    registry
        .get(&id)
        .map(Json)
        .ok_or_else(|| AppError::from(LibvirtError::NotFound(format!("sprite '{id}' not found"))))
}

async fn delete_sprite(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "sprites:write")?;
    let Some(backend) = registry.remove(&id) else {
        return Err(AppError::from(LibvirtError::NotFound(format!(
            "sprite '{id}' not found"
        ))));
    };
    let label = backend.describe();
    match teardown_sprite(&manager, backend).await {
        Ok(()) => {
            log_audit("sprite_delete", &label, "ok");
            Ok(ok_json("deleted", &id))
        }
        Err(e) => {
            log_audit("sprite_delete", &label, &format!("fail: {e}"));
            Err(AppError::from(LibvirtError::Operation(e)))
        }
    }
}

fn not_running(id: &str) -> AppError {
    AppError::from(LibvirtError::Invalid(format!(
        "sprite '{id}' is not running (it may be paused or suspended)"
    )))
}

fn not_found(id: &str) -> AppError {
    AppError::from(LibvirtError::NotFound(format!("sprite '{id}' not found")))
}

fn libvirt_not_supported() -> AppError {
    AppError::from(LibvirtError::Invalid(
        "this operation is not yet supported for libvirt sprites".into(),
    ))
}

async fn pause_sprite(
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    let backend = registry.peek_backend(&id).ok_or_else(|| not_found(&id))?;
    let BackendState::Live(live) = backend else {
        return Err(not_running(&id));
    };
    let result = match &live {
        SpriteBackendHandle::CloudHypervisor { api_socket, .. } => {
            pause_sprite_chv(api_socket).await
        }
        SpriteBackendHandle::Firecracker { api_socket, .. } => pause_sprite_fc(api_socket).await,
        SpriteBackendHandle::Libvirt { .. } => return Err(libvirt_not_supported()),
    };
    if let Err(e) = result {
        log_audit("sprite_pause", &id, &format!("fail: {e}"));
        return Err(AppError::from(e));
    }
    let handle = registry
        .mark_paused(&id)
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;
    log_audit("sprite_pause", &id, "ok");
    Ok(Json(handle))
}

async fn resume_sprite(
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    let backend = registry.peek_backend(&id).ok_or_else(|| not_found(&id))?;
    let BackendState::Live(live) = backend else {
        return Err(not_running(&id));
    };
    let result = match &live {
        SpriteBackendHandle::CloudHypervisor { api_socket, .. } => {
            resume_sprite_chv(api_socket).await
        }
        SpriteBackendHandle::Firecracker { api_socket, .. } => resume_sprite_fc(api_socket).await,
        SpriteBackendHandle::Libvirt { .. } => return Err(libvirt_not_supported()),
    };
    if let Err(e) = result {
        log_audit("sprite_resume", &id, &format!("fail: {e}"));
        return Err(AppError::from(e));
    }
    let handle = registry
        .mark_running(&id)
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;
    log_audit("sprite_resume", &id, "ok");
    Ok(Json(handle))
}

/// Cloud-Hypervisor-only — Firecracker has no live-resize API.
async fn resize_sprite(
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
    Json(req): Json<SpriteResizeRequest>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    req.validate()
        .map_err(|e| AppError::from(LibvirtError::Invalid(e.to_string())))?;
    let backend = registry.peek_backend(&id).ok_or_else(|| not_found(&id))?;
    let BackendState::Live(SpriteBackendHandle::CloudHypervisor { api_socket, .. }) = backend
    else {
        return Err(AppError::from(LibvirtError::Invalid(
            "resize is only supported for running cloud-hypervisor sprites".into(),
        )));
    };
    if let Err(e) = resize_sprite_chv(&api_socket, req.vcpus, req.memory_mb).await {
        log_audit("sprite_resize", &id, &format!("fail: {e}"));
        return Err(AppError::from(e));
    }
    let handle = registry
        .update_sizing(&id, req.vcpus, req.memory_mb)
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;
    log_audit("sprite_resize", &id, "ok");
    Ok(Json(handle))
}

/// Suspend a running/paused sprite: pause (if not already), snapshot full
/// VM state to disk, then kill the process — leaving only the snapshot
/// artifacts `restore_sprite` needs to bring it back.
async fn snapshot_sprite(
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    let backend = registry.peek_backend(&id).ok_or_else(|| not_found(&id))?;
    let BackendState::Live(live) = backend else {
        return Err(not_running(&id));
    };
    // The registry's own state is the source of truth for whether a pause
    // is still needed — not the VMM's idempotency (or lack of it) around
    // pausing an already-paused instance.
    let already_paused = registry
        .get(&id)
        .map(|h| h.state == machina_spec::SpriteState::Paused)
        .unwrap_or(false);

    let artifacts = match &live {
        SpriteBackendHandle::CloudHypervisor {
            pid,
            api_socket,
            disk_path,
            vsock_socket,
            tap_name,
        } => {
            if !already_paused {
                if let Err(e) = pause_sprite_chv(api_socket).await {
                    log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                    return Err(AppError::from(e));
                }
            }
            let snap = match snapshot_sprite_chv(&id, api_socket).await {
                Ok(s) => s,
                Err(e) => {
                    log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                    return Err(AppError::from(e));
                }
            };
            if let Err(e) =
                stop_sprite_chv_for_suspend(*pid, api_socket, vsock_socket, tap_name.as_deref())
                    .await
            {
                log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                return Err(AppError::from(LibvirtError::Operation(e)));
            }
            SpriteSnapshotArtifacts::CloudHypervisor {
                snapshot_dir: snap.snapshot_dir,
                disk_path: disk_path.clone(),
                tap_name: tap_name.clone(),
            }
        }
        SpriteBackendHandle::Firecracker {
            pid,
            api_socket,
            disk_path,
            vsock_socket,
            tap_name,
        } => {
            if !already_paused {
                if let Err(e) = pause_sprite_fc(api_socket).await {
                    log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                    return Err(AppError::from(e));
                }
            }
            let snap = match snapshot_sprite_fc(&id, api_socket).await {
                Ok(s) => s,
                Err(e) => {
                    log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                    return Err(AppError::from(e));
                }
            };
            if let Err(e) =
                stop_sprite_fc_for_suspend(*pid, api_socket, vsock_socket, tap_name.as_deref())
                    .await
            {
                log_audit("sprite_snapshot", &id, &format!("fail: {e}"));
                return Err(AppError::from(LibvirtError::Operation(e)));
            }
            SpriteSnapshotArtifacts::Firecracker {
                snapshot_path: snap.snapshot_path,
                mem_file_path: snap.mem_file_path,
                disk_path: disk_path.clone(),
                tap_name: tap_name.clone(),
            }
        }
        SpriteBackendHandle::Libvirt { .. } => return Err(libvirt_not_supported()),
    };

    let handle = registry
        .mark_suspended(&id, artifacts)
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;
    log_audit("sprite_snapshot", &id, "ok");
    Ok(Json(handle))
}

async fn restore_sprite(
    Extension(actor): Extension<RequestActor>,
    Extension(registry): Extension<Arc<SpriteRegistry>>,
    Path(id): Path<String>,
    Json(req): Json<SpriteRestoreRequest>,
) -> Result<Json<SpriteHandle>, AppError> {
    require_write(&actor, "sprites:write")?;
    req.validate()
        .map_err(|e| AppError::from(LibvirtError::Invalid(e.to_string())))?;
    let backend = registry.peek_backend(&id).ok_or_else(|| not_found(&id))?;
    let BackendState::Suspended(artifacts) = backend else {
        return Err(AppError::from(LibvirtError::Invalid(format!(
            "sprite '{id}' is not suspended"
        ))));
    };
    let network_egress = registry.get(&id).map(|h| h.network_egress).unwrap_or(false);

    let new_backend = match artifacts {
        SpriteSnapshotArtifacts::CloudHypervisor {
            snapshot_dir,
            disk_path,
            ..
        } => {
            match restore_sprite_chv(&ChvRestoreRequest {
                sprite_id: &id,
                snapshot_dir: &snapshot_dir,
                network_egress,
            })
            .await
            {
                Ok(r) => SpriteBackendHandle::CloudHypervisor {
                    pid: r.pid,
                    api_socket: r.api_socket,
                    disk_path,
                    vsock_socket: r.vsock_socket,
                    tap_name: r.tap_name,
                },
                Err(e) => {
                    log_audit("sprite_restore", &id, &format!("fail: {e}"));
                    return Err(AppError::from(e));
                }
            }
        }
        SpriteSnapshotArtifacts::Firecracker {
            snapshot_path,
            mem_file_path,
            disk_path,
            ..
        } => match restore_sprite_fc(&FcRestoreRequest {
            sprite_id: &id,
            snapshot_path: &snapshot_path,
            mem_file_path: &mem_file_path,
            network_egress,
        })
        .await
        {
            Ok(r) => SpriteBackendHandle::Firecracker {
                pid: r.pid,
                api_socket: r.api_socket,
                disk_path,
                vsock_socket: r.vsock_socket,
                tap_name: r.tap_name,
            },
            Err(e) => {
                log_audit("sprite_restore", &id, &format!("fail: {e}"));
                return Err(AppError::from(e));
            }
        },
    };

    let handle = registry
        .mark_restored(&id, new_backend, req.ttl_seconds)
        .map_err(|e| AppError::from(LibvirtError::Internal(e.into())))?;
    log_audit("sprite_restore", &id, "ok");
    Ok(Json(handle))
}

pub fn sprite_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/sprites", post(create_sprite).get(list_sprites))
        .route("/sprites/golden-images", get(list_golden_images_handler))
        .route("/sprites/{id}", get(get_sprite).delete(delete_sprite))
        .route("/sprites/{id}/pause", post(pause_sprite))
        .route("/sprites/{id}/resume", post(resume_sprite))
        .route("/sprites/{id}/resize", post(resize_sprite))
        .route("/sprites/{id}/snapshot", post(snapshot_sprite))
        .route("/sprites/{id}/restore", post(restore_sprite))
}
