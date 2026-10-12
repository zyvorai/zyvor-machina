// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// VM-oriented Atlas orchestration: provision Atlas backend volumes bound to a
// machina VM, and route that VM's snapshot / backup through the Atlas control
// plane. The `vm_atlas_volumes` table records the binding (see migration 008).

use std::time::Duration;

use crate::db::DbPool;
use serde::Serialize;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::engine::atlas_bridge::{self, AtlasJob, AtlasOwner};
use crate::state::AppState;

/// One Atlas-backed disk bound to a VM.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct VmAtlasVolume {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub volume_id: String,
    pub role: String,
    pub size_bytes: i64,
    pub policy: String,
    pub backend_native_id: Option<String>,
    pub state: String,
    pub created_at: String,
}

/// Atlas records volume sizes in bytes; machina UIs speak GiB.
const GIB: i64 = 1024 * 1024 * 1024;

/// Coerce a string into an RFC 1123 label usable as a Kubernetes PVC name:
/// lowercase, only `[a-z0-9-]`, no leading/trailing `-`. Atlas uses the volume
/// name verbatim as the PVC name, which k8s rejects otherwise (e.g. `root_disk`).
fn dns_safe(s: &str) -> String {
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
        // k8s label max is 63 chars.
        trimmed.chars().take(63).collect()
    }
}

pub async fn list_vm_volumes(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<Vec<VmAtlasVolume>> {
    let rows = crate::db::query_as::<_, VmAtlasVolume>(
        "SELECT id, vm_id, volume_id, role, size_bytes, policy, backend_native_id, state, created_at \
         FROM vm_atlas_volumes WHERE vm_id = ? ORDER BY created_at",
    )
    .bind(vm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn vm_name(pool: &DbPool, vm_id: Uuid) -> anyhow::Result<String> {
    let name: Option<String> = crate::db::query_scalar("SELECT name FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?;
    name.ok_or_else(|| anyhow::anyhow!("vm not found"))
}

/// Provision an Atlas volume and record it as a VM disk binding. The volume is
/// created with an owner binding (`machina/virtual_machine/<vm_id>/<role>`) so
/// Atlas can enumerate machina's volumes. Best-effort waits for the create job
/// to finish so the backend-native (RBD pool/image) reference can be captured
/// for domain-XML attach; the binding is recorded either way.
pub async fn provision_vm_volume(
    state: &AppState,
    vm_id: Uuid,
    size_gib: i64,
    policy: Option<&str>,
    role: &str,
    name: Option<&str>,
) -> anyhow::Result<VmAtlasVolume> {
    let cfg = &state.config;
    let client = atlas_bridge::require_client(cfg)?;

    // Refuse to start a volume Atlas cannot hold, instead of failing halfway through. A summary
    // that cannot be read does not block (see `headroom_blocker`).
    if let Ok(summary) = client.metrics_summary().await {
        if let Some(why) = atlas_bridge::headroom_blocker(
            &summary,
            size_gib.max(1) * GIB,
            atlas_bridge::max_used_ratio(),
        ) {
            anyhow::bail!(why);
        }
    }

    let vm = vm_name(&state.pool, vm_id).await?;
    // Atlas uses the volume name as the Kubernetes PVC name, which must be an
    // RFC 1123 label (lowercase alphanumeric + '-'). The role ("root_disk")
    // contains an underscore, so sanitize the generated name.
    let vol_name = dns_safe(
        &name
            .map(str::to_string)
            .unwrap_or_else(|| format!("{vm}-{role}")),
    );
    let policy = policy.unwrap_or(&cfg.atlas_default_policy).to_string();
    let size_bytes = size_gib.max(1) * GIB;

    let owner = AtlasOwner {
        product: "machina".into(),
        resource_type: "virtual_machine".into(),
        resource_id: vm_id.to_string(),
        role: role.to_string(),
    };

    let job = client
        .create_volume(
            &cfg.atlas_tenant_id,
            &vol_name,
            size_bytes,
            &policy,
            Some(&owner),
            None,
            None,
        )
        .await?;

    // The create response carries the volume id in `resource.volume_id`.
    let volume_id = job
        .resource_volume_id()
        .or_else(|| {
            job.result
                .get("volume_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("Atlas create-volume returned no volume_id"))?;

    // Wait for provisioning and capture the backend-native id (RBD pool/image)
    // needed to attach the volume to the domain. CSI provisioning can be slow, so
    // wait generously and then poll a few times — the volume row (with its
    // resolved rbd pool/image) can appear a moment after the job completes.
    let (backend_native_id, vol_state) = match job.job_id() {
        Some(jid) => {
            let _ = client.wait_for_job(jid, Duration::from_secs(120)).await;
            let mut native = None;
            let mut state = "provisioning".to_string();
            for attempt in 0..8 {
                if let Ok(v) = client.get_volume(&volume_id).await {
                    if let Some(s) = v.state {
                        state = s;
                    }
                    if v.backend_native_id.is_some() {
                        native = v.backend_native_id;
                        break;
                    }
                }
                if attempt < 7 {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
            (native, state)
        }
        None => (None, "provisioning".into()),
    };

    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO vm_atlas_volumes \
         (id, vm_id, volume_id, role, size_bytes, policy, backend_native_id, state) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(vm_id)
    .bind(&volume_id)
    .bind(role)
    .bind(size_bytes)
    .bind(&policy)
    .bind(&backend_native_id)
    .bind(&vol_state)
    .execute(&state.pool)
    .await?;

    state.emit_event(
        "atlas.volume.provisioned",
        format!("Atlas volume {volume_id} bound to VM {vm} as {role}"),
    );

    Ok(VmAtlasVolume {
        id,
        vm_id,
        volume_id,
        role: role.to_string(),
        size_bytes,
        policy,
        backend_native_id,
        state: vol_state,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Snapshot every Atlas-backed disk of a VM via the Atlas control plane.
///
/// Returns the freshly-enqueued jobs (state `queued`/`running`, not yet
/// terminal) — same "fire and let the caller poll `/atlas/jobs/{id}`"
/// contract as the plain (non-VM) `POST /atlas/volumes/{id}/snapshots`
/// endpoint. Callers that need to know the *actual* outcome (e.g. the
/// `vm.snapshot` task, which persists a `snapshot_records.status`) must poll
/// each job to a terminal state themselves before recording success —
/// treating this function's return as "done" is the false-success bug.
pub async fn snapshot_vm(
    state: &AppState,
    vm_id: Uuid,
    name: Option<&str>,
) -> anyhow::Result<Vec<AtlasJob>> {
    let client = atlas_bridge::require_client(&state.config)?;
    let volumes = list_vm_volumes(&state.pool, vm_id).await?;
    if volumes.is_empty() {
        anyhow::bail!("VM has no Atlas-backed volumes to snapshot");
    }
    let mut jobs = Vec::with_capacity(volumes.len());
    for v in &volumes {
        jobs.push(client.snapshot_volume(&v.volume_id, name).await?);
    }
    state.emit_event(
        "atlas.vm.snapshot",
        format!("Snapshotted {} Atlas volume(s) for VM {vm_id}", jobs.len()),
    );
    Ok(jobs)
}

/// Back up every Atlas-backed disk of a VM to a bound RGW bucket. `bucket_id`
/// falls back to `ATLAS_BACKUP_BUCKET_ID`.
pub async fn backup_vm(
    state: &AppState,
    vm_id: Uuid,
    bucket_id: Option<&str>,
    mode: &str,
    keep: i64,
) -> anyhow::Result<Vec<AtlasJob>> {
    let cfg = &state.config;
    let client = atlas_bridge::require_client(cfg)?;
    let bucket = bucket_id
        .map(str::to_string)
        .or_else(|| cfg.atlas_backup_bucket_id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("no bucket_id given and ATLAS_BACKUP_BUCKET_ID is not configured")
        })?;

    let volumes = list_vm_volumes(&state.pool, vm_id).await?;
    if volumes.is_empty() {
        anyhow::bail!("VM has no Atlas-backed volumes to back up");
    }
    let mut jobs = Vec::with_capacity(volumes.len());
    for v in &volumes {
        jobs.push(
            client
                .backup_volume(&v.volume_id, &bucket, mode, keep)
                .await?,
        );
    }
    state.emit_event(
        "atlas.vm.backup",
        format!("Backed up {} Atlas volume(s) for VM {vm_id}", jobs.len()),
    );
    Ok(jobs)
}

/// Build the libvirt disk `source` URI for an Atlas RBD volume from its
/// backend-native id (`pool/image`) plus cluster-wide Ceph connection params.
/// `translate::domain_xml` parses this into a `<disk type='network'>` block.
pub fn rbd_source(cfg: &ControllerConfig, backend_native_id: &str) -> String {
    let mut q: Vec<String> = Vec::new();
    let mons = cfg.atlas_rbd_mon_hosts.trim();
    if !mons.is_empty() {
        q.push(format!("mon={mons}"));
    }
    if let Some(u) = cfg.atlas_rbd_auth_user.as_deref().filter(|s| !s.is_empty()) {
        q.push(format!("auth={u}"));
    }
    if let Some(s) = cfg
        .atlas_rbd_secret_uuid
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        q.push(format!("secret={s}"));
    }
    if q.is_empty() {
        format!("rbd:{backend_native_id}")
    } else {
        format!("rbd:{backend_native_id}?{}", q.join("&"))
    }
}

/// True if the VM has at least one Atlas-backed disk — used by the generic
/// snapshot/backup handlers to route through Atlas instead of the agent.
pub async fn vm_is_atlas_backed(pool: &DbPool, vm_id: Uuid) -> bool {
    crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM vm_atlas_volumes WHERE vm_id = ?")
        .bind(vm_id)
        .fetch_one(pool)
        .await
        .map(|c| c > 0)
        .unwrap_or(false)
}
