// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Standalone, project-owned storage volumes — Phase C of the next-gen roadmap
//! (`/Users/ssahani/.claude/plans/lazy-munching-quilt.md`). Distinct from `vm_disks`,
//! which is always created alongside a specific VM: a volume here can be created
//! unattached and attached to any VM later.
//!
//! Two backends, chosen automatically per volume at create time:
//! - **Atlas** (`ATLAS_ENABLED=1`, `engine::atlas_bridge`) — real Ceph/RBD volumes with
//!   genuine snapshot/expand/delete. Attach resolves the RBD backend-native id and
//!   builds a `rbd:` libvirt network-disk source via `engine::atlas_vm::rbd_source`,
//!   the same helper the native `atlas_root_disk` VM-create path already uses.
//! - **Local pool** fallback — the existing pool-volume agent RPC in
//!   `api::storage::create_storage_pool_volume`, for deployments without Atlas.
//!
//! Either way, attach/detach/extend delegate to the existing
//! `vms::{attach_vm_disk, detach_vm_disk, resize_vm_disk}` handlers — no new libvirt
//! plumbing here. `attach_vm_disk` already supports attaching an existing path without
//! allocating a new disk when `size_gib` is omitted, which is exactly what attaching a
//! pre-existing volume needs.

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::projects::default_project_id;
use crate::api::storage::{self, CreateStorageVolumeBody, StoragePoolHostQuery};
use crate::api::vms;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::atlas_bridge::{self, AtlasOwner};
use crate::engine::atlas_vm;
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VolumeRow {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub size_gib: i64,
    pub volume_class: String,
    pub status: String,
    pub attached_vm_id: Option<Uuid>,
    pub attached_device: Option<String>,
    pub atlas_backed: bool,
    pub delete_on_termination: bool,
    pub read_iops: Option<i64>,
    pub write_iops: Option<i64>,
    pub read_bps: Option<i64>,
    pub write_bps: Option<i64>,
    #[sqlx(skip)]
    pub ec2_id: String,
}

impl VolumeRow {
    fn with_id(mut self) -> Self {
        self.ec2_id = crate::resource_ids::ec2_id(crate::resource_ids::Kind::Volume, self.id);
        self
    }
}

const VOLUME_SELECT: &str = "SELECT id, project_id, name, size_gib, volume_class, status, \
    attached_vm_id, attached_device, (atlas_volume_id IS NOT NULL) AS atlas_backed, \
    delete_on_termination, read_iops, write_iops, read_bps, write_bps FROM volumes";

/// `vms::{attach_vm_disk, detach_vm_disk, resize_vm_disk}` only enqueue a task and
/// return immediately — they don't wait for the agent to actually finish the libvirt
/// op. Without this, an attach/detach handler that updates `volumes.status`
/// immediately after enqueueing reports success even when the underlying operation
/// later fails (this was a real bug: a failed detach still left the volume marked
/// `available`). Polls the task to a terminal state before the caller updates any
/// volume state.
pub(crate) async fn wait_for_task(pool: &crate::db::DbPool, task_id: &str) -> Result<(), ApiError> {
    wait_for_task_timeout(pool, task_id, Duration::from_secs(20)).await
}

/// Same polling behavior as `wait_for_task`, but with a caller-chosen timeout — some
/// operations (e.g. VM creation with an image pull) can legitimately take longer than
/// the 20s default used for attach/detach/resize/nic operations. Note that a timeout
/// here does not mean the underlying task stopped: it keeps running in the background
/// and may still complete later — callers that track created resources (e.g.
/// `stacks::build_stack`) should account for that instead of assuming timeout == no-op.
pub(crate) async fn wait_for_task_timeout(
    pool: &crate::db::DbPool,
    task_id: &str,
    timeout: Duration,
) -> Result<(), ApiError> {
    let task_uuid = Uuid::parse_str(task_id).map_err(|e| ApiError::internal(e.to_string()))?;
    let attempts = (timeout.as_millis() / 500).max(1) as u32;
    for _ in 0..attempts {
        let row: Option<(String, Option<String>)> =
            crate::db::query_as("SELECT status, message FROM tasks WHERE id = ?")
                .bind(task_uuid)
                .fetch_optional(pool)
                .await?;
        match row {
            Some((status, _)) if status == "completed" => return Ok(()),
            Some((status, message)) if status == "failed" => {
                return Err(ApiError::internal(
                    message.unwrap_or_else(|| "task failed".into()),
                ));
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    Err(ApiError::internal(
        "timed out waiting for disk operation to complete",
    ))
}

#[derive(Debug, Deserialize)]
pub struct ListVolumesQuery {
    #[serde(default)]
    pub project_id: Option<Uuid>,
    /// Only volumes carrying this tag (`resource_tags`); `tag_value` is optional.
    #[serde(default)]
    pub tag_key: Option<String>,
    #[serde(default)]
    pub tag_value: Option<String>,
}

pub async fn list_volumes(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Query(q): Query<ListVolumesQuery>,
) -> Result<Json<Vec<VolumeRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, VolumeRow>(&format!(
        "{VOLUME_SELECT} WHERE (?1 IS NULL OR project_id = ?1) \
         AND (?2 IS NULL OR EXISTS (SELECT 1 FROM resource_tags rt WHERE rt.resource_type = 'volume' \
              AND rt.resource_id = lower(hex(volumes.id)) AND rt.key = ?2 AND (?3 IS NULL OR rt.value = ?3))) \
         ORDER BY created_at DESC"
    ))
    .bind(q.project_id)
    .bind(q.tag_key.as_deref())
    .bind(q.tag_value.as_deref())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(VolumeRow::with_id).collect()))
}

pub async fn get_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, VolumeRow>(&format!("{VOLUME_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

#[derive(Debug, Deserialize)]
pub struct CreateVolumeBody {
    pub name: String,
    pub size_gib: i64,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default = "default_volume_class")]
    pub volume_class: String,
    /// Delete the volume together with the instance it is attached to (EC2 `DeleteOnTermination`).
    #[serde(default)]
    pub delete_on_termination: bool,
}

fn default_volume_class() -> String {
    "silver".into()
}

/// `POST /api/v1/volumes` — creates the row unattached, then provisions the backing
/// storage synchronously (Atlas or local pool).
pub async fn create_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateVolumeBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.size_gib < 1 {
        return Err(ApiError::bad_request("size_gib must be at least 1"));
    }
    let project_id = match body.project_id {
        Some(id) => id,
        None => default_project_id(&state.pool).await?,
    };

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO volumes (id, project_id, name, size_gib, volume_class, status, delete_on_termination) VALUES (?, ?, ?, ?, ?, 'creating', ?)",
    )
    .bind(id)
    .bind(project_id)
    .bind(&body.name)
    .bind(body.size_gib)
    .bind(&body.volume_class)
    .bind(body.delete_on_termination)
    .execute(&state.pool)
    .await?;

    let result = if state.config.atlas_enabled {
        create_volume_atlas(&state, id, &body.name, body.size_gib, &body.volume_class).await
    } else {
        create_volume_local(&state, actor, id, body.size_gib, &body.volume_class).await
    };
    if let Err(e) = result {
        crate::db::query("UPDATE volumes SET status = 'error' WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await?;
        return Err(e);
    }

    let row = crate::db::query_as::<_, VolumeRow>(&format!("{VOLUME_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

async fn create_volume_atlas(
    state: &AppState,
    id: Uuid,
    name: &str,
    size_gib: i64,
    volume_class: &str,
) -> Result<(), ApiError> {
    let cfg = &state.config;
    let client =
        atlas_bridge::require_client(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let policy = if volume_class == "silver" {
        cfg.atlas_default_policy.as_str()
    } else {
        volume_class
    };
    let owner = AtlasOwner {
        product: "machina".into(),
        resource_type: "volume".into(),
        resource_id: id.to_string(),
        role: "data".into(),
    };
    let job = client
        .create_volume(
            &cfg.atlas_tenant_id,
            &atlas_safe_name(name),
            size_gib.max(1) * 1024 * 1024 * 1024,
            policy,
            Some(&owner),
            None,
            None,
        )
        .await
        .map_err(|e| ApiError::internal(format!("Atlas volume create failed: {e}")))?;
    let atlas_volume_id = job
        .resource_volume_id()
        .ok_or_else(|| ApiError::internal("Atlas create-volume returned no volume_id"))?;
    if let Some(jid) = job.job_id() {
        let _ = client.wait_for_job(jid, Duration::from_secs(60)).await;
    }
    crate::db::query("UPDATE volumes SET status = 'available', atlas_volume_id = ? WHERE id = ?")
        .bind(&atlas_volume_id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// Atlas uses the volume name as the Kubernetes PVC name (RFC 1123 label) — mirrors
/// `engine::atlas_vm::dns_safe`, duplicated locally since that helper is private.
fn atlas_safe_name(s: &str) -> String {
    let lowered: String = s
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = lowered.trim_matches('-');
    if trimmed.is_empty() {
        "vol".to_string()
    } else {
        trimmed.chars().take(63).collect()
    }
}

async fn create_volume_local(
    state: &AppState,
    actor: AuthUser,
    id: Uuid,
    size_gib: i64,
    volume_class: &str,
) -> Result<(), ApiError> {
    let pool_row: Option<Uuid> =
        crate::db::query_scalar("SELECT id FROM storage_pools WHERE storage_class = ? LIMIT 1")
            .bind(volume_class)
            .fetch_optional(&state.pool)
            .await?;
    let pool_id = match pool_row {
        Some(p) => p,
        None => crate::db::query_scalar("SELECT id FROM storage_pools LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("no storage pool configured"))?,
    };
    crate::db::query("UPDATE volumes SET storage_pool_id = ? WHERE id = ?")
        .bind(pool_id)
        .bind(id)
        .execute(&state.pool)
        .await?;

    let vol_file_name = format!("vol-{id}");
    let agent_result = storage::create_storage_pool_volume(
        State(state.clone()),
        Extension(actor),
        Path(pool_id),
        Query(StoragePoolHostQuery { host_id: None }),
        Json(CreateStorageVolumeBody {
            name: vol_file_name,
            capacity_gb: size_gib.max(1) as u64,
            format: "qcow2".into(),
        }),
    )
    .await?;
    let path = agent_result
        .0
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    crate::db::query("UPDATE volumes SET status = 'available', path = ? WHERE id = ?")
        .bind(path)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

pub async fn delete_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(Option<Uuid>, Option<String>, Option<Uuid>)> = crate::db::query_as(
        "SELECT attached_vm_id, atlas_volume_id, storage_pool_id FROM volumes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((attached, atlas_volume_id, pool_id)) = row else {
        return Err(ApiError::not_found("volume not found"));
    };
    if attached.is_some() {
        return Err(ApiError::bad_request(
            "volume is still attached — detach it first",
        ));
    }

    if let Some(atlas_id) = atlas_volume_id {
        let client = atlas_bridge::require_client(&state.config)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        client
            .delete_volume(&atlas_id)
            .await
            .map_err(|e| ApiError::internal(format!("Atlas volume delete failed: {e}")))?;
    } else if let Some(pool_id) = pool_id {
        let vol_file_name = format!("vol-{id}");
        let _ = storage::delete_storage_pool_volume(
            State(state.clone()),
            Extension(actor),
            Path((pool_id, vol_file_name)),
            Query(StoragePoolHostQuery { host_id: None }),
        )
        .await;
    }

    crate::db::query("DELETE FROM volumes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn resolve_atlas_rbd_source(
    client: &atlas_bridge::AtlasClient,
    cfg: &crate::config::ControllerConfig,
    atlas_volume_id: &str,
) -> Result<String, ApiError> {
    for attempt in 0..8 {
        if let Ok(v) = client.get_volume(atlas_volume_id).await {
            if let Some(native) = v.backend_native_id {
                return Ok(atlas_vm::rbd_source(cfg, &native));
            }
        }
        if attempt < 7 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
    Err(ApiError::internal(
        "Atlas volume has no backend-native id yet — try again shortly",
    ))
}

#[derive(Debug, Deserialize)]
pub struct AttachVolumeBody {
    pub vm_id: Uuid,
    #[serde(default = "default_target_dev")]
    pub target_dev: String,
    #[serde(default)]
    pub delete_on_termination: Option<bool>,
}

fn default_target_dev() -> String {
    "vdb".into()
}

pub async fn attach_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AttachVolumeBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(Option<String>, Option<String>)> =
        crate::db::query_as("SELECT path, atlas_volume_id FROM volumes WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((path, atlas_volume_id)) = row else {
        return Err(ApiError::not_found("volume not found"));
    };
    let disk_source = if let Some(atlas_id) = &atlas_volume_id {
        let client = atlas_bridge::require_client(&state.config)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        resolve_atlas_rbd_source(&client, &state.config, atlas_id).await?
    } else {
        path.ok_or_else(|| ApiError::internal("volume has no backing path"))?
    };

    // attach_vm_disk_trusted, not the public attach_vm_disk: disk_source above is
    // server-resolved (Atlas RBD lookup or the volume's own recorded local path),
    // never raw caller input, so it's exempt from the public handler's rbd:-source
    // rejection by design — see the doc comment on attach_vm_disk_trusted.
    let task = vms::attach_vm_disk_trusted(
        state.clone(),
        actor,
        body.vm_id,
        vms::AttachDiskBody {
            disk_path: disk_source,
            target_dev: body.target_dev.clone(),
            size_gib: None,
        },
    )
    .await?;
    wait_for_task(&state.pool, &task.0.task_id).await?;

    crate::db::query("UPDATE volumes SET status = 'in-use', attached_vm_id = ?, attached_device = ?, delete_on_termination = COALESCE(?, delete_on_termination) WHERE id = ?")
        .bind(body.vm_id)
        .bind(&body.target_dev)
        .bind(body.delete_on_termination)
        .bind(id)
        .execute(&state.pool)
        .await?;

    let row = crate::db::query_as::<_, VolumeRow>(&format!("{VOLUME_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

pub async fn detach_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(Option<Uuid>, Option<String>)> =
        crate::db::query_as("SELECT attached_vm_id, attached_device FROM volumes WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((vm_id, target_dev)) = row else {
        return Err(ApiError::not_found("volume not found"));
    };
    if let (Some(vm_id), Some(target_dev)) = (vm_id, target_dev) {
        let task = vms::detach_vm_disk(
            State(state.clone()),
            Extension(actor),
            Path((vm_id, target_dev)),
        )
        .await?;
        wait_for_task(&state.pool, &task.0.task_id).await?;
    }
    crate::db::query("UPDATE volumes SET status = 'available', attached_vm_id = NULL, attached_device = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, VolumeRow>(&format!("{VOLUME_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

#[derive(Debug, Deserialize)]
pub struct ExtendVolumeBody {
    pub new_size_gib: i64,
}

pub async fn extend_volume(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ExtendVolumeBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    if body.new_size_gib < 1 {
        return Err(ApiError::bad_request("new_size_gib must be at least 1"));
    }
    let row: Option<(Option<Uuid>, Option<String>, Option<String>)> = crate::db::query_as(
        "SELECT attached_vm_id, attached_device, atlas_volume_id FROM volumes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((attached_vm_id, attached_device, atlas_volume_id)) = row else {
        return Err(ApiError::not_found("volume not found"));
    };
    if let Some(atlas_id) = &atlas_volume_id {
        let client = atlas_bridge::require_client(&state.config)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        client
            .expand_volume(atlas_id, body.new_size_gib.max(1) * 1024 * 1024 * 1024)
            .await
            .map_err(|e| ApiError::internal(format!("Atlas volume expand failed: {e}")))?;
    }
    if let (Some(vm_id), Some(target_dev)) = (attached_vm_id, attached_device) {
        let task = vms::resize_vm_disk(
            State(state.clone()),
            Extension(actor),
            Path((vm_id, target_dev)),
            Json(vms::ResizeVmDiskBody {
                size_gb: body.new_size_gib.max(1) as u64,
            }),
        )
        .await?;
        wait_for_task(&state.pool, &task.0.task_id).await?;
    }
    crate::db::query("UPDATE volumes SET size_gib = ? WHERE id = ?")
        .bind(body.new_size_gib)
        .bind(id)
        .execute(&state.pool)
        .await?;
    let row = crate::db::query_as::<_, VolumeRow>(&format!("{VOLUME_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

// ---------------------------------------------------------------------------
// Snapshots — real Atlas snapshots when the volume is Atlas-backed
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VolumeSnapshotRow {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub name: String,
    pub status: String,
}

pub async fn list_volume_snapshots(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<Uuid>,
) -> Result<Json<Vec<VolumeSnapshotRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, VolumeSnapshotRow>(
        "SELECT id, volume_id, name, status FROM volume_snapshots WHERE volume_id = ? ORDER BY created_at DESC",
    )
    .bind(volume_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VolumeSnapshotWithVolumeRow {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub volume_name: String,
    pub name: String,
    pub status: String,
}

/// Same rows as `list_volume_snapshots` but across every volume -- the "Snapshots" tab
/// listing doesn't scope to one volume the way the volume-detail page does.
pub async fn list_all_volume_snapshots(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<VolumeSnapshotWithVolumeRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, VolumeSnapshotWithVolumeRow>(
        "SELECT s.id, s.volume_id, v.name AS volume_name, s.name, s.status \
         FROM volume_snapshots s JOIN volumes v ON v.id = s.volume_id \
         ORDER BY s.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct CreateSnapshotBody {
    pub name: String,
}

pub async fn create_volume_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(volume_id): Path<Uuid>,
    Json(body): Json<CreateSnapshotBody>,
) -> Result<Json<VolumeSnapshotRow>, ApiError> {
    require_operator(&actor)?;
    let atlas_volume_id: Option<String> =
        crate::db::query_scalar("SELECT atlas_volume_id FROM volumes WHERE id = ?")
            .bind(volume_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
    let atlas_volume_id = atlas_volume_id.ok_or_else(|| {
        ApiError::bad_request("snapshots require an Atlas-backed volume (ATLAS_ENABLED=1)")
    })?;

    let client = atlas_bridge::require_client(&state.config)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let job = client
        .snapshot_volume(&atlas_volume_id, Some(&body.name))
        .await
        .map_err(|e| ApiError::internal(format!("Atlas snapshot failed: {e}")))?;
    // `resource` (not `result`) carries the created object's id, matching
    // AtlasJob::resource_volume_id/resource_backup_id's convention.
    let atlas_snapshot_id = job
        .resource
        .get("snapshot_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO volume_snapshots (id, volume_id, name, atlas_snapshot_id) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(volume_id)
    .bind(&body.name)
    .bind(&atlas_snapshot_id)
    .execute(&state.pool)
    .await?;
    Ok(Json(VolumeSnapshotRow {
        id,
        volume_id,
        name: body.name,
        status: "available".into(),
    }))
}

pub async fn delete_volume_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let atlas_snapshot_id: Option<String> =
        crate::db::query_scalar("SELECT atlas_snapshot_id FROM volume_snapshots WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
    if let Some(snap_id) = atlas_snapshot_id {
        let client = atlas_bridge::require_client(&state.config)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let _ = client.delete_snapshot(&snap_id, false).await;
    }
    crate::db::query("DELETE FROM volume_snapshots WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Deserialize)]
pub struct DeleteOnTerminationBody {
    pub value: bool,
}

pub async fn set_volume_delete_on_termination(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<DeleteOnTerminationBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    let r = crate::db::query("UPDATE volumes SET delete_on_termination = ? WHERE id = ?")
        .bind(body.value)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::not_found("volume not found"));
    }
    get_volume(State(state), Extension(actor), Path(id)).await
}

/// Per-volume I/O limits. `0` removes a limit; an omitted field is left as it is.
#[derive(Debug, Deserialize)]
pub struct IoTuneBody {
    #[serde(default)]
    pub read_iops: Option<u64>,
    #[serde(default)]
    pub write_iops: Option<u64>,
    #[serde(default)]
    pub read_bps: Option<u64>,
    #[serde(default)]
    pub write_bps: Option<u64>,
}

const MAX_IOPS: u64 = 10_000_000;
const MAX_BPS: u64 = 100_000_000_000;

pub(crate) fn validate_iotune(b: &IoTuneBody) -> Result<(), String> {
    if b.read_iops.is_none() && b.write_iops.is_none() && b.read_bps.is_none() && b.write_bps.is_none() {
        return Err("give at least one of read_iops, write_iops, read_bps, write_bps".into());
    }
    if [b.read_iops, b.write_iops].iter().flatten().any(|v| *v > MAX_IOPS) {
        return Err(format!("iops limits are at most {MAX_IOPS}"));
    }
    if [b.read_bps, b.write_bps].iter().flatten().any(|v| *v > MAX_BPS) {
        return Err(format!("bytes-per-second limits are at most {MAX_BPS}"));
    }
    Ok(())
}

pub async fn set_volume_iotune(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<IoTuneBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    validate_iotune(&body).map_err(ApiError::bad_request)?;
    let row: Option<(Option<Uuid>, Option<String>)> =
        crate::db::query_as("SELECT attached_vm_id, attached_device FROM volumes WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((vm, dev)) = row else {
        return Err(ApiError::not_found("volume not found"));
    };
    if let (Some(vm), Some(dev)) = (vm, dev) {
        let (name, host): (String, Option<Uuid>) =
            crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
                .bind(vm)
                .fetch_one(&state.pool)
                .await?;
        let host = host.ok_or_else(|| ApiError::conflict("the instance has no host", "start it first"))?;
        let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
            .bind(host)
            .fetch_one(&state.pool)
            .await?;
        let mut client = crate::agent_client::connect(&addr)
            .await
            .map_err(|e| ApiError::internal(format!("agent unreachable: {e:#}")))?;
        crate::agent_client::vm_libvirt_invoke(
            &mut client,
            &name,
            "disk.iotune",
            &serde_json::json!({
                "target": dev, "read_iops": body.read_iops, "write_iops": body.write_iops,
                "read_bps": body.read_bps, "write_bps": body.write_bps,
            }),
        )
        .await
        .map_err(|e| ApiError::internal(format!("applying the limits failed: {e:#}")))?;
    }
    crate::db::query(
        "UPDATE volumes SET read_iops = COALESCE(?, read_iops), write_iops = COALESCE(?, write_iops), \
         read_bps = COALESCE(?, read_bps), write_bps = COALESCE(?, write_bps) WHERE id = ?",
    )
    .bind(body.read_iops.map(|v| v as i64))
    .bind(body.write_iops.map(|v| v as i64))
    .bind(body.read_bps.map(|v| v as i64))
    .bind(body.write_bps.map(|v| v as i64))
    .bind(id)
    .execute(&state.pool)
    .await?;
    get_volume(State(state), Extension(actor), Path(id)).await
}

/// Called by the VM delete task after the domain is gone and before the instance row is removed:
/// deletes the volumes flagged `delete_on_termination` that were attached to it. Failures are logged and
/// leave the volume behind (detached) rather than failing the instance delete.
pub(crate) async fn purge_terminating_volumes(state: &AppState, vm_id: Uuid, host: Option<Uuid>) {
    let rows: Vec<(Uuid, Option<String>, Option<Uuid>)> = crate::db::query_as(
        "SELECT id, atlas_volume_id, storage_pool_id FROM volumes WHERE attached_vm_id = ? AND delete_on_termination = TRUE",
    )
    .bind(vm_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();
    for (id, atlas, pool) in rows {
        let result: Result<(), String> = if let Some(a) = atlas {
            match atlas_bridge::require_client(&state.config) {
                Ok(c) => c.delete_volume(&a).await.map(|_| ()).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        } else if let (Some(pool), Some(host)) = (pool, host) {
            storage::delete_pool_volume_file(state, host, pool, &format!("vol-{id}"))
                .await
                .map_err(|e| e.message)
        } else {
            Ok(())
        };
        match result {
            Ok(()) => {
                let _ = crate::db::query("DELETE FROM volumes WHERE id = ?").bind(id).execute(&state.pool).await;
                state.emit_event("volume.delete", format!("Volume {id} deleted with its instance"));
            }
            Err(e) => tracing::warn!(volume = %id, "delete on termination failed: {e}"),
        }
    }
}

#[cfg(test)]
mod iotune_tests {
    use super::*;

    fn b(r: Option<u64>) -> IoTuneBody {
        IoTuneBody { read_iops: r, write_iops: None, read_bps: None, write_bps: None }
    }

    #[test]
    fn needs_a_limit_and_bounds_it() {
        assert!(validate_iotune(&b(None)).is_err());
        assert!(validate_iotune(&b(Some(0))).is_ok());
        assert!(validate_iotune(&b(Some(MAX_IOPS + 1))).is_err());
        let big = IoTuneBody { read_iops: None, write_iops: None, read_bps: None, write_bps: Some(MAX_BPS + 1) };
        assert!(validate_iotune(&big).is_err());
    }
}

#[derive(Debug, Deserialize)]
pub struct VolumeFromSnapshotBody {
    pub name: String,
}

/// EC2 `CreateVolume` with `SnapshotId`: a new volume (same size, class and project as the source) cloned from an
/// Atlas snapshot.
pub async fn create_volume_from_snapshot(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(snapshot_id): Path<Uuid>,
    Json(body): Json<VolumeFromSnapshotBody>,
) -> Result<Json<VolumeRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let snap: Option<(Option<String>, Uuid)> =
        crate::db::query_as("SELECT atlas_snapshot_id, volume_id FROM volume_snapshots WHERE id = ?")
            .bind(snapshot_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((atlas_snapshot, parent)) = snap else {
        return Err(ApiError::not_found("snapshot not found"));
    };
    let atlas_snapshot = atlas_snapshot
        .ok_or_else(|| ApiError::bad_request("this snapshot has no Atlas snapshot behind it"))?;
    let (project_id, size_gib, class): (Option<Uuid>, i64, String) =
        crate::db::query_as("SELECT project_id, size_gib, volume_class FROM volumes WHERE id = ?")
            .bind(parent)
            .fetch_one(&state.pool)
            .await?;
    let client = atlas_bridge::require_client(&state.config)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO volumes (id, project_id, name, size_gib, volume_class, status) VALUES (?, ?, ?, ?, ?, 'creating')",
    )
    .bind(id)
    .bind(project_id)
    .bind(&body.name)
    .bind(size_gib)
    .bind(&class)
    .execute(&state.pool)
    .await?;
    let cloned = async {
        let job = client
            .clone_snapshot(&atlas_snapshot, &atlas_safe_name(&body.name), None)
            .await
            .map_err(|e| ApiError::internal(format!("Atlas snapshot clone failed: {e}")))?;
        let vol = job
            .resource_volume_id()
            .ok_or_else(|| ApiError::internal("Atlas clone returned no volume_id"))?;
        if let Some(jid) = job.job_id() {
            let _ = client.wait_for_job(jid, Duration::from_secs(60)).await;
        }
        Ok::<String, ApiError>(vol)
    }
    .await;
    match cloned {
        Ok(vol) => {
            crate::db::query("UPDATE volumes SET status = 'available', atlas_volume_id = ? WHERE id = ?")
                .bind(&vol)
                .bind(id)
                .execute(&state.pool)
                .await?;
        }
        Err(e) => {
            crate::db::query("UPDATE volumes SET status = 'error' WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await?;
            return Err(e);
        }
    }
    get_volume(State(state), Extension(actor), Path(id)).await
}
