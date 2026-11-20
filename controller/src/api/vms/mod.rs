// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

mod batch;
mod graphics;
mod port_forwards;

pub use batch::*;
pub use graphics::*;
pub use port_forwards::*;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use axum::Extension;
use axum::Json;
use machina_spec::{CloudInitSpec, VirtualMachine};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::tasks::TaskResponse;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::migrate_precheck::run_migrate_precheck;
use crate::engine::placement::pick_host_for_vm;
use crate::engine::policy;
use crate::engine::template::upsert_ha_policy;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VmRow {
    pub id: Uuid,
    /// EC2-style id (`i-0123456789abcdef0`), derived from the UUID; filled in after the row is read.
    #[sqlx(skip)]
    pub ec2_id: String,
    pub name: String,
    pub host_id: Option<Uuid>,
    pub desired_state: String,
    pub observed_state: String,
    pub lifecycle_phase: String,
    pub last_error: String,
    pub managed: bool,
    pub uuid: Option<String>,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub ha_enabled: bool,
    pub project: Option<String>,
    pub tags: crate::db::Json<Vec<String>>,
    pub inventory_source: String,
    pub k8s_namespace: Option<String>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub guest_ip: Option<String>,
    pub guest_tools_status: Option<String>,
    /// The instance type (flavor) the machine was launched as or last changed to, if known.
    pub flavor_id: Option<Uuid>,
}

impl VmRow {
    fn with_ec2_id(mut self) -> Self {
        self.ec2_id = crate::resource_ids::ec2_id(crate::resource_ids::Kind::Vm, self.id);
        self
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct VmListQuery {
    /// Only machines carrying this key/value tag (`resource_tags`); `tag_value` is optional.
    #[serde(default)]
    pub tag_key: Option<String>,
    #[serde(default)]
    pub tag_value: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default)]
    pub managed: Option<bool>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    /// VM status/type filter (running, stopped, discovered, untagged, etc.)
    #[serde(default, alias = "folder")]
    pub status: Option<String>,
}

pub async fn list_vms(
    State(state): State<AppState>,
    Query(q): Query<VmListQuery>,
) -> Result<Json<Vec<VmRow>>, ApiError> {
    let rows = crate::db::query_as::<_, VmRow>(
        "SELECT v.id, v.name, v.host_id, v.desired_state, v.observed_state,
                COALESCE(v.lifecycle_phase, 'idle') AS lifecycle_phase,
                COALESCE(v.last_error, '') AS last_error,
                COALESCE(v.managed, TRUE) AS managed,
                v.uuid, v.vcpus, v.memory_mib,
                COALESCE(hp.enabled, FALSE) AS ha_enabled, v.project, COALESCE(v.tags, '[]') AS tags,
                COALESCE(v.inventory_source, 'libvirt') AS inventory_source,
                v.k8s_namespace, v.last_seen_at, v.guest_ip, v.guest_tools_status, v.flavor_id
         FROM vms v LEFT JOIN ha_policies hp ON hp.vm_id = v.id
         LEFT JOIN vm_metrics m ON m.vm_id = v.id
         WHERE (?1 IS NULL OR v.project = ?1)
           AND (?7 IS NULL OR EXISTS (
                 SELECT 1 FROM resource_tags rt
                 WHERE rt.resource_type = 'vm' AND rt.resource_id = lower(hex(v.id))
                   AND rt.key = ?7 AND (?8 IS NULL OR rt.value = ?8)))
           AND (?2 IS NULL OR v.host_id = ?2)
           AND (?3 IS NULL OR v.managed = ?3)
           AND (?4 IS NULL OR EXISTS (SELECT 1 FROM json_each(COALESCE(v.tags,'[]')) WHERE value = ?4))
           AND (?5 IS NULL OR v.inventory_source = ?5)
           AND (
             ?6 IS NULL
             OR (?6 = 'running' AND v.observed_state = 'running')
             OR (?6 = 'stopped' AND v.observed_state IN ('shutoff', 'stopped'))
             OR (?6 = 'discovered' AND v.managed = FALSE)
             OR (?6 = 'missing' AND v.observed_state = 'missing')
             OR (?6 = 'untagged' AND (v.tags IS NULL OR v.tags = '[]'))
             OR (?6 = 'high_cpu' AND m.cpu_percent > 85)
             OR (?6 = 'unprotected' AND NOT EXISTS (
               SELECT 1 FROM backup_records b
               WHERE b.vm_id = v.id AND b.status = 'completed'
                 AND b.created_at > datetime('now', '-7 days')
             ))
             OR (?6 = 'no_ip' AND (v.guest_ip IS NULL OR v.guest_ip = ''))
             OR (?6 = 'guest_agent_missing' AND COALESCE(v.inventory_source, 'libvirt') != 'kubevirt'
               AND (v.guest_tools_status IS NULL OR v.guest_tools_status NOT IN ('healthy', 'installed')))
             OR (?6 = 'migration_ready' AND v.observed_state NOT IN ('running', 'missing')
               AND COALESCE(v.managed, TRUE) = TRUE)
             OR (?6 = 'needs_attention' AND (
               NOT EXISTS (
                 SELECT 1 FROM backup_records b
                 WHERE b.vm_id = v.id AND b.status = 'completed'
                   AND b.created_at > datetime('now', '-7 days')
               )
               OR (v.guest_ip IS NULL OR v.guest_ip = '')
               OR (COALESCE(v.inventory_source, 'libvirt') != 'kubevirt'
                 AND (v.guest_tools_status IS NULL OR v.guest_tools_status NOT IN ('healthy', 'installed')))
               OR (m.cpu_percent > 85)
             ))
             OR (?6 = 'ha_enabled' AND hp.enabled = TRUE)
             OR ?6 = 'all'
           )
         ORDER BY v.name LIMIT 2000",
    )
    .bind(q.project.as_deref())
    .bind(q.host_id)
    .bind(q.managed)
    .bind(q.tag.as_deref())
    .bind(q.source.as_deref())
    .bind(q.status.as_deref())
    .bind(q.tag_key.as_deref())
    .bind(q.tag_value.as_deref())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(VmRow::with_ec2_id).collect()))
}

pub async fn get_vm(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<VmRow>, ApiError> {
    let row = crate::db::query_as::<_, VmRow>(
        "SELECT v.id, v.name, v.host_id, v.desired_state, v.observed_state,
                COALESCE(v.lifecycle_phase, 'idle') AS lifecycle_phase,
                COALESCE(v.last_error, '') AS last_error,
                COALESCE(v.managed, TRUE) AS managed,
                v.uuid, v.vcpus, v.memory_mib,
                COALESCE(hp.enabled, FALSE) AS ha_enabled, v.project, COALESCE(v.tags, '[]') AS tags,
                COALESCE(v.inventory_source, 'libvirt') AS inventory_source,
                v.k8s_namespace, v.last_seen_at, v.guest_ip, v.guest_tools_status, v.flavor_id
         FROM vms v LEFT JOIN ha_policies hp ON hp.vm_id = v.id
         WHERE v.id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row.with_ec2_id()))
}

pub async fn get_vm_spec(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let spec: serde_json::Value = crate::db::query_scalar("SELECT spec_json FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(spec))
}

#[derive(Debug, Deserialize)]
pub struct CreateVmBody {
    #[serde(flatten)]
    pub vm: VirtualMachine,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_desired")]
    pub desired_state: String,
    /// Provision the VM's root disk as an Atlas backend volume (Ceph RBD) instead
    /// of a local qcow2 file, and attach it as a libvirt network disk.
    #[serde(default)]
    pub atlas_root_disk: bool,
    /// The flavor (instance type) this machine is launched as; recorded so it can be shown and changed later.
    #[serde(default)]
    pub flavor_id: Option<Uuid>,
    /// Atlas intent → placement policy for the root volume (default from config).
    #[serde(default)]
    pub atlas_policy: Option<String>,
    /// Sleeps (memory saved) instead of running when capacity is short.
    #[serde(default)]
    pub preemptible: bool,
    /// 0..=100; lower is preempted first.
    #[serde(default)]
    pub preempt_priority: i64,
}

fn default_desired() -> String {
    "running".into()
}

pub async fn create_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateVmBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    // Stricter than the general `validate()`: this runs on the caller's raw,
    // pre-Atlas-resolution body, so an inline Ceph auth/secret param here can
    // only be something the operator typed themselves, not a legitimate
    // server-resolved value — reject it outright rather than merely checking
    // it's a well-formed UUID (see `validate_operator_submission` doc comment).
    body.vm
        .validate_operator_submission()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::bad_request("no cluster configured — add a host first"))?;

    let vcpus = body.vm.total_vcpus() as i32;
    let memory_mib = body
        .vm
        .memory_mib()
        .map_err(|e| ApiError::bad_request(e.to_string()))? as i64;
    let mut storage_gib: i64 = 0;
    for vol in &body.vm.spec.storage {
        storage_gib += machina_spec::parse_size_gib(&vol.size)
            .map_err(|e| ApiError::bad_request(e.to_string()))? as i64;
    }
    let project = body
        .vm
        .metadata
        .project
        .clone()
        .unwrap_or_else(|| "default".into());
    if let Some(tref) = body.vm.spec.template_ref.as_deref() {
        if let Some(why) = crate::engine::template::image_access_error(&state.pool, tref, &project)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            return Err(ApiError::forbidden(why).with_code("image_not_shared"));
        }
    }
    if !(0..=crate::engine::preempt::MAX_PRIORITY).contains(&body.preempt_priority) {
        return Err(ApiError::bad_request("preempt_priority must be 0..=100"));
    }
    let host_id = match body.host_id {
        Some(id) => id,
        None => match pick_host_for_vm(&state.pool, &body.tags, memory_mib).await {
            Ok(id) => id,
            Err(e) => {
                let room = if body.preemptible {
                    None
                } else {
                    crate::engine::preempt::make_room(&state, memory_mib, &body.vm.metadata.name)
                        .await
                        .map_err(|e| ApiError::internal(e.to_string()))?
                };
                room.ok_or_else(|| ApiError::bad_request(e.to_string()))?
            }
        },
    };

    for network in &body.vm.spec.network {
        let cloud_owned = crate::api::cloud::authorize_attachment(
            &state,
            &actor,
            &network.network,
            &project,
            Some(host_id),
        )
        .await?;
        if cloud_owned && body.vm.spec.ha.enabled {
            return Err(ApiError::bad_request(
                "host-local cloud subnets do not support cross-host HA",
            ));
        }
    }
    let vm_id = Uuid::new_v4();
    let spec_json =
        serde_json::to_value(&body.vm).map_err(|e| ApiError::internal(e.to_string()))?;

    // Quota and policy are checked inside this same write transaction (BEGIN IMMEDIATE takes the SQLite write lock
    // up front), so concurrent creates at a project's limit are serialised and cannot both pass.
    let mut tx = crate::db::begin_write(&state.pool).await?;
    if let Err(v) = policy::evaluate_vm_create_tx(
        &mut tx,
        &project,
        &body.tags,
        vcpus,
        memory_mib,
        storage_gib,
        body.vm.spec.ha.enabled,
    )
    .await
    {
        return Err(ApiError::policy_violation(v.message, v.remediation));
    }

    crate::db::query(
        "INSERT INTO vms (id, cluster_id, host_id, name, project, spec_json, desired_state, lifecycle_phase, vcpus, memory_mib, tags, flavor_id, preemptible, preempt_priority)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'creating', ?, ?, ?, ?, ?, ?)",
    )
    .bind(vm_id)
    .bind(cluster_id)
    .bind(host_id)
    .bind(&body.vm.metadata.name)
    // Store the SAME resolved project the quota policy was evaluated against.
    // Binding the raw Option left project=NULL when metadata.project was None
    // (evaluated as "default"), so the VM then failed project=default list
    // filters and per-project quota accounting.
    .bind(&project)
    .bind(&spec_json)
    .bind(&body.desired_state)
    .bind(vcpus)
    .bind(memory_mib)
    .bind(serde_json::to_string(&body.tags).unwrap_or_else(|_| "[]".into()))
    .bind(body.flavor_id)
    .bind(body.preemptible)
    .bind(body.preempt_priority)
    .execute(&mut *tx)
    .await?;

    for vol in &body.vm.spec.storage {
        let size_gib = machina_spec::parse_size_gib(&vol.size)
            .map_err(|e| ApiError::bad_request(e.to_string()))? as i64;
        crate::db::query(
            "INSERT INTO vm_disks (id, vm_id, name, size_gib, storage_class) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(vm_id)
        .bind(&vol.name)
        .bind(size_gib)
        .bind(&vol.class)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Atlas-backed root disk: provision an Atlas backend volume bound to this VM
    // and rewrite the stored spec so `vm.apply` attaches it as an RBD network
    // disk. Done after commit so the volume's FK/owner binding resolves.
    if body.atlas_root_disk {
        let root_size_gib = body
            .vm
            .spec
            .storage
            .first()
            .map(|s| s.size.clone())
            .and_then(|s| machina_spec::parse_size_gib(&s).ok())
            .unwrap_or(10) as i64;
        match crate::engine::atlas_vm::provision_vm_volume(
            &state,
            vm_id,
            root_size_gib,
            body.atlas_policy.as_deref(),
            "root_disk",
            None,
        )
        .await
        {
            Ok(vol) => {
                let native = vol.backend_native_id.as_deref().filter(|s| !s.is_empty());
                match native {
                    Some(native) => {
                        let source = crate::engine::atlas_vm::rbd_source(&state.config, native);
                        let mut spec = spec_json.clone();
                        if let Some(first) = spec
                            .pointer_mut("/spec/storage")
                            .and_then(|v| v.as_array_mut())
                            .and_then(|a| a.first_mut())
                        {
                            first["source"] = serde_json::json!(source);
                        }
                        // This UPDATE must actually land: `vm.apply` reads spec_json
                        // straight off this row, so a swallowed failure here (the
                        // previous `let _ =`) would leave the stored spec pointing at
                        // no/local source while the real data lives in the Atlas
                        // volume — the VM boots wrong and the volume is orphaned.
                        crate::db::query("UPDATE vms SET spec_json = ? WHERE id = ?")
                            .bind(&spec)
                            .bind(vm_id)
                            .execute(&state.pool)
                            .await
                            .map_err(|e| {
                                ApiError::internal(format!(
                                    "failed to persist Atlas-backed disk source: {e}"
                                ))
                            })?;
                    }
                    None => {
                        // Atlas accepted the request but hadn't bound a backend id by
                        // the time provision_vm_volume's retry window elapsed (volume
                        // still "provisioning"). Proceeding here would silently apply
                        // the VM against its original (non-Atlas) disk source while
                        // paying for an Atlas volume the VM never attaches to — the
                        // same silent-fallback this function's Err(e) arm explicitly
                        // refuses to do. Fail the same way.
                        let _ = crate::db::query("DELETE FROM vm_disks WHERE vm_id = ?")
                            .bind(vm_id)
                            .execute(&state.pool)
                            .await;
                        let _ = crate::db::query("DELETE FROM vms WHERE id = ?")
                            .bind(vm_id)
                            .execute(&state.pool)
                            .await;
                        return Err(ApiError::internal(
                            "Atlas volume did not finish binding a backend id in time",
                        )
                        .with_code("atlas_provision_failed")
                        .with_remediation(
                            "Check the Atlas gateway/backend status, then retry VM creation.",
                        ));
                    }
                }
            }
            Err(e) => {
                // The operator explicitly asked for Atlas storage; don't silently
                // fall back to a local disk. Roll back the VM row and fail.
                let _ = crate::db::query("DELETE FROM vm_disks WHERE vm_id = ?")
                    .bind(vm_id)
                    .execute(&state.pool)
                    .await;
                let _ = crate::db::query("DELETE FROM vms WHERE id = ?")
                    .bind(vm_id)
                    .execute(&state.pool)
                    .await;
                return Err(ApiError::internal(format!(
                    "Atlas volume provisioning failed: {e}"
                ))
                .with_code("atlas_provision_failed")
                .with_remediation(
                    "Check the Atlas gateway and ATLAS_* config, or create the VM without Atlas storage.",
                ));
            }
        }
    }

    if body.vm.spec.ha.enabled {
        upsert_ha_policy(
            &state.pool,
            vm_id,
            true,
            3,
            body.vm
                .spec
                .ha
                .restart_priority
                .as_deref()
                .unwrap_or("medium"),
            false,
            body.vm.spec.ha.anti_affinity.is_some(),
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    let task_id = enqueue_task(
        &state,
        "vm.apply",
        serde_json::json!({
            "vm_id": vm_id.to_string(),
            "host_id": host_id.to_string(),
        }),
        Some("vm"),
        Some(vm_id),
        Some(host_id),
    )
    .await
    .inspect_err(|_e| {
        // Compensate: delete the zombie VM row so the name is free to retry.
        let pool = state.pool.clone();
        tokio::spawn(async move {
            let _ = crate::db::query("DELETE FROM vm_disks WHERE vm_id = ?")
                .bind(vm_id)
                .execute(&pool)
                .await;
            let _ = crate::db::query("DELETE FROM vms WHERE id = ?")
                .bind(vm_id)
                .execute(&pool)
                .await;
        });
    })?;

    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(&actor.username)
    .bind("vm.create")
    .bind("vm")
    .bind(vm_id)
    .bind(serde_json::json!({ "name": body.vm.metadata.name }))
    .execute(&state.pool)
    .await?;

    if let Some(net) = body.vm.spec.network.first() {
        if let Some(profile) = &net.firewall_profile {
            let _ = crate::db::query(
                "INSERT INTO firewall_timeline (id, target_kind, target_id, kind, summary, detail_json, actor) VALUES (?, 'vm', ?, 'profile_requested', ?, ?, ?)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(vm_id)
            .bind(format!("VM network requests firewall profile {profile}"))
            .bind(serde_json::json!({ "profile": profile, "host_id": host_id.to_string(), "network": net.network }))
            .bind(&actor.username)
            .execute(&state.pool)
            .await;
        }
    }

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.apply".into(),
    }))
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateFromTemplateBody {
    pub template_ref: String,
    pub name: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default = "default_desired")]
    pub desired_state: String,
    #[serde(default)]
    pub cloud_init_user: Option<String>,
    #[serde(default)]
    pub cloud_init_password: Option<String>,
    #[serde(default)]
    pub cloud_init_ssh_pubkey: Option<String>,
    /// Name of a saved key pair whose public key is injected (EC2 `KeyName`); giving it together with `cloud_init_ssh_pubkey` is an error.
    #[serde(default)]
    pub key_name: Option<String>,
    /// Free-form cloud-init user-data (a `#cloud-config` document or script), at most 16 KiB; passed to the guest verbatim.
    #[serde(default)]
    pub cloud_init_user_data: Option<String>,
    /// Substituted into name and cloud-init fields as `{{ key }}`.
    #[serde(default)]
    pub template_vars: std::collections::HashMap<String, String>,
    /// When set, overrides `memory` above and sets vCPU count — resolved from the
    /// native flavor catalog (`api::flavors`) rather than the caller naming raw
    /// cpu/memory values directly, mirroring how a Nova instance names a flavor
    /// rather than raw hardware sizing.
    #[serde(default)]
    pub flavor_id: Option<Uuid>,
    /// Network name (matches `api::networks`) for the primary NIC. Omit to keep
    /// `VirtualMachine::new`'s default network.
    #[serde(default)]
    pub network: Option<String>,
    /// Scale-to-zero policy (see `set_vm_sleep_policy`); omit to inherit the project default.
    #[serde(default)]
    pub sleep_after_minutes: Option<i64>,
    #[serde(default)]
    pub preemptible: bool,
    #[serde(default)]
    pub preempt_priority: i64,
}

fn default_memory() -> String {
    "4Gi".into()
}

pub async fn create_from_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateFromTemplateBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    if let Some(m) = body.sleep_after_minutes {
        if !(0..=10_080).contains(&m)
            || (1..crate::engine::vm_sleep::MIN_SLEEP_AFTER_MINUTES).contains(&m)
        {
            return Err(ApiError::bad_request(format!(
                "sleep_after_minutes must be 0 or between {} and 10080",
                crate::engine::vm_sleep::MIN_SLEEP_AFTER_MINUTES
            )));
        }
    }
    let sleep_after = body.sleep_after_minutes;
    let pool = state.pool.clone();
    let apply = |s: &str| crate::engine::template::apply_template_vars(s, &body.template_vars);
    let name = apply(&body.name);
    machina_spec::validate_name(&name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let template_ref = apply(&body.template_ref);
    let flavor: Option<(i32, i64)> = match body.flavor_id {
        Some(flavor_id) => Some(
            crate::db::query_as("SELECT vcpus, memory_mib FROM flavors WHERE id = ?")
                .bind(flavor_id)
                .fetch_optional(&state.pool)
                .await?
                .ok_or_else(|| ApiError::bad_request("flavor not found"))?,
        ),
        None => None,
    };
    let memory = flavor
        .map(|(_, memory_mib)| format!("{memory_mib}Mi"))
        .unwrap_or_else(|| body.memory.clone());
    let mut vm = VirtualMachine::new(&name, &memory);
    if let Some((vcpus, _)) = flavor {
        vm.spec.cpu.cores = vcpus.max(1) as u32;
    }
    if let Some(network) = &body.network {
        if let Some(net) = vm.spec.network.first_mut() {
            net.network = network.clone();
        }
    }
    vm.spec.template_ref = Some(template_ref.clone());
    let ssh_pubkey = resolve_key_name(&state.pool, body.key_name.as_deref(), body.cloud_init_ssh_pubkey.clone()).await?;
    if body.cloud_init_user.is_some()
        || body.cloud_init_password.is_some()
        || ssh_pubkey.is_some()
        || body.cloud_init_user_data.is_some()
    {
        vm.spec.cloud_init = Some(CloudInitSpec {
            user: body
                .cloud_init_user
                .as_ref()
                .map(|u| apply(u))
                .unwrap_or_else(|| "ubuntu".into()),
            password: body.cloud_init_password.as_ref().map(|p| apply(p)),
            ssh_pubkey: ssh_pubkey.as_ref().map(|k| apply(k)),
            user_data: body.cloud_init_user_data.clone(),
        });
    }
    if let Ok(Some(profile)) =
        crate::engine::template::resolve_template_firewall_profile(&state.pool, &template_ref).await
    {
        if let Some(net) = vm.spec.network.first_mut() {
            net.firewall_profile = Some(profile);
        }
    }
    let create_body = CreateVmBody {
        flavor_id: body.flavor_id,
        vm,
        host_id: body.host_id,
        tags: vec![],
        desired_state: body.desired_state,
        atlas_root_disk: false,
        atlas_policy: None,
        preemptible: body.preemptible,
        preempt_priority: body.preempt_priority,
    };
    let resp = create_vm(State(state), Extension(actor), Json(create_body)).await?;
    if let (Some(m), Ok(task_id)) = (sleep_after, Uuid::parse_str(&resp.task_id)) {
        crate::db::query(
            "UPDATE vms SET sleep_after_minutes = ? WHERE id = (SELECT resource_id FROM tasks WHERE id = ?)",
        )
        .bind(m)
        .bind(task_id)
        .execute(&pool)
        .await?;
    }
    Ok(resp)
}

#[derive(Debug, Deserialize)]
pub struct CreateFromIsoBody {
    pub name: String,
    pub iso_path: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default)]
    pub disk_gib: Option<u64>,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default = "default_desired")]
    pub desired_state: String,
    #[serde(default)]
    pub cloud_init_user: Option<String>,
    #[serde(default)]
    pub cloud_init_password: Option<String>,
    #[serde(default)]
    pub cloud_init_ssh_pubkey: Option<String>,
    /// Name of a saved key pair whose public key is injected (EC2 `KeyName`); giving it together with `cloud_init_ssh_pubkey` is an error.
    #[serde(default)]
    pub key_name: Option<String>,
    /// Free-form cloud-init user-data (a `#cloud-config` document or script), at most 16 KiB; passed to the guest verbatim.
    #[serde(default)]
    pub cloud_init_user_data: Option<String>,
}

pub async fn create_from_iso(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateFromIsoBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let iso_path = body.iso_path.trim();
    if iso_path.is_empty() {
        return Err(ApiError::bad_request("iso_path is required"));
    }
    if !iso_path.contains(':') && !iso_path.starts_with('/') {
        return Err(ApiError::bad_request(
            "iso_path must be an absolute path on the hypervisor",
        ));
    }
    let approved: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM content_images WHERE path = ? AND status = 'available'",
    )
    .bind(iso_path)
    .fetch_one(&state.pool)
    .await?;
    if approved == 0 {
        // Deliberately a warning, not a hard reject, for the *governance*
        // check specifically: the actual arbitrary-host-path risk (an
        // operator pointing a CD-ROM at e.g. `/etc/shadow` or another
        // project's disk image) is already hard-closed downstream —
        // `create_vm` calls `body.vm.validate_operator_submission()`, which
        // runs `spec::vm::validate_install_iso_path` and rejects any
        // `install_iso` label outside `/var/lib/libvirt/images/` or
        // `/var/lib/machina/images/` (plus a `..`-traversal guard), for both
        // this endpoint and `create_from_virt_install`'s identical label.
        // What's left unenforced here is purely the *content_images
        // approval* registry — i.e. "was this ISO vetted and marked
        // available" — for a path that's already confined to machina-owned
        // directories. Also, `content_images` approval has no separation of
        // duties from submission (both gated only by `require_operator`, see
        // `api/content.rs`), so a hard reject would just push the same
        // operator to self-register+self-approve the path first rather than
        // stop anything. Whether unapproved-but-in-directory ISOs should be
        // hard-blocked is a product/governance call, not a security fix —
        // left as a warning (with actor attribution for audit) pending that
        // decision.
        tracing::warn!(
            iso_path,
            actor = %actor.username,
            "create_from_iso: ISO not in approved content library"
        );
    }

    let disk_gib = body.disk_gib.unwrap_or(40);
    let mut vm = VirtualMachine::new(&body.name, &body.memory);
    vm.spec.storage = vec![machina_spec::StorageVolumeSpec {
        name: "root".into(),
        size: format!("{disk_gib}Gi"),
        class: "silver".into(),
        source: None,
    }];
    vm.metadata.labels = Some(std::collections::HashMap::from([(
        "install_iso".into(),
        iso_path.to_string(),
    )]));
    let ssh_pubkey = resolve_key_name(&state.pool, body.key_name.as_deref(), body.cloud_init_ssh_pubkey.clone()).await?;
    if body.cloud_init_user.is_some()
        || body.cloud_init_password.is_some()
        || ssh_pubkey.is_some()
        || body.cloud_init_user_data.is_some()
    {
        vm.spec.cloud_init = Some(CloudInitSpec {
            user: body
                .cloud_init_user
                .clone()
                .unwrap_or_else(|| "ubuntu".into()),
            password: body.cloud_init_password.clone(),
            ssh_pubkey: ssh_pubkey.clone(),
            user_data: body.cloud_init_user_data.clone(),
        });
    }
    let create_body = CreateVmBody {
        flavor_id: None,
        vm,
        host_id: body.host_id,
        tags: vec!["iso-install".into()],
        desired_state: body.desired_state,
        atlas_root_disk: false,
        atlas_policy: None,
        preemptible: false,
        preempt_priority: 0,
    };
    create_vm(State(state), Extension(actor), Json(create_body)).await
}

#[derive(Debug, Deserialize)]
pub struct CreateFromVirtInstallBody {
    pub name: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default)]
    pub disk_gib: Option<u64>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub host_id: Option<Uuid>,
    #[serde(default = "default_desired")]
    pub desired_state: String,
    #[serde(default)]
    pub os_variant: Option<String>,
    #[serde(default)]
    pub firmware: Option<String>,
    #[serde(default)]
    pub virt_install_location: Option<String>,
    #[serde(default)]
    pub virt_install_pxe: Option<bool>,
    #[serde(default)]
    pub virt_install_pxe_network: Option<String>,
    #[serde(default)]
    pub virt_install_install_os: Option<String>,
    #[serde(default)]
    pub virt_install_extra_args: Option<String>,
    #[serde(default)]
    pub virt_install_define_only: Option<bool>,
    #[serde(default)]
    pub existing_disk: Option<String>,
    #[serde(default)]
    pub root_disk_storage_pool: Option<String>,
    #[serde(default)]
    pub root_disk_storage_volume: Option<String>,
    #[serde(default)]
    pub virt_install_disk_backing_store: Option<String>,
    #[serde(default)]
    pub install_iso: Option<String>,
    #[serde(default)]
    pub virt_install_path_in_use_check_off: Option<bool>,
    #[serde(default)]
    pub virt_install_unattended: Option<bool>,
    #[serde(default)]
    pub virt_install_admin_password: Option<String>,
    #[serde(default)]
    pub virt_install_user_login: Option<String>,
    #[serde(default)]
    pub virt_install_user_password: Option<String>,
    #[serde(default)]
    pub cloud_init_user: Option<String>,
    #[serde(default)]
    pub cloud_init_password: Option<String>,
    #[serde(default)]
    pub cloud_init_ssh_pubkey: Option<String>,
    /// Name of a saved key pair whose public key is injected (EC2 `KeyName`); giving it together with `cloud_init_ssh_pubkey` is an error.
    #[serde(default)]
    pub key_name: Option<String>,
    /// Free-form cloud-init user-data (a `#cloud-config` document or script), at most 16 KiB; passed to the guest verbatim.
    #[serde(default)]
    pub cloud_init_user_data: Option<String>,
}

pub async fn create_from_virt_install(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateFromVirtInstallBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let has_location = body
        .virt_install_location
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_pxe = body.virt_install_pxe == Some(true);
    let has_install_os = body
        .virt_install_install_os
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let define_only = body.virt_install_define_only == Some(true);
    if !has_location && !has_pxe && !has_install_os && !define_only {
        return Err(ApiError::bad_request(
            "Provide virt_install_location, virt_install_pxe, virt_install_install_os, or virt_install_define_only",
        ));
    }
    if has_pxe && (has_location || has_install_os) {
        return Err(ApiError::bad_request(
            "virt_install_pxe cannot be combined with location or install_os",
        ));
    }

    let disk_gib = body.disk_gib.unwrap_or(40);
    let network = body
        .network
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "default".into());
    let mut labels = std::collections::HashMap::new();
    labels.insert("create_backend".into(), "virt_install".into());
    if let Some(v) = body.os_variant.as_deref().filter(|s| !s.trim().is_empty()) {
        labels.insert("os_variant".into(), v.trim().to_string());
    }
    if let Some(loc) = body
        .virt_install_location
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("virt_install_location".into(), loc.trim().to_string());
    }
    if has_pxe {
        labels.insert("virt_install_pxe".into(), "true".into());
    }
    if let Some(net) = body
        .virt_install_pxe_network
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("virt_install_pxe_network".into(), net.trim().to_string());
    }
    if let Some(os) = body
        .virt_install_install_os
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("virt_install_install_os".into(), os.trim().to_string());
    }
    if let Some(args) = body
        .virt_install_extra_args
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("virt_install_extra_args".into(), args.trim().to_string());
    }
    if define_only {
        labels.insert("virt_install_define_only".into(), "true".into());
    }
    if let Some(disk) = body
        .existing_disk
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("existing_disk".into(), disk.trim().to_string());
    }
    if let Some(pool) = body
        .root_disk_storage_pool
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("root_disk_storage_pool".into(), pool.trim().to_string());
    }
    if let Some(vol) = body
        .root_disk_storage_volume
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert("root_disk_storage_volume".into(), vol.trim().to_string());
    }
    if let Some(backing) = body
        .virt_install_disk_backing_store
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        labels.insert(
            "virt_install_disk_backing_store".into(),
            backing.trim().to_string(),
        );
    }
    if let Some(iso) = body.install_iso.as_deref().filter(|s| !s.trim().is_empty()) {
        labels.insert("install_iso".into(), iso.trim().to_string());
    }
    if body.virt_install_path_in_use_check_off == Some(true) {
        labels.insert("virt_install_path_in_use_check_off".into(), "true".into());
    }
    if body.virt_install_unattended == Some(true) {
        labels.insert("virt_install_unattended".into(), "true".into());
    }
    if let Some(pw) = body
        .virt_install_admin_password
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        labels.insert("virt_install_admin_password".into(), pw.to_string());
    }
    if let Some(u) = body
        .virt_install_user_login
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        labels.insert("virt_install_user_login".into(), u.to_string());
    }
    if let Some(pw) = body
        .virt_install_user_password
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        labels.insert("virt_install_user_password".into(), pw.to_string());
    }

    let mut vm = VirtualMachine::new(&body.name, &body.memory);
    vm.spec.storage = vec![machina_spec::StorageVolumeSpec {
        name: "root".into(),
        size: format!("{disk_gib}Gi"),
        class: "silver".into(),
        source: None,
    }];
    vm.spec.network = vec![machina_spec::NetworkAttachmentSpec {
        network: network.clone(),
        ip_mode: "dhcp".into(),
        firewall_profile: None,
    }];
    if let Some(fw) = body.firmware.as_deref().filter(|s| !s.trim().is_empty()) {
        vm.spec.firmware = fw.trim().to_string();
    }
    vm.metadata.labels = Some(labels);
    let ssh_pubkey = resolve_key_name(&state.pool, body.key_name.as_deref(), body.cloud_init_ssh_pubkey.clone()).await?;
    if body.cloud_init_user.is_some()
        || body.cloud_init_password.is_some()
        || ssh_pubkey.is_some()
        || body.cloud_init_user_data.is_some()
    {
        vm.spec.cloud_init = Some(CloudInitSpec {
            user: body
                .cloud_init_user
                .clone()
                .unwrap_or_else(|| "ubuntu".into()),
            password: body.cloud_init_password.clone(),
            ssh_pubkey: ssh_pubkey.clone(),
            user_data: body.cloud_init_user_data.clone(),
        });
    }

    let tag = if has_pxe {
        "pxe-install"
    } else if has_install_os {
        "download-install"
    } else if define_only {
        "define-only"
    } else {
        "url-install"
    };
    let create_body = CreateVmBody {
        flavor_id: None,
        vm,
        host_id: body.host_id,
        tags: vec![tag.into(), "virt-install".into()],
        desired_state: body.desired_state,
        atlas_root_disk: false,
        atlas_policy: None,
        preemptible: false,
        preempt_priority: 0,
    };
    create_vm(State(state), Extension(actor), Json(create_body)).await
}

pub async fn adaptive_migration_decision(
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<crate::engine::adaptive_migration::MigrationTelemetry>,
) -> Result<Json<crate::engine::adaptive_migration::AdaptiveMigrationDecision>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(crate::engine::adaptive_migration::decide(
        &body,
        &crate::engine::adaptive_migration::AdaptiveMigrationPolicy::default(),
    )))
}

/// `POST /api/v1/migrations/adaptive/step`: look at one running migration of a VM, decide the next safe tuning step and apply it.
pub async fn adaptive_migration_step(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<crate::engine::adaptive_actuator::AdaptiveActuatorRequest>,
) -> Result<Json<crate::engine::adaptive_actuator::AdaptiveActuatorResponse>, ApiError> {
    require_operator(&actor)?;
    crate::engine::adaptive_actuator::step(&state.pool, &body)
        .await
        .map(Json)
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub async fn migrate_precheck(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<MigrateVmBody>,
) -> Result<Json<crate::engine::migrate_precheck::MigratePrecheckResult>, ApiError> {
    require_operator(&actor)?;
    let result = run_migrate_precheck(&state.pool, id, body.dest_host_id, body.live)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(result))
}

/// `POST /api/v1/vms/{id}/migration-oracle`: predict whether live pre-copy converges and whether the destination is worth it.
/// Read-only; the fail-closed `migrate/precheck` stays authoritative for whether a move may be attempted.
pub async fn migration_oracle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::engine::migration_oracle::MigrationOracleRequest>,
) -> Result<Json<crate::engine::migration_oracle::MigrationOracleReport>, ApiError> {
    require_operator(&actor)?;
    crate::engine::migration_oracle::predict(&state.pool, id, &body)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn start_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "start", "vm.start", None).await
}

pub async fn stop_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "stop", "vm.stop", None).await
}

pub async fn reboot_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<VmPowerBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let mode = body.and_then(|b| b.0.mode);
    power_action(&state, id, "reboot", "vm.reboot", mode).await
}

pub async fn shutdown_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<VmPowerBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let mode = body.and_then(|b| b.0.mode);
    power_action(&state, id, "shutdown", "vm.shutdown", mode).await
}

pub async fn pause_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "pause", "vm.pause", None).await
}

pub async fn resume_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "resume", "vm.resume", None).await
}

pub async fn sleep_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "managedsave", "vm.sleep", None).await
}

pub async fn wake_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "start", "vm.wake", None).await
}

#[derive(Debug, Deserialize)]
pub struct SleepPolicyBody {
    /// Minutes of idleness before the VM is put to sleep; `null` inherits the
    /// project default, `0` never sleeps.
    pub sleep_after_minutes: Option<i64>,
}

pub async fn get_vm_sleep_policy(
    State(state): State<AppState>,
    Extension(_actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::engine::vm_sleep::SleepPolicyView>, ApiError> {
    let view = crate::engine::vm_sleep::policy_view(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("vm not found"))?;
    Ok(Json(view))
}

pub async fn set_vm_sleep_policy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SleepPolicyBody>,
) -> Result<Json<crate::engine::vm_sleep::SleepPolicyView>, ApiError> {
    require_operator(&actor)?;
    if let Some(m) = body.sleep_after_minutes {
        if !(0..=10_080).contains(&m) {
            return Err(ApiError::bad_request(
                "sleep_after_minutes must be between 0 and 10080 (7 days)",
            ));
        }
        if (1..crate::engine::vm_sleep::MIN_SLEEP_AFTER_MINUTES).contains(&m) {
            return Err(ApiError::bad_request(format!(
                "sleep_after_minutes must be 0 or at least {}",
                crate::engine::vm_sleep::MIN_SLEEP_AFTER_MINUTES
            )));
        }
    }
    let res = crate::db::query("UPDATE vms SET sleep_after_minutes = ? WHERE id = ?")
        .bind(body.sleep_after_minutes)
        .bind(id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found("vm not found"));
    }
    let view = crate::engine::vm_sleep::policy_view(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("vm not found"))?;
    Ok(Json(view))
}

pub async fn reset_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    power_action(&state, id, "reset", "vm.reset", None).await
}

pub async fn install_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let meta: (Option<Uuid>, String, String) = crate::db::query_as(
        "SELECT host_id, COALESCE(inventory_source, 'libvirt'), observed_state FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if meta.1 == "kubevirt" {
        return Err(ApiError::bad_request("Install applies to libvirt VMs only"));
    }
    if meta.2 != "stopped" && meta.2 != "shut off" && meta.2 != "shutoff" {
        return Err(ApiError::bad_request(
            "Shut off the VM before starting installation",
        ));
    }
    let host_id = meta
        .0
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let task_id = enqueue_task(
        &state,
        "vm.install",
        serde_json::json!({ "vm_id": id.to_string() }),
        Some("vm"),
        Some(id),
        Some(host_id),
    )
    .await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.install".into(),
    }))
}

pub async fn get_vm_domain_xml(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Domain XML can embed a plaintext VNC/SPICE `passwd=` attribute (see
    // translate::domain_xml::graphics_block) — gate it like the other
    // console-credential-bearing reads (get_vm_viewer_vv, get_vm_qemu_logs)
    // rather than leaving it open to any authenticated (including viewer) role.
    require_operator(&actor)?;
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Domain XML is only available for libvirt-managed VMs",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let xml = crate::agent_client::get_domain_xml(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "xml": xml })))
}

pub(crate) async fn delete_vm_inventory_row(
    pool: &crate::db::DbPool,
    vm_id: Uuid,
) -> Result<String, ApiError> {
    let name: Option<String> = crate::db::query_scalar("DELETE FROM vms WHERE id = ? RETURNING name")
        .bind(vm_id)
        .fetch_optional(pool)
        .await?;
    name.ok_or_else(|| ApiError::not_found("vm not found"))
}

pub async fn delete_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<DeleteVmBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let require: bool = crate::db::query_scalar(
        "SELECT require_vm_delete_approval FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(false);
    if require && !body.as_ref().is_some_and(|b| b.0.confirmed) {
        return Err(ApiError::bad_request(
            "VM deletion requires approval — resubmit with {\"confirmed\": true}",
        ));
    }

    let meta: (Option<Uuid>, String) =
        crate::db::query_as("SELECT host_id, observed_state FROM vms WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;

    if meta.1 == "missing" {
        let name = delete_vm_inventory_row(&state.pool, id).await?;
        state.emit_event(
            "vm.pruned",
            format!("Pruned missing VM record {name} from inventory"),
        );
        return Ok(Json(TaskResponse {
            task_id: id.to_string(),
            status: "completed".into(),
            operation: "vm.delete".into(),
        }));
    }

    let host_id = meta.0;

    // Set before the task is even dequeued, not just when the worker picks it
    // up: the reconcile loop's own query already excludes lifecycle_phase =
    // 'deleting', but it runs on a 60s tick independent of the task queue, so a
    // delete that only flips the phase at dequeue time left a window where a
    // paused-but-desired-running VM's delete could race a concurrently
    // reconcile-enqueued `resume`.
    crate::engine::vm_lifecycle::set_vm_phase(
        &state.pool,
        id,
        crate::engine::vm_lifecycle::PHASE_DELETING,
    )
    .await?;

    let task_id = enqueue_task(
        &state,
        "vm.delete",
        serde_json::json!({ "vm_id": id.to_string() }),
        Some("vm"),
        Some(id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.delete".into(),
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct DeleteVmBody {
    #[serde(default)]
    pub confirmed: bool,
}

pub async fn install_guest_tools(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let task_id = enqueue_task(
        &state,
        "vm.guest_tools.install",
        serde_json::json!({ "vm_id": id.to_string() }),
        Some("vm"),
        Some(id),
        host_id,
    )
    .await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.guest_tools.install".into(),
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct VmPowerBody {
    #[serde(default)]
    pub mode: Option<String>,
}

pub(crate) async fn power_action(
    state: &AppState,
    vm_id: Uuid,
    action: &str,
    operation: &str,
    mode: Option<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let meta: (Option<Uuid>, String, String, String) = crate::db::query_as(
        "SELECT host_id, COALESCE(inventory_source, 'libvirt'), observed_state, COALESCE(lifecycle_phase, 'idle') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_one(&state.pool)
    .await?;
    if meta.3 == crate::engine::vm_lifecycle::PHASE_RETIRED && action == "start" {
        return Err(ApiError::bad_request(
            "VM is retired — restore lifecycle before starting",
        ));
    }
    if meta.1 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Power actions apply to libvirt VMs only — use Kubernetes / KubeVirt tools for cluster guests",
        ));
    }
    if meta.2 == "missing" {
        return Err(ApiError::bad_request(
            "VM is missing from hypervisor inventory — sync hosts or remove the stale record",
        ));
    }
    if action == "managedsave" && !matches!(meta.2.as_str(), "running" | "blocked") {
        return Err(
            ApiError::bad_request("Only a running VM can be put to sleep")
                .with_code("vm_not_running"),
        );
    }
    let host_id = meta
        .0
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;

    // Record explicit user intent in desired_state so the reconcile loop doesn't
    // immediately revert the action — e.g. without this, shutting a VM down leaves
    // desired_state='running' and reconcile restarts it. Transient actions (pause)
    // leave the desired power state unchanged. Sleep ("managedsave") sets
    // desired_state='sleeping' only once the save succeeds, since a sleeping
    // desired state with a running guest reads as "woken by traffic".
    let desired_state = match action {
        "start" | "reboot" | "reset" | "resume" => Some("running"),
        "stop" | "shutdown" => Some("stopped"),
        _ => None,
    };
    if let Some(desired) = desired_state {
        crate::db::query("UPDATE vms SET desired_state = ? WHERE id = ?")
            .bind(desired)
            .bind(vm_id)
            .execute(&state.pool)
            .await?;
    }

    let mut payload = serde_json::json!({
        "vm_id": vm_id.to_string(),
        "action": action,
    });
    if let Some(m) = mode.filter(|s| !s.trim().is_empty()) {
        payload["mode"] = serde_json::Value::String(m);
    }
    let task_id = enqueue_task(
        state,
        "vm.power",
        payload,
        Some("vm"),
        Some(vm_id),
        Some(host_id),
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: operation.to_string(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct MigrateVmBody {
    pub dest_host_id: Uuid,
    #[serde(default = "default_live")]
    pub live: bool,
    /// Max migration bandwidth (MiB/s); omit or 0 for libvirt default.
    #[serde(default)]
    pub bandwidth_mib: Option<u64>,
    #[serde(default)]
    pub postcopy: bool,
    /// Defaults to true: an omitted/false value leaves the VM defined on
    /// BOTH source and destination host after migration, which is a
    /// split-brain risk on shared storage (both hosts could start the same
    /// VM). A caller that genuinely wants the source left defined (e.g. to
    /// roll back quickly) must now opt in explicitly with `false`.
    #[serde(default = "default_undefine_source")]
    pub undefine_source: bool,
    #[serde(default)]
    pub tunnelled: bool,
    #[serde(default)]
    pub migrate_disks: Vec<String>,
    #[serde(default)]
    pub disks_uri: Option<String>,
    #[serde(default)]
    pub copy_storage: bool,
}

fn default_live() -> bool {
    true
}

fn default_undefine_source() -> bool {
    true
}

pub async fn migrate_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<MigrateVmBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    crate::api::cloud::check_vm_host(&state.pool, id, body.dest_host_id).await?;
    let source_host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.migrate",
        serde_json::json!({
            "vm_id": id.to_string(),
            "dest_host_id": body.dest_host_id.to_string(),
            "live": body.live,
            "bandwidth_mib": body.bandwidth_mib.unwrap_or(0),
            "postcopy": body.postcopy,
            "undefine_source": body.undefine_source,
            "tunnelled": body.tunnelled,
            "migrate_disks": body.migrate_disks,
            "disks_uri": body.disks_uri,
            "copy_storage": body.copy_storage,
        }),
        Some("vm"),
        Some(id),
        source_host,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.migrate".into(),
    }))
}

#[derive(Debug, Default, Deserialize)]
pub struct RecoverFluxvmBody {
    /// Host to re-create on; default the VM's current host.
    #[serde(default)]
    pub host_id: Option<Uuid>,
}

/// Re-create a FluxVM VM from its last inventory record on `host_id` (the same
/// `ha.recover` task HA runs after a host failure): a fresh instance on the
/// same shared disk, taking over the disk lock.
pub async fn recover_fluxvm_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<RecoverFluxvmBody>>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    if !crate::engine::fluxvm_fleet::is_fluxvm(&state.pool, id).await {
        return Err(ApiError::bad_request("not a FluxVM VM").with_code("not_fluxvm"));
    }
    let current: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let target = body
        .and_then(|Json(b)| b.host_id)
        .or(current)
        .ok_or_else(|| ApiError::bad_request("VM has no host; pass host_id"))?;
    crate::api::cloud::check_vm_host(&state.pool, id, target).await?;
    let mut payload = serde_json::json!({
        "vm_id": id.to_string(),
        "host_id": target.to_string(),
        "desired_state": "running",
    });
    if let Some(h) = current {
        payload["recovered_from_host_id"] = h.to_string().into();
    }
    let task_id = enqueue_task(
        &state,
        "ha.recover",
        payload,
        Some("vm"),
        Some(id),
        Some(target),
    )
    .await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "ha.recover".into(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct CloneVmBody {
    pub new_name: String,
    /// `linked` (default), `full`, or `xml` (legacy shared disk).
    #[serde(default = "default_clone_mode")]
    pub clone_mode: String,
}

fn default_clone_mode() -> String {
    "linked".into()
}

pub async fn clone_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<CloneVmBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.new_name)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let task_id = enqueue_task(
        &state,
        "vm.clone",
        serde_json::json!({
            "vm_id": id.to_string(),
            "new_name": body.new_name,
            "clone_mode": body.clone_mode,
        }),
        Some("vm"),
        Some(id),
        host_id,
    )
    .await?;

    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.clone".into(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct PatchVmBody {
    pub desired_state: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub description: Option<String>,
    pub labels: Option<std::collections::BTreeMap<String, String>>,
}

pub async fn patch_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchVmBody>,
) -> Result<Json<VmRow>, ApiError> {
    require_operator(&actor)?;
    if let Some(ds) = &body.desired_state {
        crate::db::query("UPDATE vms SET desired_state = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(ds)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(project) = &body.project {
        crate::db::query("UPDATE vms SET project = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(project)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(tags) = &body.tags {
        crate::db::query("UPDATE vms SET tags = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(serde_json::to_string(tags).unwrap_or_else(|_| "[]".into()))
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(labels) = &body.labels {
        super::vm_network_policies::validate_labels(labels)?;
        crate::db::query("UPDATE vms SET labels = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(serde_json::to_string(labels).unwrap_or_else(|_| "{}".into()))
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(desc) = &body.description {
        let mut spec: serde_json::Value =
            crate::db::query_scalar("SELECT spec_json FROM vms WHERE id = ?")
                .bind(id)
                .fetch_one(&state.pool)
                .await?;
        let labels = spec
            .pointer_mut("/metadata/labels")
            .and_then(|v| v.as_object_mut());
        if let Some(map) = labels {
            let trimmed = desc.trim();
            if trimmed.is_empty() {
                map.remove("description");
            } else {
                map.insert(
                    "description".into(),
                    serde_json::Value::String(trimmed.into()),
                );
            }
        } else {
            spec["metadata"]["labels"] = serde_json::json!({ "description": desc.trim() });
        }
        crate::db::query("UPDATE vms SET spec_json = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&spec)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    get_vm(State(state), Path(id)).await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VmDiskRow {
    pub id: Uuid,
    pub name: String,
    pub size_gib: i64,
    pub storage_class: String,
    pub path: Option<String>,
}

pub async fn list_vm_disks(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<VmDiskRow>>, ApiError> {
    let rows = crate::db::query_as::<_, VmDiskRow>(
        "SELECT id, name, size_gib, storage_class, path FROM vm_disks WHERE vm_id = ? LIMIT 200",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    if !rows.is_empty() {
        return Ok(Json(rows));
    }

    // Adopted / inventory-discovered VMs often have no vm_disks rows yet.
    // Fall back to live libvirt disk inventory so the UI is not empty.
    match live_vm_disks_fallback(&state, id).await {
        Ok(live) if !live.is_empty() => Ok(Json(live)),
        Ok(_) => Ok(Json(rows)),
        Err(e) => {
            tracing::debug!(vm_id = %id, error = ?e, "live disk fallback unavailable");
            Ok(Json(rows))
        }
    }
}

async fn live_vm_disks_fallback(state: &AppState, id: Uuid) -> Result<Vec<VmDiskRow>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Ok(vec![]);
    }
    let Some(host_id) = row.1 else {
        return Ok(vec![]);
    };
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let details = crate::agent_client::get_vm_details(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(details
        .disks
        .into_iter()
        .filter(|d| d.device == "disk" || d.device.is_empty())
        .map(|d| {
            let size_gib = d
                .capacity_bytes
                .map(|b| (b / (1024 * 1024 * 1024)) as i64)
                .unwrap_or(0);
            let name = if d.target.is_empty() {
                d.source.rsplit('/').next().unwrap_or("disk").to_string()
            } else {
                d.target.clone()
            };
            VmDiskRow {
                id: Uuid::new_v4(),
                name,
                size_gib,
                storage_class: "discovered".into(),
                path: if d.source.is_empty() {
                    None
                } else {
                    Some(d.source)
                },
            }
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct VmNicRow {
    pub mac_address: String,
    pub network: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
}

/// Live NIC inventory from libvirt (no separate vm_nics table yet).
pub async fn list_vm_nics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<VmNicRow>>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Ok(Json(vec![]));
    }
    let Some(host_id) = row.1 else {
        return Ok(Json(vec![]));
    };
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let details = crate::agent_client::get_vm_details(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        details
            .interfaces
            .into_iter()
            .map(|i| VmNicRow {
                mac_address: i.mac_address,
                network: i.source,
                model: i.model,
                ip: i.ip,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct VmMetricsRow {
    pub vm_id: Uuid,
    pub cpu_percent: f32,
    pub memory_used_mib: i64,
    pub disk_read_iops: i64,
    pub disk_write_iops: i64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn get_vm_metrics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<VmMetricsRow>, ApiError> {
    let row = crate::db::query_as::<_, VmMetricsRow>(
        "SELECT vm_id, cpu_percent, memory_used_mib, disk_read_iops, disk_write_iops, updated_at
         FROM vm_metrics WHERE vm_id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("vm metrics not found"))
}

pub async fn adopt_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<VmRow>, ApiError> {
    require_operator(&actor)?;
    let row: Option<(bool, String)> =
        crate::db::query_as("SELECT managed, observed_state FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((managed, observed)) = row else {
        return Err(ApiError::not_found("vm not found"));
    };
    if managed {
        return Err(ApiError::bad_request("VM is already managed"));
    }
    let source: String =
        crate::db::query_scalar("SELECT COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    let desired = if source == "kubevirt" {
        if observed == "running" {
            "running"
        } else if observed == "stopped" {
            "stopped"
        } else {
            "unknown"
        }
    } else if observed == "running" {
        "running"
    } else {
        "stopped"
    };
    let lifecycle = match desired {
        "running" => crate::engine::vm_lifecycle::PHASE_RUNNING,
        "stopped" => crate::engine::vm_lifecycle::PHASE_STOPPED,
        _ => crate::engine::vm_lifecycle::PHASE_IDLE,
    };
    // `AND managed = FALSE` makes the flip atomic: of two concurrent adopts only
    // one affects a row and proceeds to write the audit/event; the loser gets the
    // same "already managed" error instead of a duplicate audit entry (TOCTOU on
    // the SELECT-then-UPDATE above).
    let adopted = crate::db::query(
        "UPDATE vms SET managed = TRUE, desired_state = ?, lifecycle_phase = ?, last_error = '', updated_at = datetime('now') WHERE id = ? AND managed = FALSE",
    )
    .bind(desired)
    .bind(lifecycle)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if adopted.rows_affected() == 0 {
        return Err(ApiError::bad_request("VM is already managed"));
    }
    crate::tasks::enqueue::write_audit(
        &state,
        &actor.username,
        "vm.adopt",
        "vm",
        Some(id),
        serde_json::json!({}),
    )
    .await?;
    state.emit_event(
        "vm.adopt",
        format!("VM {id} adopted into platform inventory"),
    );
    get_vm(State(state), Path(id)).await
}

#[derive(Debug, Serialize)]
pub struct PruneMissingResponse {
    pub deleted: u64,
}

pub async fn prune_missing_vms(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<PruneMissingResponse>, ApiError> {
    crate::auth::require_admin(&actor)?;
    let result = crate::db::query("DELETE FROM vms WHERE observed_state = 'missing' RETURNING id")
        .execute(&state.pool)
        .await?;
    let deleted = result.rows_affected();
    if deleted > 0 {
        state.emit_event(
            "vm.pruned",
            format!("Pruned {deleted} missing VM record(s) from inventory"),
        );
    }
    Ok(Json(PruneMissingResponse { deleted }))
}

#[derive(Debug, Serialize)]
pub struct PruneVmInventoryResponse {
    pub deleted: bool,
    pub name: String,
}

pub async fn prune_vm_inventory_record(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<PruneVmInventoryResponse>, ApiError> {
    require_operator(&actor)?;
    let observed: String = crate::db::query_scalar("SELECT observed_state FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if observed != "missing" {
        return Err(ApiError::bad_request(
            "Only missing VM records can be pruned — use delete for guests still on the hypervisor",
        ));
    }
    let name = delete_vm_inventory_row(&state.pool, id).await?;
    state.emit_event(
        "vm.pruned",
        format!("Pruned missing VM record {name} from inventory"),
    );
    Ok(Json(PruneVmInventoryResponse {
        deleted: true,
        name,
    }))
}

#[derive(Debug, Deserialize)]
pub struct AttachDiskBody {
    pub disk_path: String,
    #[serde(default = "default_target")]
    pub target_dev: String,
    #[serde(default)]
    pub size_gib: Option<i64>,
}

fn default_target() -> String {
    "vdb".into()
}

pub async fn attach_vm_disk(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AttachDiskBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    // Reject a caller-supplied Ceph/RBD network-disk source at this public HTTP
    // boundary: an `rbd:` string embeds a mon host + cephx auth secret UUID that the
    // agent (`agent::libvirt_ops::attach_disk`) trusts completely and attaches without
    // the local-pool confinement check (`assert_backup_source_within_pools`) that
    // otherwise applies to every disk_path here — without this guard, any operator
    // could attach an arbitrary Ceph pool/image from an arbitrary mon host just by
    // hand-crafting disk_path. The only legitimate source of an `rbd:` disk_path is
    // the server-resolved source `api::volumes::attach_volume` builds itself from
    // Atlas's own volume registry, which calls `attach_vm_disk_trusted` below instead
    // of this public handler.
    if body.disk_path.starts_with("rbd:") {
        return Err(ApiError::bad_request(
            "rbd: disk sources cannot be attached directly here — use POST /api/v1/volumes/{id}/attach",
        ));
    }
    attach_vm_disk_trusted(state, actor, id, body).await
}

/// Same as `attach_vm_disk` but skips the public-HTTP `rbd:`-source rejection above.
/// Used only by `api::volumes::attach_volume`, which resolves the RBD source itself
/// server-side (via Atlas's own volume registry) rather than accepting it as raw
/// caller input.
pub(crate) async fn attach_vm_disk_trusted(
    state: AppState,
    _actor: AuthUser,
    id: Uuid,
    body: AttachDiskBody,
) -> Result<Json<TaskResponse>, ApiError> {
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let disk_id = if let Some(size) = body.size_gib {
        let disk_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO vm_disks (id, vm_id, name, size_gib, storage_class, path)
             VALUES (?, ?, ?, ?, 'silver', ?)",
        )
        .bind(disk_id)
        .bind(id)
        .bind(&body.target_dev)
        .bind(size)
        .bind(&body.disk_path)
        .execute(&state.pool)
        .await?;
        Some(disk_id)
    } else {
        None
    };
    let task_id = enqueue_task(
        &state,
        "vm.disk.attach",
        serde_json::json!({
            "vm_id": id.to_string(),
            "disk_path": body.disk_path,
            "target_dev": body.target_dev,
        }),
        Some("vm"),
        Some(id),
        host_id,
    )
    .await
    .inspect_err(|_e| {
        if let Some(did) = disk_id {
            let pool = state.pool.clone();
            tokio::spawn(async move {
                let _ = crate::db::query("DELETE FROM vm_disks WHERE id = ?")
                    .bind(did)
                    .execute(&pool)
                    .await;
            });
        }
    })?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.disk.attach".into(),
    }))
}

pub async fn get_vm_libvirt_details(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<machina_core::state::VmDetails>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Libvirt details are only available for libvirt-managed VMs",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let details = crate::agent_client::get_vm_details(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(details))
}

pub async fn get_vm_hardware_summary(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<machina_core::libvirt::hardware_summary::VmHardwareSummaryReport>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Hardware summary applies to libvirt-managed VMs only",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::vm_libvirt_query(
        &mut client,
        &row.0,
        "hardware.summary",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let summary: machina_core::libvirt::hardware_summary::VmHardwareSummaryReport =
        serde_json::from_value(result).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(summary))
}

pub async fn get_vm_hardware_compat(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<machina_core::libvirt::hardware_summary::HardwareCompatReport>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Hardware compatibility check applies to libvirt-managed VMs only",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::vm_libvirt_query(
        &mut client,
        &row.0,
        "hardware.compat",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let report: machina_core::libvirt::hardware_summary::HardwareCompatReport =
        serde_json::from_value(result).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(report))
}

pub async fn get_vm_domain_caps(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<machina_core::libvirt::hardware_summary::DomainCapabilitiesReport>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Domain capabilities apply to libvirt-managed VMs only",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::vm_libvirt_query(
        &mut client,
        &row.0,
        "domain.caps.report",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let report: machina_core::libvirt::hardware_summary::DomainCapabilitiesReport =
        serde_json::from_value(result).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(report))
}

pub async fn get_vm_pending_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<machina_core::libvirt::pending_config::PendingConfig>, ApiError> {
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Pending config applies to libvirt-managed VMs only",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let result = crate::agent_client::vm_libvirt_query(
        &mut client,
        &row.0,
        "pending.config",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let cfg: machina_core::libvirt::pending_config::PendingConfig =
        serde_json::from_value(result).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(cfg))
}

#[derive(Debug, Deserialize)]
pub struct BatchParityBody {
    pub vm_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct VmParitySummaryItem {
    pub needs_shutdown: bool,
    pub spice: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn batch_vm_parity_summary(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BatchParityBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let mut items = serde_json::Map::new();
    for vm_id in body.vm_ids.iter().take(64) {
        let row: Result<(String, Option<Uuid>, String), sqlx::Error> = crate::db::query_as(
            "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
        )
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await;
        let Ok((name, host_id, source)) = row else {
            continue;
        };
        if source == "kubevirt" {
            items.insert(
                vm_id.to_string(),
                serde_json::to_value(VmParitySummaryItem {
                    needs_shutdown: false,
                    spice: false,
                    error: None,
                })
                .unwrap_or(serde_json::Value::Null),
            );
            continue;
        };
        let Some(host_id) = host_id else {
            items.insert(
                vm_id.to_string(),
                serde_json::to_value(VmParitySummaryItem {
                    needs_shutdown: false,
                    spice: false,
                    error: Some("no host".into()),
                })
                .unwrap_or(serde_json::Value::Null),
            );
            continue;
        };
        let summary = async {
            let (_, agent_addr) =
                crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
                    .await
                    .map_err(|e| e.to_string())?;
            let mut client = crate::agent_client::connect(&agent_addr)
                .await
                .map_err(|e| e.to_string())?;
            crate::agent_client::vm_libvirt_query(
                &mut client,
                &name,
                "parity.summary",
                &serde_json::json!({}),
            )
            .await
            .map_err(|e| e.to_string())
        }
        .await;
        match summary {
            Ok(v) => {
                items.insert(vm_id.to_string(), v);
            }
            Err(e) => {
                items.insert(
                    vm_id.to_string(),
                    serde_json::to_value(VmParitySummaryItem {
                        needs_shutdown: false,
                        spice: false,
                        error: Some(e),
                    })
                    .unwrap_or(serde_json::Value::Null),
                );
            }
        }
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct BatchGuestIpBody {
    pub vm_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct BatchGuestIpItem {
    pub guest_ip: Option<String>,
    pub nic_ip: Option<String>,
}

pub async fn batch_vm_guest_ips(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<BatchGuestIpBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let mut items = serde_json::Map::new();
    for vm_id in body.vm_ids.iter().take(64) {
        let row: Result<(String, Option<Uuid>, String), sqlx::Error> = crate::db::query_as(
            "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
        )
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await;
        let Ok((name, host_id, source)) = row else {
            continue;
        };
        if source == "kubevirt" {
            continue;
        }
        let Some(host_id) = host_id else {
            continue;
        };
        let ip = async {
            let (_, agent_addr) =
                crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
                    .await
                    .map_err(|e| e.to_string())?;
            let mut client = crate::agent_client::connect(&agent_addr)
                .await
                .map_err(|e| e.to_string())?;
            let details = crate::agent_client::get_vm_details(&mut client, &name)
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(details)
        }
        .await;
        if let Ok(details) = ip {
            let nic_ip = details
                .interfaces
                .iter()
                .find_map(|i| i.ip.clone())
                .filter(|s| !s.is_empty());
            let guest_ip = details
                .guest_ip
                .filter(|s| !s.is_empty())
                .or(nic_ip.clone());
            items.insert(
                vm_id.to_string(),
                serde_json::to_value(BatchGuestIpItem { guest_ip, nic_ip })
                    .unwrap_or(serde_json::Value::Null),
            );
        }
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn get_vm_viewer_vv(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    require_operator(&actor)?;
    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "virt-viewer download applies to libvirt-managed VMs only",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let plan = crate::agent_client::get_console_access_plan(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let console_type = if plan.console_type.is_empty() {
        "vnc".to_string()
    } else {
        plan.console_type.clone()
    };
    let mut listen_host = plan.vnc_host.trim().to_string();
    if listen_host.is_empty() {
        listen_host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.split(':').next())
            .filter(|h| {
                h.chars()
                    .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
            })
            .unwrap_or("127.0.0.1")
            .to_string();
    }
    let port = plan.vnc_port.max(0);
    let vv = format!(
        "[virt-viewer]\ntype={console_type}\nhost={listen_host}\nport={port}\ntitle={}\ndelete-this-file=1\nfullscreen=0\n",
        row.0
    );
    let filename = format!("{}.vv", row.0);
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/x-virt-viewer"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).unwrap_or_else(|_| {
                    HeaderValue::from_static("attachment; filename=\"console.vv\"")
                }),
            ),
        ],
        vv,
    ))
}

pub async fn get_vm_qemu_logs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<QemuLogsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (name, host_id) = crate::api::vm_row::vm_agent_row_libvirt(&state, id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let lines = q.lines.unwrap_or(500).min(5000);
    let result = crate::agent_client::vm_libvirt_query(
        &mut client,
        &name,
        "qemu.logs",
        &serde_json::json!({ "lines": lines }),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct RenameVmBody {
    pub new_name: String,
}

pub async fn rename_platform_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameVmBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let new_name = body.new_name.trim().to_string();
    if new_name.is_empty() {
        return Err(ApiError::bad_request("new_name is required"));
    }
    machina_spec::validate_name(&new_name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let row: (String, Option<Uuid>, String, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt'), observed_state FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request("Rename applies to libvirt VMs only"));
    }
    if row.3 != "shutoff" && row.3 != "stopped" {
        return Err(ApiError::bad_request("VM must be shut off before rename"));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let old_name = row.0.clone();
    // Update DB first — if libvirt rename then fails we can roll back the DB row safely.
    crate::db::query("UPDATE vms SET name = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&new_name)
        .bind(id)
        .execute(&state.pool)
        .await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if let Err(e) = crate::agent_client::vm_libvirt_invoke(
        &mut client,
        &old_name,
        "domain.rename",
        &serde_json::json!({ "new_name": new_name }),
    )
    .await
    {
        // Libvirt rename failed — roll back the DB name to keep them in sync.
        let _ = crate::db::query("UPDATE vms SET name = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&old_name)
            .bind(id)
            .execute(&state.pool)
            .await;
        return Err(ApiError::internal(e.to_string()));
    }
    state.emit_event("vm.rename", format!("VM renamed to {new_name}"));
    Ok(Json(
        serde_json::json!({ "status": "ok", "new_name": new_name }),
    ))
}

pub async fn inject_vm_nmi(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (name, host_id) = crate::api::vm_row::vm_agent_row_libvirt(&state, id).await?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    crate::agent_client::vm_libvirt_invoke(
        &mut client,
        &name,
        "domain.nmi",
        &serde_json::json!({}),
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    state.emit_event("vm.nmi", format!("NMI injected into VM {name}"));
    Ok(Json(serde_json::json!({ "status": "nmi_injected" })))
}

#[derive(Debug, Deserialize)]
pub struct QemuLogsQuery {
    pub lines: Option<u32>,
}

async fn enqueue_vm_host_task(
    state: &AppState,
    vm_id: Uuid,
    operation: &str,
    payload: serde_json::Value,
) -> Result<Json<TaskResponse>, ApiError> {
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    let task_id = enqueue_task(state, operation, payload, Some("vm"), Some(vm_id), host_id).await?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: operation.to_string(),
    }))
}

pub async fn detach_vm_disk(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, target)): Path<(Uuid, String)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.disk.detach",
        serde_json::json!({
            "vm_id": id.to_string(),
            "target_dev": target,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct ResizeVmDiskBody {
    pub size_gb: u64,
}

pub async fn resize_vm_disk(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, target)): Path<(Uuid, String)>,
    Json(body): Json<ResizeVmDiskBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.disk.resize",
        serde_json::json!({
            "vm_id": id.to_string(),
            "target_dev": target,
            "size_gb": body.size_gb,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct AttachNicBody {
    pub network: String,
    #[serde(default = "default_nic_model")]
    pub model: String,
}

fn default_nic_model() -> String {
    "virtio".into()
}

pub async fn attach_vm_nic(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AttachNicBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let (project, host): (String, Option<Uuid>) =
        crate::db::query_as("SELECT COALESCE(project,'default'),host_id FROM vms WHERE id=?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    crate::api::cloud::authorize_attachment(&state, &actor, &body.network, &project, host).await?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.nic.attach",
        serde_json::json!({
            "vm_id": id.to_string(),
            "network": body.network,
            "model": body.model,
        }),
    )
    .await
}

pub async fn detach_vm_nic(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((id, mac)): Path<(Uuid, String)>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.nic.detach",
        serde_json::json!({
            "vm_id": id.to_string(),
            "mac_address": mac,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct SetAutostartBody {
    pub enabled: bool,
}

pub async fn set_vm_autostart(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetAutostartBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.autostart",
        serde_json::json!({
            "vm_id": id.to_string(),
            "enabled": body.enabled,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct SetVcpusBody {
    pub count: u32,
}

pub async fn set_vm_vcpus(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetVcpusBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.resize",
        serde_json::json!({
            "vm_id": id.to_string(),
            "kind": "vcpus",
            "count": body.count,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct ChangeTypeBody {
    pub flavor_id: Uuid,
}

/// Why a type change cannot proceed from these sizes, if it cannot. Same size under a different flavor is allowed (it only
/// relabels the machine); the identical flavor at the identical size is refused so a click does not cost a restart.
/// Resolve EC2-style `key_name` to the saved key pair's public key. Both a name and a literal key is ambiguous.
pub(crate) async fn resolve_key_name(
    pool: &crate::db::DbPool,
    key_name: Option<&str>,
    literal: Option<String>,
) -> Result<Option<String>, ApiError> {
    let Some(name) = key_name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(literal);
    };
    if literal.as_deref().is_some_and(|k| !k.trim().is_empty()) {
        return Err(ApiError::bad_request("give either key_name or cloud_init_ssh_pubkey, not both"));
    }
    let key: Option<String> = crate::db::query_scalar("SELECT public_key FROM keypairs WHERE name = ? ORDER BY created_at LIMIT 1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    key.map(Some)
        .ok_or_else(|| ApiError::bad_request(format!("no key pair named '{name}'")).with_code("keypair_not_found"))
}

pub(crate) fn type_change_blocker(
    cur_vcpus: i32,
    cur_memory_mib: i64,
    new_vcpus: i32,
    new_memory_mib: i64,
    same_flavor: bool,
) -> Option<String> {
    if new_vcpus < 1 || new_memory_mib < 128 {
        return Some("that flavor has no usable CPU or memory size".into());
    }
    if same_flavor && cur_vcpus == new_vcpus && cur_memory_mib == new_memory_mib {
        return Some("the machine already has that instance type".into());
    }
    None
}

/// Change a machine's instance type (EC2's ModifyInstanceAttribute `instanceType`): the guest is shut down cleanly, resized to
/// the flavor's vCPUs and memory, and started again if it was running. Disk size is not changed. Machines managed by an
/// instance group are refused (change the launch template instead).
pub async fn change_vm_type(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ChangeTypeBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    let vm: Option<(i32, i64, Option<String>, Option<Uuid>)> =
        crate::db::query_as("SELECT vcpus, memory_mib, project, flavor_id FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((vcpus, memory_mib, project, current_flavor)) = vm else {
        return Err(ApiError::not_found("vm not found"));
    };
    let flavor: Option<(i32, i64)> =
        crate::db::query_as("SELECT vcpus, memory_mib FROM flavors WHERE id = ?")
            .bind(body.flavor_id)
            .fetch_optional(&state.pool)
            .await?;
    let (new_vcpus, new_memory) = flavor.ok_or_else(|| ApiError::bad_request("flavor not found"))?;
    if let Some(why) = type_change_blocker(
        vcpus,
        memory_mib,
        new_vcpus,
        new_memory,
        current_flavor == Some(body.flavor_id),
    ) {
        return Err(ApiError::bad_request(why));
    }
    let in_group: Option<i64> =
        crate::db::query_scalar("SELECT 1 FROM cloud_group_members WHERE vm_id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    if in_group.is_some() {
        return Err(ApiError::conflict(
            "this machine is managed by an instance group",
            "Change the group's launch template instead; a group only adopts machines that match its template exactly.",
        ));
    }
    let mut conn = state.pool.acquire().await?;
    policy::evaluate_vm_resize_tx(
        &mut conn,
        project.as_deref().unwrap_or("default"),
        i64::from(new_vcpus) - i64::from(vcpus),
        new_memory - memory_mib,
    )
    .await
    .map_err(|v| ApiError::policy_violation(v.message, v.remediation))?;
    drop(conn);
    let _ = crate::tasks::enqueue::write_audit(
        &state,
        &actor.username,
        "vm.change_type",
        "vm",
        Some(id),
        serde_json::json!({ "flavor_id": body.flavor_id, "from": { "vcpus": vcpus, "memory_mib": memory_mib }, "to": { "vcpus": new_vcpus, "memory_mib": new_memory } }),
    )
    .await;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.change_type",
        serde_json::json!({ "vm_id": id.to_string(), "flavor_id": body.flavor_id.to_string() }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct SetMemoryBody {
    pub memory_mb: u64,
}

pub async fn set_vm_memory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetMemoryBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    require_operator(&actor)?;
    enqueue_vm_host_task(
        &state,
        id,
        "vm.resize",
        serde_json::json!({
            "vm_id": id.to_string(),
            "kind": "memory",
            "memory_mb": body.memory_mb,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct PublishTemplateFromVmBody {
    pub template_name: String,
    pub version: String,
    #[serde(default = "publish_tpl_category")]
    pub category: String,
    #[serde(default)]
    pub workload: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub marketplace: bool,
    #[serde(default)]
    pub project: String,
}

fn publish_tpl_category() -> String {
    "Linux".into()
}

/// Publish a libvirt VM as a golden template (unifies daemon JSON + platform DB).
pub async fn publish_vm_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<PublishTemplateFromVmBody>,
) -> Result<Json<crate::api::templates::TemplateRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.template_name)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let row: (String, Option<Uuid>, String) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if row.2 == "kubevirt" {
        return Err(ApiError::bad_request(
            "Templates require libvirt-managed VMs",
        ));
    }
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("VM has no host assigned"))?;
    let (_, agent_addr) =
        crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut client = crate::agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let xml = crate::agent_client::get_domain_xml(&mut client, &row.0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let source_disk = machina_core::libvirt::template_apply::primary_disk_path_from_xml(&xml)
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::bad_request("Could not resolve VM root disk path from domain XML")
        })?;

    let daemon_json = format!("/var/lib/machina/templates/{}.json", body.template_name);
    let approval = if body.marketplace {
        "pending"
    } else {
        "approved"
    };
    let desc = if body.description.is_empty() {
        format!("Golden image from VM '{}'", row.0)
    } else {
        body.description.clone()
    };
    let tpl_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO templates (id, name, version, source_disk, cloud_init, os_family, category, workload, description, featured, marketplace, daemon_json_path, approval_status, project)
         VALUES (?, ?, ?, ?, TRUE, 'linux', ?, ?, ?, FALSE, ?, ?, ?, ?)
         ON CONFLICT (name, version) DO UPDATE SET
           source_disk = EXCLUDED.source_disk,
           workload = EXCLUDED.workload,
           description = EXCLUDED.description,
           daemon_json_path = EXCLUDED.daemon_json_path,
           approval_status = EXCLUDED.approval_status,
           project = EXCLUDED.project",
    )
    .bind(tpl_id)
    .bind(&body.template_name)
    .bind(&body.version)
    .bind(&source_disk)
    .bind(&body.category)
    .bind(&body.workload)
    .bind(&desc)
    .bind(body.marketplace)
    .bind(&daemon_json)
    .bind(approval)
    .bind(&body.project)
    .execute(&state.pool)
    .await?;

    let template_row = crate::db::query_as::<_, crate::api::templates::TemplateRow>(
        "SELECT id, name, version, source_disk, cloud_init, os_family, category, COALESCE(workload, '') AS workload, description, featured, marketplace, icon, firewall_profile, COALESCE(approval_status, 'approved') AS approval_status, COALESCE(git_ref, '') AS git_ref, COALESCE(daemon_json_path, '') AS daemon_json_path, COALESCE(project, '') AS project FROM templates WHERE name = ? AND version = ?",
    )
    .bind(&body.template_name)
    .bind(&body.version)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(template_row))
}

#[derive(Debug, Deserialize)]
pub struct RetireVmBody {
    #[serde(default)]
    pub final_backup: bool,
}

/// Stop VM, tag as retired, block future starts until restored.
pub async fn retire_vm(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<RetireVmBody>,
) -> Result<Json<TaskResponse>, ApiError> {
    crate::auth::require_operator(&actor)?;
    let row: (String, Option<Uuid>, String) =
        crate::db::query_as("SELECT name, host_id, observed_state FROM vms WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    crate::db::query(
        "UPDATE vms SET lifecycle_phase = ?, desired_state = 'stopped',
         tags = CASE WHEN tags IS NULL THEN '[\"retired\"]' ELSE json_insert(tags, '$[#]', 'retired') END
         WHERE id = ? AND NOT EXISTS (SELECT 1 FROM json_each(COALESCE(tags,'[]')) WHERE value = 'retired')",
    )
    .bind(crate::engine::vm_lifecycle::PHASE_RETIRED)
    .bind(id)
    .execute(&state.pool)
    .await?;

    let mut task_id = None;
    if row.2 == "running" {
        task_id = Some(
            enqueue_task(
                &state,
                "vm.power",
                serde_json::json!({ "vm_id": id.to_string(), "action": "stop" }),
                Some("vm"),
                Some(id),
                row.1,
            )
            .await?
            .to_string(),
        );
    }
    if body.final_backup {
        let backup_id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, 'full', 'pending')",
        )
        .bind(backup_id)
        .bind(id)
        .execute(&state.pool)
        .await?;
        enqueue_task(
            &state,
            "vm.backup",
            serde_json::json!({
                "vm_id": id.to_string(),
                "backup_id": backup_id.to_string(),
            }),
            Some("vm"),
            Some(id),
            row.1,
        )
        .await
        .inspect_err(|_e| {
            let pool = state.pool.clone();
            tokio::spawn(async move {
                let _ = crate::db::query("DELETE FROM backup_records WHERE id = ?")
                    .bind(backup_id)
                    .execute(&pool)
                    .await;
            });
        })?;
    }
    state.emit_event("vm.retire", format!("VM {} marked retired", row.0));
    Ok(Json(TaskResponse {
        task_id: task_id.unwrap_or_else(|| "none".into()),
        status: "completed".into(),
        operation: "vm.retire".into(),
    }))
}

/// Portable export: enqueues full qcow2 backup suitable for download from backup_path.
pub async fn export_vm_disk(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskResponse>, ApiError> {
    crate::auth::require_operator(&actor)?;
    let host_id: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let backup_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO backup_records (id, vm_id, backup_type, status) VALUES (?, ?, 'export', 'pending')",
    )
    .bind(backup_id)
    .bind(id)
    .execute(&state.pool)
    .await?;
    let task_id = enqueue_task(
        &state,
        "vm.backup",
        serde_json::json!({
            "vm_id": id.to_string(),
            "backup_id": backup_id.to_string(),
            "export": true,
        }),
        Some("vm"),
        Some(id),
        host_id,
    )
    .await
    .inspect_err(|_e| {
        let pool = state.pool.clone();
        tokio::spawn(async move {
            let _ = crate::db::query("DELETE FROM backup_records WHERE id = ?")
                .bind(backup_id)
                .execute(&pool)
                .await;
        });
    })?;
    Ok(Json(TaskResponse {
        task_id: task_id.to_string(),
        status: "pending".into(),
        operation: "vm.disk.export".into(),
    }))
}

#[cfg(test)]
mod type_change_tests {
    use super::type_change_blocker;

    #[test]
    fn the_same_flavor_at_the_same_size_is_refused() {
        assert!(type_change_blocker(2, 4096, 2, 4096, true).unwrap().contains("already"));
    }

    #[test]
    fn a_different_flavor_is_allowed_even_at_the_same_size_or_smaller_or_larger() {
        assert_eq!(type_change_blocker(2, 4096, 2, 4096, false), None); // relabel only
        assert_eq!(type_change_blocker(2, 4096, 4, 8192, true), None);
        assert_eq!(type_change_blocker(4, 8192, 2, 2048, false), None);
    }

    #[test]
    fn a_flavor_without_a_usable_size_is_refused() {
        assert!(type_change_blocker(2, 4096, 0, 4096, false).is_some());
        assert!(type_change_blocker(2, 4096, 2, 64, false).is_some());
    }
}

/// Most instances one `run-instances` call may start.
pub const MAX_RUN_COUNT: u32 = 20;

/// `{ ...create-from-template fields, count, min_count }` (EC2 `RunInstances`).
#[derive(Debug, Deserialize)]
pub struct RunInstancesBody {
    #[serde(flatten)]
    pub base: CreateFromTemplateBody,
    #[serde(default = "one")]
    pub count: u32,
    /// Fewest instances that count as success; defaults to `count`.
    #[serde(default)]
    pub min_count: Option<u32>,
}

fn one() -> u32 {
    1
}

/// Names for `count` instances: the name itself for one, `name-1 … name-N` for several.
pub(crate) fn run_names(name: &str, count: u32) -> Vec<String> {
    if count <= 1 {
        return vec![name.to_string()];
    }
    (1..=count).map(|i| format!("{name}-{i}")).collect()
}

pub(crate) fn validate_run_counts(count: u32, min_count: Option<u32>) -> Result<u32, String> {
    if count == 0 || count > MAX_RUN_COUNT {
        return Err(format!("count must be between 1 and {MAX_RUN_COUNT}"));
    }
    let min = min_count.unwrap_or(count);
    if min == 0 || min > count {
        return Err("min_count must be between 1 and count".into());
    }
    Ok(min)
}

/// Start several instances from one template. Each is created like `create_from_template`; if fewer than
/// `min_count` could be created the call fails and says which ones were already created (they are not rolled back).
pub async fn run_instances(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<RunInstancesBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let min = validate_run_counts(body.count, body.min_count).map_err(ApiError::bad_request)?;
    let mut created: Vec<serde_json::Value> = Vec::new();
    let mut last_error: Option<ApiError> = None;
    for name in run_names(&body.base.name, body.count) {
        let mut one = body.base.clone();
        one.name = name.clone();
        match create_from_template(State(state.clone()), Extension(actor.clone()), Json(one)).await {
            Ok(Json(t)) => created.push(serde_json::json!({ "name": name, "task_id": t.task_id })),
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }
    if created.len() < min as usize {
        let names: Vec<String> = created.iter().filter_map(|c| c["name"].as_str().map(String::from)).collect();
        let why = last_error.map(|e| e.message).unwrap_or_default();
        return Err(ApiError::conflict(
            format!(
                "only {} of {} instances could be created (minimum {min}): {why}",
                created.len(),
                body.count
            ),
            if names.is_empty() {
                "fix the cause and retry".to_string()
            } else {
                format!("already created and left in place: {}", names.join(", "))
            },
        )
        .with_code("run_instances_min_count"));
    }
    Ok(Json(serde_json::json!({
        "requested": body.count,
        "created": created.len(),
        "instances": created,
        "error": last_error.map(|e| e.message),
    })))
}

#[cfg(test)]
mod run_instances_tests {
    use super::*;

    #[test]
    fn names_one_or_numbered() {
        assert_eq!(run_names("web", 1), ["web"]);
        assert_eq!(run_names("web", 3), ["web-1", "web-2", "web-3"]);
    }

    #[test]
    fn counts_are_bounded() {
        assert_eq!(validate_run_counts(3, None), Ok(3));
        assert_eq!(validate_run_counts(5, Some(2)), Ok(2));
        assert!(validate_run_counts(0, None).is_err());
        assert!(validate_run_counts(MAX_RUN_COUNT + 1, None).is_err());
        assert!(validate_run_counts(3, Some(0)).is_err());
        assert!(validate_run_counts(3, Some(4)).is_err());
    }
}
