// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Declarative multi-resource stacks — Phase D of the next-gen roadmap
//! (`/Users/ssahani/.claude/plans/lazy-munching-quilt.md`). A stack template
//! describes a set of resources (security groups, volumes, VMs) created together and
//! torn down as a unit — not a new orchestration engine, just a thin composition layer
//! over the existing native `create_security_group`/`create_volume`/`create_vm`
//! handlers, created in a fixed dependency order (security groups and volumes first,
//! since VMs may reference them by name) and torn down in reverse.
//!
//! Stack v2 adds instance groups and who-talks-to-whom policies
//! ([`crate::engine::stack_plan`]). Those parts are reconciled: an update applies
//! the difference, `engine::stack_reconcile` reports drift and recreates missing
//! VMs, and a failed deploy is rolled back.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use machina_bpf::netpol::VmNetworkPolicy;
use machina_spec::VirtualMachine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::networking::{self, CreateSecurityGroupBody, CreateSecurityGroupRuleBody};
use crate::api::projects::default_project_id;
use crate::api::vms::{self, CreateVmBody};
use crate::api::volumes::{self, AttachVolumeBody, CreateVolumeBody};
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::stack_plan::{self as sp, StackInstances, StackPolicy};
use crate::engine::vm_netpol;
use crate::state::AppState;

pub const DEPLOY_ACTION: &str = "stack.deploy";

// ---------------------------------------------------------------------------
// Template shape
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct StackTemplate {
    #[serde(default)]
    pub security_groups: Vec<StackSecurityGroup>,
    #[serde(default)]
    pub volumes: Vec<StackVolume>,
    #[serde(default)]
    pub vms: Vec<StackVm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<StackInstances>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<StackPolicy>,
}

impl StackTemplate {
    pub fn is_v2(&self) -> bool {
        !self.instances.is_empty() || !self.policies.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StackSecurityGroup {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<StackSecurityGroupRule>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StackSecurityGroupRule {
    #[serde(default = "default_direction")]
    pub direction: String,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub port_min: Option<i32>,
    #[serde(default)]
    pub port_max: Option<i32>,
    #[serde(default)]
    pub remote_cidr: Option<String>,
}

fn default_direction() -> String {
    "ingress".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StackVolume {
    pub name: String,
    pub size_gib: i64,
    #[serde(default = "default_volume_class")]
    pub volume_class: String,
}

fn default_volume_class() -> String {
    "silver".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StackVm {
    pub name: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_cores")]
    pub cpu_cores: u32,
    #[serde(default = "default_disk_gib")]
    pub disk_gib: i64,
    #[serde(default = "default_network")]
    pub network: String,
    /// Names of `volumes` (above) in this same template to attach once the VM is running.
    #[serde(default)]
    pub attach_volumes: Vec<String>,
}

fn default_memory() -> String {
    "1Gi".into()
}
fn default_cores() -> u32 {
    1
}
fn default_disk_gib() -> i64 {
    10
}
fn default_network() -> String {
    "default".into()
}

// ---------------------------------------------------------------------------
// Stack rows
// ---------------------------------------------------------------------------

/// `kind` is `security_group`, `volume`, `vm` (v1), `instance` (a VM of a v2
/// group), `netpol` (by name; `id` is nil) or `backup_schedule`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct StackResourceRef {
    pub(crate) kind: String,
    pub(crate) id: Uuid,
    pub(crate) name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StackRow {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub last_error: Option<String>,
    pub template_json: crate::db::Json<StackTemplate>,
    pub resources_json: crate::db::Json<serde_json::Value>,
    pub drift_json: crate::db::Json<serde_json::Value>,
    pub checked_at: Option<String>,
    pub auto_heal: bool,
    pub updated_at: Option<String>,
}

const STACK_SELECT: &str =
    "SELECT id, project_id, name, status, last_error, template_json, resources_json,
            drift_json, checked_at, auto_heal, updated_at FROM stacks";

pub async fn list_stacks(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<StackRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, StackRow>(&format!("{STACK_SELECT} ORDER BY created_at DESC"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

pub async fn get_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<StackRow>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(load_row(&state, id).await?))
}

async fn load_row(state: &AppState, id: Uuid) -> Result<StackRow, ApiError> {
    crate::db::query_as::<_, StackRow>(&format!("{STACK_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("stack not found"))
}

#[derive(Debug, Deserialize)]
pub struct CreateStackBody {
    pub name: String,
    pub template: StackTemplate,
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

/// `build_stack` creates every listed resource synchronously, in-process, inside a
/// single HTTP request — bypassing the per-route rate-limit/tracing middleware that
/// would normally apply if each resource were created via its own `POST` call. Cap the
/// template so one `create_stack` request can't fan out into an unbounded burst of
/// real VM/volume/security-group creation against the underlying hosts.
const MAX_STACK_RESOURCES: usize = 25;

fn validate_stack_size(template: &StackTemplate) -> Result<(), ApiError> {
    let total = template.security_groups.len() + template.volumes.len() + template.vms.len();
    if total > MAX_STACK_RESOURCES {
        return Err(ApiError::bad_request(format!(
            "stack template has {total} resources, exceeding the {MAX_STACK_RESOURCES} limit per stack"
        )));
    }
    Ok(())
}

/// Every problem with a template, v1 size limit and v2 groups and policies.
async fn validate_template(
    state: &AppState,
    name: &str,
    template: &StackTemplate,
) -> Result<sp::Catalog, ApiError> {
    machina_spec::validate_name(name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    validate_stack_size(template)?;
    let cat = catalog(&state.pool).await;
    let errors = sp::validate(name, &template.instances, &template.policies, &cat);
    if !errors.is_empty() {
        return Err(ApiError::bad_request(errors.join("; ")).with_code("invalid_stack_template"));
    }
    Ok(cat)
}

/// Flavors and images a template may name. Networks are not checked here.
pub async fn catalog(pool: &crate::db::DbPool) -> sp::Catalog {
    let flavors: Vec<(String, i64, i64, i64)> =
        crate::db::query_as("SELECT name, vcpus, memory_mib, disk_gib FROM flavors")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    let images: Vec<String> = crate::db::query_scalar(
        "SELECT DISTINCT name FROM templates WHERE COALESCE(approval_status, 'approved') = 'approved'",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    sp::Catalog {
        flavors: flavors
            .into_iter()
            .map(|(n, c, m, d)| {
                (
                    n,
                    sp::Flavor {
                        vcpus: c.max(1) as u32,
                        memory_mib: m,
                        disk_gib: d,
                    },
                )
            })
            .collect(),
        images: images.into_iter().collect(),
        networks: BTreeSet::new(),
    }
}

/// The stack being built: its row id and project.
#[derive(Debug, Clone)]
pub(crate) struct StackCtx {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) project_id: Uuid,
    pub(crate) project_name: String,
}

async fn resolve_project(
    state: &AppState,
    project_id: Option<Uuid>,
) -> Result<(Uuid, String), ApiError> {
    let project_id = match project_id {
        Some(id) => id,
        None => default_project_id(&state.pool).await?,
    };
    // vms.project is matched by free-text NAME (see Phase A's backward-compat note),
    // not by projects.id — resolve the name once so VM creation stays consistent with
    // every other project-scoped query in the codebase.
    let project_name: String = crate::db::query_scalar("SELECT name FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project not found"))?;
    Ok((project_id, project_name))
}

async fn ensure_name_free(state: &AppState, name: &str) -> Result<(), ApiError> {
    let taken: Option<Uuid> = crate::db::query_scalar("SELECT id FROM stacks WHERE name = ?")
        .bind(name)
        .fetch_optional(&state.pool)
        .await?;
    if taken.is_some() {
        return Err(ApiError::conflict(
            format!("a stack named '{name}' already exists"),
            "pick another name or update the existing stack",
        ));
    }
    Ok(())
}

/// `POST /api/v1/stacks` — creates every resource in the template, in order (security
/// groups, then volumes, then VMs with their `attach_volumes` resolved by name), and
/// records what was created so `delete_stack` can tear it all down. On any failure
/// partway through, the resources created so far are left in place (matching every
/// other create handler in this codebase — no silent rollback) and the stack is
/// marked `error` with the resource list captured up to the failure point, so the
/// operator can inspect and clean up via the normal per-resource DELETE endpoints.
/// A template with v2 groups or policies is rolled back instead (`rolled_back`).
pub async fn create_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateStackBody>,
) -> Result<Json<StackRow>, ApiError> {
    require_operator(&actor)?;
    validate_template(&state, &body.name, &body.template).await?;
    if body.template.is_v2() {
        ensure_name_free(&state, &body.name).await?;
    }
    let (project_id, project_name) = resolve_project(&state, body.project_id).await?;
    let ctx = StackCtx {
        id: Uuid::new_v4(),
        name: body.name.clone(),
        project_id,
        project_name,
    };
    insert_row(&state, &ctx, &body.template).await?;
    let result = run_create(&state, actor, &ctx, &body.template).await;
    let row = load_row(&state, ctx.id).await?;
    if let Err(e) = result {
        let code = if body.template.is_v2() {
            "stack_rolled_back"
        } else {
            "stack_partial_failure"
        };
        return Err(ApiError::internal(format!(
            "stack '{}' {} before failing: {}",
            body.name,
            if body.template.is_v2() {
                "was rolled back"
            } else {
                "partially created"
            },
            e.message
        ))
        .with_code(code));
    }
    Ok(Json(row))
}

async fn insert_row(
    state: &AppState,
    ctx: &StackCtx,
    template: &StackTemplate,
) -> Result<(), ApiError> {
    let template_json =
        serde_json::to_value(template).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query(
        "INSERT INTO stacks (id, project_id, name, template_json, status) VALUES (?, ?, ?, ?, 'creating')",
    )
    .bind(ctx.id)
    .bind(ctx.project_id)
    .bind(&ctx.name)
    .bind(&template_json)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn save_resources(
    state: &AppState,
    id: Uuid,
    status: &str,
    refs: &[StackResourceRef],
    error: Option<&str>,
) -> Result<(), ApiError> {
    let resources_json =
        serde_json::to_value(refs).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query(
        "UPDATE stacks SET status = ?, resources_json = ?, last_error = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(status)
    .bind(&resources_json)
    .bind(error)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Builds a new stack whose row exists in `creating`. v1 resources keep their
/// leave-in-place failure behaviour; with v2 parts a failure tears down
/// everything created.
pub(crate) async fn run_create(
    state: &AppState,
    actor: AuthUser,
    ctx: &StackCtx,
    template: &StackTemplate,
) -> Result<(), ApiError> {
    let mut refs = match build_stack(
        state,
        actor.clone(),
        template,
        ctx.project_id,
        &ctx.project_name,
    )
    .await
    {
        Ok(r) => r,
        Err((r, e)) => {
            return fail_create(state, actor, ctx, template, r, e).await;
        }
    };
    if template.is_v2() {
        if let Err(e) = apply_v2(state, &actor, ctx, template, &mut refs, ApplyMode::Create).await {
            return fail_create(state, actor, ctx, template, refs, e).await;
        }
        store_drift(state, ctx.id, &[]).await;
    }
    save_resources(state, ctx.id, "created", &refs, None).await
}

async fn fail_create(
    state: &AppState,
    actor: AuthUser,
    ctx: &StackCtx,
    template: &StackTemplate,
    refs: Vec<StackResourceRef>,
    e: ApiError,
) -> Result<(), ApiError> {
    if template.is_v2() {
        let errors = teardown(state, &actor, &refs).await;
        let msg = if errors.is_empty() {
            e.message.clone()
        } else {
            format!("{}; rollback left: {}", e.message, errors.join("; "))
        };
        let left: Vec<StackResourceRef> = if errors.is_empty() { Vec::new() } else { refs };
        save_resources(state, ctx.id, "rolled_back", &left, Some(&msg)).await?;
    } else {
        save_resources(state, ctx.id, "error", &refs, Some(&e.message)).await?;
    }
    Err(e)
}

async fn build_stack(
    state: &AppState,
    actor: AuthUser,
    template: &StackTemplate,
    project_id: Uuid,
    project_name: &str,
) -> Result<Vec<StackResourceRef>, (Vec<StackResourceRef>, ApiError)> {
    let mut created = Vec::new();
    let mut attach_count: usize = 0;

    for sg in &template.security_groups {
        let group = networking::create_security_group(
            State(state.clone()),
            Extension(actor.clone()),
            Json(CreateSecurityGroupBody {
                name: sg.name.clone(),
                description: String::new(),
                project_id: Some(project_id),
            }),
        )
        .await
        .map_err(|e| (created.clone(), e))?;
        created.push(StackResourceRef {
            kind: "security_group".into(),
            id: group.0.id,
            name: sg.name.clone(),
        });

        for rule in &sg.rules {
            let _ = networking::create_security_group_rule(
                State(state.clone()),
                Extension(actor.clone()),
                Path(group.0.id),
                Json(CreateSecurityGroupRuleBody {
                    direction: rule.direction.clone(),
                    protocol: rule.protocol.clone(),
                    port_min: rule.port_min,
                    port_max: rule.port_max,
                    remote_cidr: rule.remote_cidr.clone(),
                    remote_sg_id: None,
                    description: String::new(),
                }),
            )
            .await
            .map_err(|e| (created.clone(), e))?;
        }
    }

    let mut volume_ids: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();
    for vol in &template.volumes {
        let row = volumes::create_volume(
            State(state.clone()),
            Extension(actor.clone()),
            Json(CreateVolumeBody {
                name: vol.name.clone(),
                size_gib: vol.size_gib,
                project_id: Some(project_id),
                volume_class: vol.volume_class.clone(),
                delete_on_termination: false,
            }),
        )
        .await
        .map_err(|e| (created.clone(), e))?;
        created.push(StackResourceRef {
            kind: "volume".into(),
            id: row.0.id,
            name: vol.name.clone(),
        });
        volume_ids.insert(vol.name.clone(), row.0.id);
    }

    for vm_spec in &template.vms {
        let mut vm = VirtualMachine::new(&vm_spec.name, &vm_spec.memory);
        vm.metadata.project = Some(project_name.to_string());
        vm.spec.cpu.cores = vm_spec.cpu_cores.max(1);
        if let Some(root) = vm.spec.storage.first_mut() {
            root.size = format!("{}Gi", vm_spec.disk_gib.max(1));
        }
        if let Some(net) = vm.spec.network.first_mut() {
            net.network = vm_spec.network.clone();
        }
        let task = vms::create_vm(
            State(state.clone()),
            Extension(actor.clone()),
            Json(CreateVmBody {
                flavor_id: None,
                vm,
                host_id: None,
                tags: vec!["stack".into()],
                desired_state: "running".into(),
                atlas_root_disk: false,
                atlas_policy: None,
                preemptible: false,
                preempt_priority: 0,
            }),
        )
        .await
        .map_err(|e| (created.clone(), e))?;
        let task_uuid = Uuid::parse_str(&task.0.task_id)
            .map_err(|e| (created.clone(), ApiError::internal(e.to_string())))?;
        let vm_id: Uuid = crate::db::query_scalar("SELECT resource_id FROM tasks WHERE id = ?")
            .bind(task_uuid)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| (created.clone(), ApiError::from(e)))?;
        // Track the VM as created BEFORE waiting on its provisioning task: if the wait
        // below times out or the task fails partway through a real libvirt define+start,
        // the VM may still exist on the host and must be in `created` so `delete_stack`
        // (and the operator-visible partial-failure resource list) can find and remove
        // it — losing this entry here would leak the VM.
        created.push(StackResourceRef {
            kind: "vm".into(),
            id: vm_id,
            name: vm_spec.name.clone(),
        });
        // VM provisioning (define + start via libvirt) runs well past the 20s default
        // used elsewhere in this file — give it a generous ceiling before giving up.
        volumes::wait_for_task_timeout(&state.pool, &task.0.task_id, Duration::from_secs(180))
            .await
            .map_err(|e| (created.clone(), e))?;

        for vol_name in &vm_spec.attach_volumes {
            let vol_id = *volume_ids.get(vol_name).ok_or_else(|| {
                (
                    created.clone(),
                    ApiError::bad_request(format!(
                        "attach_volumes references unknown volume '{vol_name}'"
                    )),
                )
            })?;
            attach_count += 1;
            let _ = volumes::attach_volume(
                State(state.clone()),
                Extension(actor.clone()),
                Path(vol_id),
                Json(AttachVolumeBody {
                    vm_id,
                    target_dev: default_target_dev_for(attach_count),
                    delete_on_termination: None,
                }),
            )
            .await
            .map_err(|e| (created.clone(), e))?;
        }
    }

    Ok(created)
}

/// `n` is a 1-based attach count (the VM's own root disk is always `vda`, reserved and
/// never assigned here). Bijective base-26 over b..z, then aa, ab, ... — same scheme
/// spreadsheet columns use — so n=1 -> vdb, n=25 -> vdz, n=26 -> vdaa, and it never
/// wraps back around to collide with vda no matter how many volumes are attached.
fn default_target_dev_for(n: usize) -> String {
    let mut v = n + 1; // v=1 is reserved for vda; attachments start at v=2 ('b').
    let mut letters = Vec::new();
    while v > 0 {
        v -= 1;
        letters.push((b'a' + (v % 26) as u8) as char);
        v /= 26;
    }
    letters.reverse();
    format!("vd{}", letters.into_iter().collect::<String>())
}

/// `DELETE /api/v1/stacks/{id}` — tears down every tracked resource in reverse
/// creation order (VMs first, so their FK `ON DELETE SET NULL` frees any attached
/// volumes before those volumes are deleted; security groups last).
pub async fn delete_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let _busy = Busy::try_take(id).ok_or_else(busy_error)?;
    let errors = delete_stack_id(&state, &actor, id).await?;
    if errors.is_empty() {
        Ok(Json(serde_json::json!({ "deleted": true })))
    } else {
        Err(ApiError::internal(format!(
            "stack teardown had {} failure(s): {}",
            errors.len(),
            errors.join("; ")
        )))
    }
}

pub(crate) async fn delete_stack_id(
    state: &AppState,
    actor: &AuthUser,
    id: Uuid,
) -> Result<Vec<String>, ApiError> {
    let resources_json: crate::db::Json<Vec<StackResourceRef>> =
        crate::db::query_scalar("SELECT resources_json FROM stacks WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("stack not found"))?;
    let errors = teardown(state, actor, &resources_json.0).await;
    if errors.is_empty() {
        crate::db::query("DELETE FROM stacks WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await?;
    } else {
        crate::db::query("UPDATE stacks SET status = 'delete_failed', last_error = ? WHERE id = ?")
            .bind(errors.join("; "))
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    Ok(errors)
}

/// Deletes `refs` in reverse order; returns what could not be deleted.
pub(crate) async fn teardown(
    state: &AppState,
    actor: &AuthUser,
    refs: &[StackResourceRef],
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut policies = false;
    for res in refs.iter().rev() {
        let result = match res.kind.as_str() {
            // Must wait for the delete task to actually finish — vms::delete_vm only
            // enqueues it. Without this, the immediately-following volume delete (which
            // requires attached_vm_id to already be NULL via the FK's ON DELETE SET
            // NULL) races the still-pending VM deletion and fails with "still attached"
            // even though the VM is in the process of being removed.
            "vm" | "instance" => delete_vm_and_wait(state, actor, res.id).await,
            "volume" => {
                volumes::delete_volume(State(state.clone()), Extension(actor.clone()), Path(res.id))
                    .await
                    .map(|_| ())
            }
            "security_group" => networking::delete_security_group(
                State(state.clone()),
                Extension(actor.clone()),
                Path(res.id),
            )
            .await
            .map(|_| ()),
            "netpol" => {
                policies = true;
                vm_netpol::delete(&state.pool, &res.name)
                    .await
                    .map(|_| ())
                    .map_err(|e| ApiError::internal(e.to_string()))
            }
            "backup_schedule" => crate::db::query("DELETE FROM backup_schedules WHERE id = ?")
                .bind(res.id)
                .execute(&state.pool)
                .await
                .map(|_| ())
                .map_err(ApiError::from),
            other => Err(ApiError::internal(format!(
                "unknown stack resource kind '{other}'"
            ))),
        };
        if let Err(e) = result {
            errors.push(format!("{} '{}': {}", res.kind, res.name, e.message));
        }
    }
    if policies {
        vm_netpol::reconcile(&state.pool, false).await;
    }
    errors
}

async fn delete_vm_and_wait(state: &AppState, actor: &AuthUser, id: Uuid) -> Result<(), ApiError> {
    let exists: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Ok(());
    }
    let task = vms::delete_vm(
        State(state.clone()),
        Extension(actor.clone()),
        Path(id),
        None,
    )
    .await?;
    volumes::wait_for_task_timeout(&state.pool, &task.0.task_id, Duration::from_secs(120)).await
}

// ---------------------------------------------------------------------------
// v2: instance groups and policies
// ---------------------------------------------------------------------------

/// Stacks something is building, updating or healing; one at a time per stack.
static BUSY: Mutex<BTreeSet<Uuid>> = Mutex::new(BTreeSet::new());

pub(crate) struct Busy(Uuid);

impl Busy {
    pub(crate) fn try_take(id: Uuid) -> Option<Busy> {
        let mut b = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        // A Busy built while the lock is held would deadlock in Drop.
        if b.insert(id) {
            Some(Busy(id))
        } else {
            None
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

fn busy_error() -> ApiError {
    ApiError::conflict(
        "the stack is being changed right now",
        "wait for the current deploy, update or heal to finish",
    )
    .with_code("stack_busy")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApplyMode {
    /// A new stack: create everything.
    Create,
    /// A new template for an existing stack: create, delete and change.
    Update,
    /// The same template: recreate what went missing, re-apply labels and
    /// policies, report the rest.
    Heal,
    /// Report only.
    Inspect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftItem {
    pub kind: String,
    pub name: String,
    pub detail: String,
    pub fixed: bool,
}

fn item(kind: &str, name: &str, detail: impl Into<String>, fixed: bool) -> DriftItem {
    DriftItem {
        kind: kind.into(),
        name: name.into(),
        detail: detail.into(),
        fixed,
    }
}

async fn stack_policies(pool: &crate::db::DbPool, stack: &str) -> Vec<VmNetworkPolicy> {
    vm_netpol::policies(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.0)
        .filter(|p| p.labels.get(sp::LABEL_STACK).map(String::as_str) == Some(stack))
        .collect()
}

type VmRow = (Uuid, String, i64, i64, Option<String>);

async fn instance_rows(
    pool: &crate::db::DbPool,
    refs: &[StackResourceRef],
) -> Result<BTreeMap<String, VmRow>, ApiError> {
    let mut out = BTreeMap::new();
    for r in refs.iter().filter(|r| r.kind == "instance") {
        let row: Option<VmRow> =
            crate::db::query_as("SELECT id, name, vcpus, memory_mib, labels FROM vms WHERE id = ?")
                .bind(r.id)
                .fetch_optional(pool)
                .await?;
        if let Some(row) = row {
            out.insert(row.1.clone(), row);
        }
    }
    Ok(out)
}

fn parse_labels(s: Option<&str>) -> BTreeMap<String, String> {
    s.and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// Creates the VM for `p`; returns its id and provisioning task.
async fn create_instance(
    state: &AppState,
    actor: &AuthUser,
    ctx: &StackCtx,
    p: &sp::PlannedVm,
) -> Result<(Uuid, String), ApiError> {
    let mut vm = VirtualMachine::new(&p.name, format!("{}Mi", p.memory_mib));
    vm.metadata.project = Some(ctx.project_name.clone());
    vm.spec.cpu.cores = p.vcpus;
    vm.spec.template_ref = p.image.clone();
    vm.spec.ha.enabled = p.ha;
    if let Some(root) = vm.spec.storage.first_mut() {
        root.size = format!("{}Gi", p.disk_gib);
    }
    if let Some(net) = vm.spec.network.first_mut() {
        net.network = p.network.clone();
    }
    let task = vms::create_vm(
        State(state.clone()),
        Extension(actor.clone()),
        Json(CreateVmBody {
            flavor_id: None,
            vm,
            host_id: None,
            tags: p.tags.clone(),
            desired_state: "running".into(),
            atlas_root_disk: false,
            atlas_policy: None,
            preemptible: false,
            preempt_priority: 0,
        }),
    )
    .await?;
    let task_uuid =
        Uuid::parse_str(&task.0.task_id).map_err(|e| ApiError::internal(e.to_string()))?;
    let vm_id: Uuid = crate::db::query_scalar("SELECT resource_id FROM tasks WHERE id = ?")
        .bind(task_uuid)
        .fetch_one(&state.pool)
        .await?;
    write_settings(state, vm_id, p, &BTreeMap::new()).await?;
    if p.ha {
        crate::engine::template::upsert_ha_policy(
            &state.pool,
            vm_id,
            true,
            3,
            "medium",
            false,
            false,
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    Ok((vm_id, task.0.task_id))
}

/// Labels (merged over `existing`), sleep policy and restore point schedule.
async fn write_settings(
    state: &AppState,
    vm_id: Uuid,
    p: &sp::PlannedVm,
    existing: &BTreeMap<String, String>,
) -> Result<(), ApiError> {
    let mut labels = existing.clone();
    labels.extend(p.labels.clone());
    crate::db::query(
        "UPDATE vms SET labels = ?, sleep_after_minutes = ?, restore_point_minutes = ?, restore_point_keep = ? WHERE id = ?",
    )
    .bind(serde_json::to_string(&labels).unwrap_or_else(|_| "{}".into()))
    .bind(p.sleep_after_minutes)
    .bind(p.restore_points.as_ref().map(|r| r.every_minutes))
    .bind(p.restore_points.as_ref().and_then(|r| r.keep))
    .bind(vm_id)
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// Brings the v2 part of a stack to `template`. `refs` is updated in place
/// with what exists afterwards, also when an error is returned.
pub(crate) async fn apply_v2(
    state: &AppState,
    actor: &AuthUser,
    ctx: &StackCtx,
    template: &StackTemplate,
    refs: &mut Vec<StackResourceRef>,
    mode: ApplyMode,
) -> Result<Vec<DriftItem>, ApiError> {
    let fix = mode != ApplyMode::Inspect;
    let cat = catalog(&state.pool).await;
    let planned = sp::expand(&ctx.name, &template.instances, &cat);
    let rows = instance_rows(&state.pool, refs).await?;
    if fix {
        refs.retain(|r| r.kind != "instance" || rows.values().any(|v| v.0 == r.id));
    }
    let actual: Vec<sp::ActualVm> = rows
        .values()
        .map(|(_, name, vcpus, mem, labels)| sp::ActualVm {
            name: name.clone(),
            vcpus: (*vcpus).max(0) as u32,
            memory_mib: *mem,
            labels: parse_labels(labels.as_deref()),
        })
        .collect();
    let want_p = sp::compile_policies(&ctx.name, &template.policies);
    let have_p = stack_policies(&state.pool, &ctx.name).await;
    let d = sp::diff(&planned, &actual, &want_p, &have_p);
    let mut items = Vec::new();
    let mut netpol_changed = false;

    let mut tasks = Vec::new();
    for name in &d.create {
        let p = planned.iter().find(|p| &p.name == name).expect("planned");
        if !fix {
            items.push(item("instance", name, "missing", false));
            continue;
        }
        let taken: Option<(Uuid, Option<String>)> =
            crate::db::query_as("SELECT id, labels FROM vms WHERE name = ?")
                .bind(name)
                .fetch_optional(&state.pool)
                .await?;
        if let Some((id, labels)) = taken {
            let labels = parse_labels(labels.as_deref());
            if labels.get(sp::LABEL_STACK) != Some(&ctx.name) {
                return Err(ApiError::conflict(
                    format!("a VM named '{name}' already exists"),
                    "rename the group or the stack, or remove that VM",
                ));
            }
            refs.push(StackResourceRef {
                kind: "instance".into(),
                id,
                name: name.clone(),
            });
            items.push(item("instance", name, "found untracked; adopted", true));
            continue;
        }
        match create_instance(state, actor, ctx, p).await {
            Ok((id, task)) => {
                refs.push(StackResourceRef {
                    kind: "instance".into(),
                    id,
                    name: name.clone(),
                });
                netpol_changed = true;
                if mode == ApplyMode::Heal {
                    items.push(item("instance", name, "missing; recreated", true));
                }
                tasks.push((name.clone(), task));
            }
            Err(e) if mode == ApplyMode::Heal => {
                items.push(item(
                    "instance",
                    name,
                    format!("missing; recreate failed: {}", e.message),
                    false,
                ));
            }
            Err(e) => return Err(e),
        }
    }
    let waits = tasks.iter().map(|(name, task)| async move {
        (
            name.clone(),
            volumes::wait_for_task_timeout(&state.pool, task, Duration::from_secs(300)).await,
        )
    });
    for (name, r) in futures_util::future::join_all(waits).await {
        if let Err(e) = r {
            if mode == ApplyMode::Heal {
                items.push(item(
                    "instance",
                    &name,
                    format!("recreate failed: {}", e.message),
                    false,
                ));
            } else {
                return Err(ApiError::internal(format!("VM {name}: {}", e.message)));
            }
        }
    }

    for name in &d.delete {
        if mode != ApplyMode::Update {
            items.push(item("instance", name, "not in the template", false));
            continue;
        }
        let Some(r) = refs
            .iter()
            .position(|r| r.kind == "instance" && &r.name == name)
        else {
            continue;
        };
        delete_vm_and_wait(state, actor, refs[r].id).await?;
        refs.remove(r);
        netpol_changed = true;
    }

    for (name, what) in &d.resize {
        items.push(item(
            "instance",
            name,
            format!("{what}; resize it or recreate it"),
            false,
        ));
    }

    for p in &planned {
        let Some(row) = rows.get(&p.name) else {
            continue;
        };
        let relabel = d.relabel.contains(&p.name);
        if relabel && !fix {
            items.push(item("instance", &p.name, "stack labels missing", false));
        }
        if (relabel && fix) || matches!(mode, ApplyMode::Update) {
            write_settings(state, row.0, p, &parse_labels(row.4.as_deref())).await?;
            if relabel {
                netpol_changed = true;
                if mode == ApplyMode::Heal {
                    items.push(item("instance", &p.name, "stack labels re-applied", true));
                }
            }
        }
    }

    sync_backups(state, ctx, template, refs, mode, &mut items).await?;

    for name in &d.policies_upsert {
        let p = want_p.iter().find(|p| &p.name == name).expect("compiled");
        let existed = have_p.iter().any(|h| &h.name == name);
        if !fix {
            items.push(item(
                "policy",
                name,
                if existed {
                    "changed outside the stack"
                } else {
                    "missing"
                },
                false,
            ));
            continue;
        }
        vm_netpol::upsert(&state.pool, p, &actor.username)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        netpol_changed = true;
        if mode == ApplyMode::Heal {
            items.push(item(
                "policy",
                name,
                if existed {
                    "changed outside the stack; re-applied"
                } else {
                    "missing; re-applied"
                },
                true,
            ));
        }
    }
    for name in &d.policies_delete {
        if !fix {
            items.push(item("policy", name, "not in the template", false));
            continue;
        }
        vm_netpol::delete(&state.pool, name)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        netpol_changed = true;
        if mode == ApplyMode::Heal {
            items.push(item("policy", name, "not in the template; removed", true));
        }
    }
    if fix {
        refs.retain(|r| r.kind != "netpol");
        refs.extend(want_p.iter().map(|p| StackResourceRef {
            kind: "netpol".into(),
            id: Uuid::nil(),
            name: p.name.clone(),
        }));
    }
    if netpol_changed {
        vm_netpol::reconcile(&state.pool, false).await;
    }
    Ok(items)
}

/// One backup schedule per group with `backup`, matching the group's tag.
async fn sync_backups(
    state: &AppState,
    ctx: &StackCtx,
    template: &StackTemplate,
    refs: &mut Vec<StackResourceRef>,
    mode: ApplyMode,
    items: &mut Vec<DriftItem>,
) -> Result<(), ApiError> {
    let fix = mode != ApplyMode::Inspect;
    let mut wanted = BTreeSet::new();
    for g in &template.instances {
        let Some(b) = &g.backup else { continue };
        let name = format!("stack-{}-{}", ctx.name, g.name);
        wanted.insert(name.clone());
        let existing = refs
            .iter()
            .find(|r| r.kind == "backup_schedule" && r.name == name)
            .map(|r| r.id);
        let row: Option<(i64, i64)> = match existing {
            Some(id) => {
                crate::db::query_as(
                    "SELECT interval_hours, retain_count FROM backup_schedules WHERE id = ?",
                )
                .bind(id)
                .fetch_optional(&state.pool)
                .await?
            }
            None => None,
        };
        match (existing, row) {
            (Some(id), Some((ih, rc))) => {
                if (ih, rc) != (i64::from(b.interval_hours), i64::from(b.retain)) {
                    if fix {
                        crate::db::query("UPDATE backup_schedules SET interval_hours = ?, retain_count = ? WHERE id = ?")
                            .bind(b.interval_hours)
                            .bind(b.retain)
                            .bind(id)
                            .execute(&state.pool)
                            .await?;
                    }
                    if mode != ApplyMode::Update {
                        items.push(item(
                            "backup",
                            &name,
                            "schedule changed outside the stack",
                            fix,
                        ));
                    }
                }
            }
            (existing, _) => {
                if !fix {
                    items.push(item("backup", &name, "schedule missing", false));
                    continue;
                }
                refs.retain(|r| Some(r.id) != existing || r.kind != "backup_schedule");
                let id = Uuid::new_v4();
                crate::db::query(
                    "INSERT INTO backup_schedules (id, name, project, tag_filter, backup_type, interval_hours, retain_count, enabled)
                     VALUES (?, ?, ?, ?, 'full', ?, ?, TRUE)",
                )
                .bind(id)
                .bind(&name)
                .bind(&ctx.project_name)
                .bind(sp::group_tag(&ctx.name, &g.name))
                .bind(b.interval_hours)
                .bind(b.retain)
                .execute(&state.pool)
                .await?;
                refs.push(StackResourceRef {
                    kind: "backup_schedule".into(),
                    id,
                    name: name.clone(),
                });
                if mode == ApplyMode::Heal {
                    items.push(item("backup", &name, "schedule missing; recreated", true));
                }
            }
        }
    }
    let stale: Vec<StackResourceRef> = refs
        .iter()
        .filter(|r| r.kind == "backup_schedule" && !wanted.contains(&r.name))
        .cloned()
        .collect();
    for r in stale {
        if mode != ApplyMode::Update {
            items.push(item("backup", &r.name, "not in the template", false));
            continue;
        }
        crate::db::query("DELETE FROM backup_schedules WHERE id = ?")
            .bind(r.id)
            .execute(&state.pool)
            .await?;
        refs.retain(|x| x != &r);
    }
    Ok(())
}

async fn store_drift(state: &AppState, id: Uuid, items: &[DriftItem]) {
    let open = items.iter().filter(|i| !i.fixed).count();
    let drift = json!({ "in_sync": open == 0, "open": open, "items": items });
    let _ = crate::db::query(
        "UPDATE stacks SET drift_json = ?, checked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
    )
    .bind(drift)
    .bind(id)
    .execute(&state.pool)
    .await;
}

fn ctx_of(row: &StackRow, project_name: String) -> StackCtx {
    StackCtx {
        id: row.id,
        name: row.name.clone(),
        project_id: row.project_id.unwrap_or_default(),
        project_name,
    }
}

async fn row_ctx(state: &AppState, row: &StackRow) -> Result<StackCtx, ApiError> {
    let project_name: String = match row.project_id {
        Some(p) => crate::db::query_scalar("SELECT name FROM projects WHERE id = ?")
            .bind(p)
            .fetch_optional(&state.pool)
            .await?
            .unwrap_or_else(|| "default".into()),
        None => "default".into(),
    };
    Ok(ctx_of(row, project_name))
}

fn refs_of(row: &StackRow) -> Vec<StackResourceRef> {
    serde_json::from_value(row.resources_json.0.clone()).unwrap_or_default()
}

/// Checks one stack against its template and, with `heal`, fixes what it can.
/// Returns `None` when the stack is busy or not reconcilable.
pub(crate) async fn reconcile_stack(
    state: &AppState,
    id: Uuid,
    heal: bool,
) -> Result<Option<Value>, ApiError> {
    let Some(_busy) = Busy::try_take(id) else {
        return Ok(None);
    };
    let row = load_row(state, id).await?;
    if row.status != "created" || !row.template_json.0.is_v2() {
        return Ok(None);
    }
    let ctx = row_ctx(state, &row).await?;
    let mut refs = refs_of(&row);
    let actor = AuthUser {
        username: "stack-reconcile".into(),
        role: "admin".into(),
        auth_source: None,
    };
    let mode = if heal {
        ApplyMode::Heal
    } else {
        ApplyMode::Inspect
    };
    let items = apply_v2(state, &actor, &ctx, &row.template_json.0, &mut refs, mode).await?;
    if heal && refs != refs_of(&row) {
        let resources_json =
            serde_json::to_value(&refs).map_err(|e| ApiError::internal(e.to_string()))?;
        crate::db::query("UPDATE stacks SET resources_json = ? WHERE id = ?")
            .bind(resources_json)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    store_drift(state, id, &items).await;
    Ok(Some(load_row(state, id).await?.drift_json.0))
}

/// Applies a new template to an existing stack, after [`begin_update`] and
/// while holding its [`Busy`]. On failure the VMs this update created are
/// removed and the old template is re-applied.
pub(crate) async fn run_update(
    state: &AppState,
    actor: AuthUser,
    id: Uuid,
    template: &StackTemplate,
) -> Result<(), ApiError> {
    let row = load_row(state, id).await?;
    let previous: Option<crate::db::Json<StackTemplate>> =
        crate::db::query_scalar("SELECT previous_template_json FROM stacks WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    let ctx = row_ctx(state, &row).await?;
    let before = refs_of(&row);
    let mut refs = before.clone();
    match apply_v2(state, &actor, &ctx, template, &mut refs, ApplyMode::Update).await {
        Ok(_) => {
            store_drift(state, id, &[]).await;
            save_resources(state, id, "created", &refs, None).await
        }
        Err(e) => {
            let added: Vec<StackResourceRef> = refs
                .iter()
                .filter(|r| r.kind == "instance" && !before.contains(r))
                .cloned()
                .collect();
            let mut errors = teardown(state, &actor, &added).await;
            refs.retain(|r| !added.contains(r));
            let old = previous.map(|p| p.0).unwrap_or_default();
            if let Err(e2) = apply_v2(state, &actor, &ctx, &old, &mut refs, ApplyMode::Update).await
            {
                errors.push(e2.message);
            }
            let template_json =
                serde_json::to_value(&old).map_err(|e| ApiError::internal(e.to_string()))?;
            crate::db::query("UPDATE stacks SET template_json = ? WHERE id = ?")
                .bind(template_json)
                .bind(id)
                .execute(&state.pool)
                .await?;
            let msg = if errors.is_empty() {
                format!("update failed and was rolled back: {}", e.message)
            } else {
                format!(
                    "update failed: {}; rollback left: {}",
                    e.message,
                    errors.join("; ")
                )
            };
            save_resources(state, id, "created", &refs, Some(&msg)).await?;
            Err(e)
        }
    }
}

/// Records the new template and marks the stack `updating`.
async fn begin_update(
    state: &AppState,
    id: Uuid,
    template: &StackTemplate,
) -> Result<StackRow, ApiError> {
    let row = load_row(state, id).await?;
    if !matches!(row.status.as_str(), "created" | "error") {
        return Err(ApiError::conflict(
            format!("the stack is {}", row.status),
            "wait until it is created",
        ));
    }
    let template_json =
        serde_json::to_value(template).map_err(|e| ApiError::internal(e.to_string()))?;
    crate::db::query(
        "UPDATE stacks SET previous_template_json = template_json, template_json = ?, status = 'updating' WHERE id = ?",
    )
    .bind(template_json)
    .bind(id)
    .execute(&state.pool)
    .await?;
    Ok(row)
}

#[derive(Debug, Deserialize)]
pub struct UpdateStackBody {
    pub template: StackTemplate,
}

/// `PUT /api/v1/stacks/{id}` — apply a new template now. Only the v2 parts
/// (groups and policies) change; v1 resources stay as they are.
pub async fn update_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateStackBody>,
) -> Result<Json<StackRow>, ApiError> {
    require_operator(&actor)?;
    let row = load_row(&state, id).await?;
    validate_template(&state, &row.name, &body.template).await?;
    let _busy = Busy::try_take(id).ok_or_else(busy_error)?;
    begin_update(&state, id, &body.template).await?;
    run_update(&state, actor, id, &body.template).await?;
    Ok(Json(load_row(&state, id).await?))
}

/// `GET /api/v1/stacks/{id}/drift` — what differs from the template now.
pub async fn get_drift(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    match reconcile_stack(&state, id, false).await? {
        Some(d) => Ok(Json(d)),
        None => Ok(Json(load_row(&state, id).await?.drift_json.0)),
    }
}

/// `POST /api/v1/stacks/{id}/converge` — fix drift now.
pub async fn converge_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let row = load_row(&state, id).await?;
    if !row.template_json.0.is_v2() {
        return Err(ApiError::bad_request(
            "only stacks with instance groups or policies are reconciled",
        ));
    }
    if row.status != "created" {
        return Err(ApiError::conflict(
            format!("the stack is {}", row.status),
            "wait until it is created",
        ));
    }
    reconcile_stack(&state, id, true)
        .await?
        .map(Json)
        .ok_or_else(busy_error)
}

#[derive(Debug, Deserialize)]
pub struct AutoHealBody {
    pub enabled: bool,
}

pub async fn set_auto_heal(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AutoHealBody>,
) -> Result<Json<StackRow>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("UPDATE stacks SET auto_heal = ? WHERE id = ?")
        .bind(body.enabled)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(load_row(&state, id).await?))
}

// ---------------------------------------------------------------------------
// Plan (dry run), draft and approval
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanVm {
    pub name: String,
    pub group: String,
    pub vcpus: u32,
    pub memory_mib: i64,
    pub disk_gib: i64,
    pub image: Option<String>,
    pub host: Option<String>,
    pub monthly_usd: f64,
    /// create, keep, resize or delete.
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCheck {
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackPlanView {
    pub name: String,
    pub project: String,
    pub errors: Vec<String>,
    pub vms: Vec<PlanVm>,
    pub totals: Value,
    pub monthly_usd: f64,
    pub quota: PlanCheck,
    pub placement: PlanCheck,
    pub policies: Vec<StackPolicy>,
    pub policy_yaml: String,
    pub replay: Value,
    pub replay_summary: String,
    pub diff: Option<sp::StackDiff>,
    pub legacy: Value,
}

impl StackPlanView {
    pub fn blocked(&self) -> Option<String> {
        if !self.errors.is_empty() {
            return Some(self.errors.join("; "));
        }
        if !self.quota.ok {
            return Some(self.quota.detail.clone());
        }
        if !self.placement.ok {
            return Some(self.placement.detail.clone());
        }
        None
    }

    fn review(&self) -> String {
        let new = self.vms.iter().filter(|v| v.action == "create").count();
        let gone = self.vms.iter().filter(|v| v.action == "delete").count();
        let mut s = format!(
            "{new} new VM{}{}, ${:.2}/month in total, {} polic{}. {}",
            if new == 1 { "" } else { "s" },
            if gone > 0 {
                format!(", {gone} removed")
            } else {
                String::new()
            },
            self.monthly_usd,
            self.policies.len(),
            if self.policies.len() == 1 { "y" } else { "ies" },
            self.placement.detail
        );
        if !self.replay_summary.is_empty() {
            s.push_str(&format!(" Replay: {}.", self.replay_summary));
        }
        s
    }
}

/// What deploying (or, with `stack_id`, updating to) `template` would do.
pub(crate) async fn build_plan(
    state: &AppState,
    name: &str,
    project_id: Option<Uuid>,
    template: &StackTemplate,
    stack_id: Option<Uuid>,
) -> Result<StackPlanView, ApiError> {
    let project_name = match stack_id {
        Some(id) => {
            row_ctx(state, &load_row(state, id).await?)
                .await?
                .project_name
        }
        None => resolve_project(state, project_id).await?.1,
    };
    let cat = catalog(&state.pool).await;
    let mut errors = Vec::new();
    if let Err(e) = machina_spec::validate_name(name) {
        errors.push(e.to_string());
    }
    if let Err(e) = validate_stack_size(template) {
        errors.push(e.message);
    }
    errors.extend(sp::validate(
        name,
        &template.instances,
        &template.policies,
        &cat,
    ));
    let planned = sp::expand(name, &template.instances, &cat);

    let (rows, diff) = match stack_id {
        Some(id) => {
            let row = load_row(state, id).await?;
            let rows = instance_rows(&state.pool, &refs_of(&row)).await?;
            let actual: Vec<sp::ActualVm> = rows
                .values()
                .map(|(_, n, c, m, l)| sp::ActualVm {
                    name: n.clone(),
                    vcpus: (*c).max(0) as u32,
                    memory_mib: *m,
                    labels: parse_labels(l.as_deref()),
                })
                .collect();
            let want_p = sp::compile_policies(name, &template.policies);
            let have_p = stack_policies(&state.pool, name).await;
            let d = sp::diff(&planned, &actual, &want_p, &have_p);
            (rows, Some(d))
        }
        None => {
            if template.is_v2() {
                let taken: Option<Uuid> =
                    crate::db::query_scalar("SELECT id FROM stacks WHERE name = ?")
                        .bind(name)
                        .fetch_optional(&state.pool)
                        .await?;
                if taken.is_some() {
                    errors.push(format!("a stack named '{name}' already exists"));
                }
            }
            (BTreeMap::new(), None)
        }
    };
    let creating: BTreeSet<&str> = match &diff {
        Some(d) => d.create.iter().map(String::as_str).collect(),
        None => planned.iter().map(|p| p.name.as_str()).collect(),
    };
    if diff.is_none() {
        for p in &planned {
            let taken: Option<Uuid> = crate::db::query_scalar("SELECT id FROM vms WHERE name = ?")
                .bind(&p.name)
                .fetch_optional(&state.pool)
                .await?;
            if taken.is_some() {
                errors.push(format!("a VM named '{}' already exists", p.name));
            }
        }
    }

    let (vcpu_rate, gib_rate): (f64, f64) = crate::db::query_as(
        "SELECT finops_vcpu_hour_usd, finops_gib_hour_usd FROM clusters ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or((0.02, 0.005));

    let legacy_mem: i64 = template
        .vms
        .iter()
        .map(|v| machina_spec::parse_memory_mib(&v.memory).unwrap_or(1024) as i64)
        .sum();
    let legacy_cpu: i64 = template
        .vms
        .iter()
        .map(|v| i64::from(v.cpu_cores.max(1)))
        .sum();
    let legacy_disk: i64 = template.vms.iter().map(|v| v.disk_gib.max(1)).sum::<i64>()
        + template.volumes.iter().map(|v| v.size_gib).sum::<i64>();

    let new: Vec<&sp::PlannedVm> = planned
        .iter()
        .filter(|p| creating.contains(p.name.as_str()))
        .collect();
    let asks: Vec<crate::engine::placement::PlacementAsk> = new
        .iter()
        .map(|p| crate::engine::placement::PlacementAsk {
            name: p.name.clone(),
            tags: p.tags.clone(),
            memory_mib: p.memory_mib,
        })
        .collect();
    let placed = if asks.is_empty() {
        Vec::new()
    } else {
        crate::engine::placement::simulate_placement(&state.pool, &asks)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
    };
    let host_of: BTreeMap<&str, Option<String>> = placed
        .iter()
        .map(|(n, h, _)| (n.as_str(), h.as_ref().map(|h| h.1.clone())))
        .collect();
    let unplaced: Vec<&str> = placed
        .iter()
        .filter(|p| p.1.is_none())
        .map(|p| p.0.as_str())
        .collect();
    let relaxed: Vec<&str> = placed
        .iter()
        .filter(|p| p.2)
        .map(|p| p.0.as_str())
        .collect();
    let mut per_host: BTreeMap<String, usize> = BTreeMap::new();
    for (_, h, _) in &placed {
        if let Some((_, host)) = h {
            *per_host.entry(host.clone()).or_default() += 1;
        }
    }
    let placement = if !unplaced.is_empty() {
        PlanCheck {
            ok: false,
            detail: format!("No host has room for {}.", unplaced.join(", ")),
        }
    } else {
        let spread = per_host
            .iter()
            .map(|(h, n)| format!("{n} on {h}"))
            .collect::<Vec<_>>()
            .join(", ");
        PlanCheck {
            ok: true,
            detail: if placed.is_empty() {
                "Nothing new to place.".into()
            } else if relaxed.is_empty() {
                format!("Placement: {spread}.")
            } else {
                format!(
                    "Placement: {spread}; {} share a host with a group peer (not enough hosts to keep them apart).",
                    relaxed.join(", ")
                )
            },
        }
    };

    let new_vcpus: i64 = new.iter().map(|p| i64::from(p.vcpus)).sum::<i64>() + legacy_cpu;
    let new_mem: i64 = new.iter().map(|p| p.memory_mib).sum::<i64>() + legacy_mem;
    let new_disk: i64 = new.iter().map(|p| p.disk_gib).sum::<i64>() + legacy_disk;
    let quota = match crate::engine::policy::check_project_quota_batch(
        &state.pool,
        &project_name,
        (new.len() + template.vms.len()) as i64,
        new_vcpus as i32,
        new_mem,
        new_disk,
    )
    .await
    {
        Ok(()) => PlanCheck {
            ok: true,
            detail: format!("Fits project '{project_name}' quota."),
        },
        Err(v) => PlanCheck {
            ok: false,
            detail: format!("{} {}", v.message, v.remediation),
        },
    };

    let mut vms: Vec<PlanVm> = planned
        .iter()
        .map(|p| {
            let action = match &diff {
                None => "create",
                Some(d) if d.create.contains(&p.name) => "create",
                Some(d) if d.resize.iter().any(|r| r.0 == p.name) => "resize",
                Some(_) => "keep",
            };
            PlanVm {
                name: p.name.clone(),
                group: p.group.clone(),
                vcpus: p.vcpus,
                memory_mib: p.memory_mib,
                disk_gib: p.disk_gib,
                image: p.image.clone(),
                host: host_of.get(p.name.as_str()).cloned().flatten(),
                monthly_usd: crate::engine::ai::idle::monthly_cost(
                    i64::from(p.vcpus),
                    p.memory_mib,
                    vcpu_rate,
                    gib_rate,
                ),
                action: action.into(),
            }
        })
        .collect();
    if let Some(d) = &diff {
        for name in &d.delete {
            let r = rows.get(name);
            vms.push(PlanVm {
                name: name.clone(),
                group: String::new(),
                vcpus: r.map_or(0, |r| r.2.max(0) as u32),
                memory_mib: r.map_or(0, |r| r.3),
                disk_gib: 0,
                image: None,
                host: None,
                monthly_usd: 0.0,
                action: "delete".into(),
            });
        }
    }

    let compiled = sp::compile_policies(name, &template.policies);
    let policy_yaml = machina_bpf::netpol::nl::to_yaml(&compiled);
    let (replay, replay_summary) = if compiled.is_empty() {
        (Value::Null, String::new())
    } else {
        match crate::api::vm_network_policies::replay_draft(state, compiled, 5000).await {
            Ok(r) => {
                let s = crate::api::vm_network_policies::replay_summary(&r);
                (r, s)
            }
            Err(e) => (Value::Null, format!("replay unavailable: {}", e.message)),
        }
    };

    let total_vcpus: u32 = planned.iter().map(|p| p.vcpus).sum();
    let total_mem: i64 = planned.iter().map(|p| p.memory_mib).sum();
    let total_disk: i64 = planned.iter().map(|p| p.disk_gib).sum();
    Ok(StackPlanView {
        name: name.into(),
        project: project_name,
        errors,
        totals: json!({
            "vms": planned.len(),
            "vcpus": total_vcpus,
            "memory_mib": total_mem,
            "storage_gib": total_disk,
        }),
        monthly_usd: sp::monthly_cost(&planned, vcpu_rate, gib_rate),
        vms,
        quota,
        placement,
        policies: template.policies.clone(),
        policy_yaml,
        replay,
        replay_summary,
        diff,
        legacy: json!({
            "security_groups": template.security_groups.len(),
            "volumes": template.volumes.len(),
            "vms": template.vms.len(),
        }),
    })
}

#[derive(Debug, Deserialize)]
pub struct PlanBody {
    pub name: String,
    pub template: StackTemplate,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub stack_id: Option<Uuid>,
}

/// `POST /api/v1/stacks/plan` — quota, placement, monthly cost and a replay of
/// the policies against recorded traffic. Nothing is created.
pub async fn plan_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<PlanBody>,
) -> Result<Json<StackPlanView>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        build_plan(
            &state,
            &body.name,
            body.project_id,
            &body.template,
            body.stack_id,
        )
        .await?,
    ))
}

const MAX_PROMPT: usize = 4000;

#[derive(Debug, Deserialize)]
pub struct DraftBody {
    pub prompt: String,
    pub name: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub rules_only: bool,
}

/// Zyvor's LLM when configured, with one repair round; the rule-based drafter
/// otherwise or when the LLM's draft is unusable.
pub(crate) async fn draft_template(
    state: &AppState,
    name: &str,
    prompt: &str,
    rules_only: bool,
) -> (sp::DraftTemplate, &'static str, Vec<String>) {
    let cat = catalog(&state.pool).await;
    let mut notes = Vec::new();
    if !rules_only {
        let system = sp::llm_system_prompt(name, &cat);
        if let Ok(Some(reply)) =
            crate::engine::ai::llm::complete_simple(&state.pool, &system, prompt).await
        {
            if let Some(why) = sp::llm_declined(&reply) {
                notes.push(format!("Zyvor declined: {why}"));
            } else {
                match sp::accept_llm(name, &reply, &cat) {
                    Ok(t) => return (t, "llm", notes),
                    Err(errors) => {
                        let repair =
                            sp::llm_repair_prompt(prompt, sp::extract_json(&reply), &errors);
                        let errors = match crate::engine::ai::llm::complete_simple(
                            &state.pool,
                            &system,
                            &repair,
                        )
                        .await
                        {
                            Ok(Some(r)) => match sp::accept_llm(name, &r, &cat) {
                                Ok(t) => return (t, "llm", notes),
                                Err(e) => e,
                            },
                            _ => errors,
                        };
                        notes.push(format!("Zyvor's draft was rejected: {}", errors.join("; ")));
                    }
                }
            }
        }
    }
    let (t, more) = sp::draft_rules(prompt);
    notes.extend(more);
    (t, "rules", notes)
}

/// `POST /api/v1/stacks/draft` — a template and its plan from plain English.
pub async fn draft_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<DraftBody>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    let prompt = body.prompt.trim();
    if prompt.is_empty() || prompt.len() > MAX_PROMPT {
        return Err(ApiError::bad_request(format!(
            "describe the stack in 1-{MAX_PROMPT} characters"
        )));
    }
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (draft, source, notes) = draft_template(&state, &body.name, prompt, body.rules_only).await;
    let template = StackTemplate {
        instances: draft.instances,
        policies: draft.policies,
        ..Default::default()
    };
    let plan = build_plan(&state, &body.name, body.project_id, &template, None).await?;
    Ok(Json(json!({
        "template": template,
        "source": source,
        "notes": notes,
        "plan": plan,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ProposeBody {
    pub name: String,
    pub template: StackTemplate,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub stack_id: Option<Uuid>,
    #[serde(default)]
    pub prompt: Option<String>,
}

/// `POST /api/v1/stacks/propose` — queue a deploy (or an update with
/// `stack_id`) for approval. Refused when the plan would fail.
pub async fn propose_stack(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<ProposeBody>,
) -> Result<Json<crate::engine::ai::actions::ZyraActionRow>, ApiError> {
    require_operator(&actor)?;
    let name = match body.stack_id {
        Some(id) => load_row(&state, id).await?.name,
        None => body.name.clone(),
    };
    let plan = build_plan(
        &state,
        &name,
        body.project_id,
        &body.template,
        body.stack_id,
    )
    .await?;
    if let Some(why) = plan.blocked() {
        return Err(ApiError::conflict(
            format!("the plan would fail: {why}"),
            "fix the template and plan again",
        )
        .with_code("stack_plan_blocked"));
    }
    let project_id = match body.stack_id {
        Some(id) => load_row(&state, id).await?.project_id.unwrap_or_default(),
        None => resolve_project(&state, body.project_id).await?.0,
    };
    let update = body.stack_id.is_some();
    let action = crate::engine::ai::actions::create_action(
        &state.pool,
        &crate::engine::ai::actions::CreateActionBody {
            action_type: DEPLOY_ACTION.into(),
            label: format!("{} stack {name}", if update { "Update" } else { "Deploy" }),
            review: plan.review(),
            risk: if plan.vms.iter().any(|v| v.action == "delete") {
                "Removes VMs".into()
            } else {
                "Review required".into()
            },
            object_ref: json!({
                "stack_id": body.stack_id.unwrap_or_else(Uuid::new_v4),
                "name": name,
                "project_id": project_id,
                "template": body.template,
                "update": update,
                "prompt": body.prompt,
                "monthly_usd": plan.monthly_usd,
            }),
            source: "stacks".into(),
        },
        &actor.username,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(action))
}

#[derive(Debug, Deserialize)]
struct DeployRef {
    stack_id: Uuid,
    name: String,
    project_id: Uuid,
    template: StackTemplate,
    #[serde(default)]
    update: bool,
}

/// Runs an approved `stack.deploy`: starts the deploy or update in the
/// background and returns at once.
pub(crate) async fn execute_deploy(
    state: &AppState,
    actor: &AuthUser,
    object_ref: &Value,
) -> Result<Value, ApiError> {
    let r: DeployRef = serde_json::from_value(object_ref.clone())
        .map_err(|e| ApiError::bad_request(format!("stack.deploy object_ref: {e}")))?;
    validate_template(state, &r.name, &r.template).await?;
    let state2 = state.clone();
    let actor2 = actor.clone();
    if r.update {
        let busy = Busy::try_take(r.stack_id).ok_or_else(busy_error)?;
        begin_update(state, r.stack_id, &r.template).await?;
        let template = r.template.clone();
        tokio::spawn(async move {
            let _busy = busy;
            if let Err(e) = run_update(&state2, actor2, r.stack_id, &template).await {
                tracing::warn!(stack = %r.stack_id, "stack update failed: {}", e.message);
            }
        });
        return Ok(
            json!({ "message": format!("Updating stack {}", r.name), "stack_id": r.stack_id }),
        );
    }
    ensure_name_free(state, &r.name).await?;
    let project_name: String = crate::db::query_scalar("SELECT name FROM projects WHERE id = ?")
        .bind(r.project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project not found"))?;
    let ctx = StackCtx {
        id: r.stack_id,
        name: r.name.clone(),
        project_id: r.project_id,
        project_name,
    };
    insert_row(state, &ctx, &r.template).await?;
    let template = r.template.clone();
    tokio::spawn(async move {
        if let Err(e) = run_create(&state2, actor2, &ctx, &template).await {
            tracing::warn!(stack = %ctx.id, "stack deploy failed: {}", e.message);
        }
    });
    Ok(json!({ "message": format!("Deploying stack {}", r.name), "stack_id": r.stack_id }))
}

/// Before-state for `stack.deploy`: whether the stack existed and its template.
pub(crate) async fn deploy_before(state: &AppState, object_ref: &Value) -> Value {
    let id = object_ref
        .get("stack_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok());
    let row = match id {
        Some(id) => load_row(state, id).await.ok(),
        None => None,
    };
    json!({
        "stack_id": id,
        "existed": row.is_some(),
        "template": row.map(|r| serde_json::to_value(&r.template_json.0).unwrap_or(Value::Null)),
    })
}

/// (status, detail) of a deployed stack, for verify.
pub(crate) async fn deploy_check(state: &AppState, object_ref: &Value) -> (&'static str, String) {
    let Some(id) = object_ref
        .get("stack_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return ("unknown", "No stack id on this action.".into());
    };
    let Ok(row) = load_row(state, id).await else {
        return ("failed", "The stack no longer exists.".into());
    };
    let open = row
        .drift_json
        .0
        .get("open")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    match row.status.as_str() {
        "created" if row.last_error.is_none() && open == 0 => (
            "ok",
            "The stack is created and matches its template.".into(),
        ),
        "created" if open > 0 => ("pending", format!("The stack has {open} drift item(s).")),
        "created" => ("failed", row.last_error.unwrap_or_default()),
        "creating" | "updating" => ("pending", format!("The stack is {}.", row.status)),
        other => (
            "failed",
            format!(
                "The stack is {other}: {}",
                row.last_error.unwrap_or_default()
            ),
        ),
    }
}

/// Undo `stack.deploy`: delete a new stack, or re-apply the previous template.
pub(crate) async fn deploy_undo(
    state: &AppState,
    actor: &AuthUser,
    before: &Value,
) -> Result<String, ApiError> {
    let id = before
        .get("stack_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| ApiError::bad_request("no stack id recorded"))?;
    if before.get("existed").and_then(Value::as_bool) != Some(true) {
        let _busy = Busy::try_take(id).ok_or_else(busy_error)?;
        let errors = delete_stack_id(state, actor, id).await?;
        return if errors.is_empty() {
            Ok("Stack deleted.".into())
        } else {
            Err(ApiError::internal(errors.join("; ")))
        };
    }
    let template: StackTemplate = serde_json::from_value(before["template"].clone())
        .map_err(|e| ApiError::internal(format!("previous template: {e}")))?;
    let _busy = Busy::try_take(id).ok_or_else(busy_error)?;
    begin_update(state, id, &template).await?;
    run_update(state, actor.clone(), id, &template).await?;
    Ok("Previous template re-applied.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::{seed_host, test_state};

    async fn seed(state: &AppState) -> Uuid {
        let pool = &state.pool;
        seed_host(pool, Uuid::from_u128(7)).await;
        crate::db::query("UPDATE hosts SET memory_total_mib = 65536, memory_used_mib = 0")
            .execute(pool)
            .await
            .unwrap();
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c1')")
            .bind(Uuid::from_u128(1))
            .execute(pool)
            .await
            .unwrap();
        let project = Uuid::from_u128(9);
        crate::db::query("INSERT INTO projects (id, name) VALUES (?, 'dev')")
            .bind(project)
            .execute(pool)
            .await
            .unwrap();
        project
    }

    fn template() -> StackTemplate {
        serde_json::from_value(json!({
            "instances": [
                { "name": "web", "count": 2, "cpu_cores": 2, "memory": "2Gi", "anti_affinity": true },
                { "name": "db", "memory": "4Gi" }
            ],
            "policies": [{ "from": "web", "to": "db", "ports": [5432] }]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn plan_counts_cost_quota_and_placement() {
        let (state, _rx) = test_state().await;
        let project = seed(&state).await;
        let plan = build_plan(&state, "shop", Some(project), &template(), None)
            .await
            .unwrap();
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(plan.totals["vms"], 3);
        assert_eq!(plan.totals["memory_mib"], 8192);
        assert!(plan.placement.ok, "{}", plan.placement.detail);
        assert!(plan
            .vms
            .iter()
            .all(|v| v.host.as_deref() == Some("h1") && v.action == "create"));
        assert!(
            plan.placement.detail.contains("share a host"),
            "{}",
            plan.placement.detail
        );
        assert!(plan.quota.ok, "{}", plan.quota.detail);
        assert!(plan.monthly_usd > 0.0);
        assert!(plan.policy_yaml.contains("stack-shop-db"));
        assert!(plan.blocked().is_none());

        crate::db::query("INSERT INTO project_quotas (project, max_vms) VALUES ('dev', 2)")
            .execute(&state.pool)
            .await
            .unwrap();
        let plan = build_plan(&state, "shop", Some(project), &template(), None)
            .await
            .unwrap();
        assert!(!plan.quota.ok);
        assert!(plan.blocked().unwrap().contains("VM count quota"));

        crate::db::query("UPDATE hosts SET memory_used_mib = 64000")
            .execute(&state.pool)
            .await
            .unwrap();
        let plan = build_plan(&state, "shop", Some(project), &template(), None)
            .await
            .unwrap();
        assert!(!plan.placement.ok);
    }

    #[tokio::test]
    async fn inspect_reports_missing_instances_and_policies() {
        let (state, _rx) = test_state().await;
        let project = seed(&state).await;
        let ctx = StackCtx {
            id: Uuid::new_v4(),
            name: "shop".into(),
            project_id: project,
            project_name: "dev".into(),
        };
        insert_row(&state, &ctx, &template()).await.unwrap();
        save_resources(&state, ctx.id, "created", &[], None)
            .await
            .unwrap();
        let drift = reconcile_stack(&state, ctx.id, false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(drift["in_sync"], false);
        assert_eq!(drift["open"], 4, "{drift}");
        let names: Vec<&str> = drift["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["shop-web-1", "shop-web-2", "shop-db", "stack-shop-db"]
        );

        let held = Busy::try_take(ctx.id).unwrap();
        assert!(reconcile_stack(&state, ctx.id, false)
            .await
            .unwrap()
            .is_none());
        drop(held);
    }

    #[tokio::test]
    async fn propose_refuses_a_blocked_plan_and_queues_a_good_one() {
        let (state, _rx) = test_state().await;
        let project = seed(&state).await;
        let actor = AuthUser {
            username: "op".into(),
            role: "admin".into(),
            auth_source: None,
        };
        let mut bad = template();
        bad.instances[0].count = 0;
        let err = propose_stack(
            State(state.clone()),
            Extension(actor.clone()),
            Json(ProposeBody {
                name: "shop".into(),
                template: bad,
                project_id: Some(project),
                stack_id: None,
                prompt: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.error_code.as_deref(), Some("stack_plan_blocked"));
        let row = propose_stack(
            State(state.clone()),
            Extension(actor.clone()),
            Json(ProposeBody {
                name: "shop".into(),
                template: template(),
                project_id: Some(project),
                stack_id: None,
                prompt: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(row.action_type, DEPLOY_ACTION);
        assert_eq!(row.status, "pending");
        assert_eq!(row.object_ref["update"], false);
        assert!(row.review.contains("3 new VMs"), "{}", row.review);
    }
}
