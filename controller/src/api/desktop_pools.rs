// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Simple VDI desktop pools (see `engine/desktop_pools.rs` for the rules). A pool clones a stopped
//! template VM into numbered desktops; a user checks one out (it is powered on if needed) and
//! releases it when done. A non-persistent pool deletes and re-clones the desktop on release.
//! Clones, power changes and deletes reuse the existing `vm.clone`, `vm.power` and `vm.delete`
//! tasks, so desktops appear as ordinary VMs. Nothing runs by itself: `reconcile` (also called by
//! checkout, release and scale) advances members, so call it again until clones finish.

use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::engine::desktop_pools as rules;
use crate::state::AppState;
use crate::tasks::enqueue::{enqueue_task, write_audit};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PoolRow {
    pub id: Uuid,
    pub name: String,
    pub template_vm_id: Uuid,
    pub prefix: String,
    pub size: i64,
    pub clone_mode: String,
    pub persistent: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct MemberDb {
    id: Uuid,
    vm_name: String,
    assigned_user: String,
    assigned_at: String,
    state: String,
    message: String,
}

/// A desktop as returned to clients, with the live VM it maps to once the clone exists.
#[derive(Debug, Serialize)]
pub struct MemberView {
    pub id: Uuid,
    pub vm_name: String,
    pub vm_id: Option<Uuid>,
    pub vm_state: Option<String>,
    pub assigned_user: String,
    pub assigned_at: String,
    pub state: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PoolDetail {
    #[serde(flatten)]
    pub pool: PoolRow,
    pub members: Vec<MemberView>,
}

const POOL_COLS: &str =
    "id, name, template_vm_id, prefix, size, clone_mode, persistent, created_by, created_at, updated_at";

async fn load_pool(state: &AppState, id: Uuid) -> Result<PoolRow, ApiError> {
    crate::db::query_as(&format!(
        "SELECT {POOL_COLS} FROM desktop_pools WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("pool not found"))
}

async fn members_db(state: &AppState, pool_id: Uuid) -> Result<Vec<MemberDb>, ApiError> {
    Ok(crate::db::query_as(
        "SELECT id, vm_name, assigned_user, assigned_at, state, message
         FROM desktop_pool_members WHERE pool_id = ? ORDER BY vm_name",
    )
    .bind(pool_id)
    .fetch_all(&state.pool)
    .await?)
}

async fn vm_by_name(state: &AppState, name: &str) -> Result<Option<(Uuid, String)>, ApiError> {
    Ok(
        crate::db::query_as("SELECT id, observed_state FROM vms WHERE name = ? LIMIT 1")
            .bind(name)
            .fetch_optional(&state.pool)
            .await?,
    )
}

async fn detail(state: &AppState, id: Uuid) -> Result<PoolDetail, ApiError> {
    let pool = load_pool(state, id).await?;
    let mut members = Vec::new();
    for m in members_db(state, id).await? {
        let vm = vm_by_name(state, &m.vm_name).await?;
        members.push(MemberView {
            id: m.id,
            vm_id: vm.as_ref().map(|v| v.0),
            vm_state: vm.map(|v| v.1),
            vm_name: m.vm_name,
            assigned_user: m.assigned_user,
            assigned_at: m.assigned_at,
            state: m.state,
            message: m.message,
        });
    }
    Ok(PoolDetail { pool, members })
}

async fn set_member(
    state: &AppState,
    id: Uuid,
    new_state: &str,
    message: &str,
    clear_assignment: bool,
) -> Result<(), ApiError> {
    let sql = if clear_assignment {
        "UPDATE desktop_pool_members SET state = ?, message = ?, assigned_user = '', assigned_at = '',
                updated_at = datetime('now') WHERE id = ?"
    } else {
        "UPDATE desktop_pool_members SET state = ?, message = ?, updated_at = datetime('now') WHERE id = ?"
    };
    crate::db::query(sql)
        .bind(new_state)
        .bind(message)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

/// Queue deletion of a desktop VM, marking it deleting first so reconcile loops leave it alone.
async fn delete_vm(state: &AppState, vm_id: Uuid) -> Result<(), ApiError> {
    let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    crate::engine::vm_lifecycle::set_vm_phase(
        &state.pool,
        vm_id,
        crate::engine::vm_lifecycle::PHASE_DELETING,
    )
    .await?;
    enqueue_task(
        state,
        "vm.delete",
        serde_json::json!({ "vm_id": vm_id.to_string() }),
        Some("vm"),
        Some(vm_id),
        host,
    )
    .await?;
    Ok(())
}

/// Start (`start`) or stop (`shutdown`) a desktop VM, recording the intent so reconcile agrees.
async fn power(state: &AppState, vm_id: Uuid, action: &str) -> Result<(), ApiError> {
    let host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
        .bind(vm_id)
        .fetch_one(&state.pool)
        .await?;
    let host = host.ok_or_else(|| ApiError::bad_request("the desktop has no host yet"))?;
    let desired = if action == "start" {
        "running"
    } else {
        "stopped"
    };
    crate::db::query("UPDATE vms SET desired_state = ? WHERE id = ?")
        .bind(desired)
        .bind(vm_id)
        .execute(&state.pool)
        .await?;
    enqueue_task(
        state,
        "vm.power",
        serde_json::json!({ "vm_id": vm_id.to_string(), "action": action }),
        Some("vm"),
        Some(vm_id),
        Some(host),
    )
    .await?;
    Ok(())
}

/// Advance every member and drop surplus ones. Idempotent.
async fn reconcile_pool(state: &AppState, pool_id: Uuid) -> Result<(), ApiError> {
    let pool = load_pool(state, pool_id).await?;
    let members = members_db(state, pool_id).await?;
    let template_host: Option<Uuid> =
        crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(pool.template_vm_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();

    // Surplus (after a shrink): unassigned members beyond the pool size go away.
    let infos: Vec<rules::Member> = members
        .iter()
        .map(|m| rules::Member {
            id: m.id,
            vm_name: m.vm_name.clone(),
            assigned_user: m.assigned_user.clone(),
            state: m.state.clone(),
        })
        .collect();
    let plan = rules::plan_scale(&infos, &pool.prefix, pool.size);
    for id in &plan.remove {
        if let Some(m) = members.iter().find(|m| m.id == *id) {
            if let Some((vm_id, _)) = vm_by_name(state, &m.vm_name).await? {
                delete_vm(state, vm_id).await?;
            }
        }
        crate::db::query("DELETE FROM desktop_pool_members WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    for m in members.iter().filter(|m| !plan.remove.contains(&m.id)) {
        let vm = vm_by_name(state, &m.vm_name).await?;
        let view = vm.as_ref().map(|(_, s)| rules::VmView {
            observed_state: s.clone(),
        });
        let step = rules::next_step(&m.state, !m.assigned_user.is_empty(), view.as_ref());
        let mut new_state = step.state;
        let mut message = String::new();
        if step.action == rules::Action::EnqueueClone {
            let queued = enqueue_task(
                state,
                "vm.clone",
                serde_json::json!({
                    "vm_id": pool.template_vm_id.to_string(),
                    "new_name": m.vm_name,
                    "clone_mode": pool.clone_mode,
                }),
                Some("vm"),
                Some(pool.template_vm_id),
                template_host,
            )
            .await;
            // A clone that cannot be queued is retried by the next reconcile via `error`.
            if let Err(e) = queued {
                new_state = "error";
                message = format!("could not queue the clone: {}", e.message);
            }
        }
        if new_state != m.state || step.clear_assignment || !message.is_empty() {
            set_member(state, m.id, new_state, &message, step.clear_assignment).await?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub template_vm_id: Uuid,
    /// Desktop names are `<prefix>-01`...; defaults to the pool name.
    #[serde(default)]
    pub prefix: Option<String>,
    pub size: i64,
    /// `linked` (default, shares the template disk) or `full`.
    #[serde(default)]
    pub clone_mode: Option<String>,
    /// Keep a user's desktop and its disk between sessions. Default false: re-clone on release.
    #[serde(default)]
    pub persistent: bool,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateBody>,
) -> Result<Json<PoolDetail>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let prefix = body.prefix.clone().unwrap_or_else(|| body.name.clone());
    rules::validate_prefix(&prefix).map_err(ApiError::bad_request)?;
    rules::validate_size(body.size).map_err(ApiError::bad_request)?;
    let clone_mode = body.clone_mode.clone().unwrap_or_else(|| "linked".into());
    rules::validate_clone_mode(&clone_mode).map_err(ApiError::bad_request)?;

    let template: Option<(String,)> =
        crate::db::query_as("SELECT observed_state FROM vms WHERE id = ?")
            .bind(body.template_vm_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((tstate,)) = template else {
        return Err(ApiError::bad_request("template VM not found"));
    };
    rules::template_ok(&tstate).map_err(ApiError::bad_request)?;

    // Refuse names that already belong to some other VM.
    for n in rules::member_names(&prefix, body.size) {
        if vm_by_name(&state, &n).await?.is_some() {
            return Err(ApiError::conflict(
                format!("a VM named {n} already exists"),
                "choose another prefix",
            ));
        }
    }

    let id = Uuid::new_v4();
    let mut tx = state.pool.begin().await?;
    crate::db::query(
        "INSERT INTO desktop_pools (id, name, template_vm_id, prefix, size, clone_mode, persistent, created_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&body.name)
    .bind(body.template_vm_id)
    .bind(&prefix)
    .bind(body.size)
    .bind(&clone_mode)
    .bind(body.persistent)
    .bind(&actor.username)
    .execute(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            ApiError::conflict("a pool with that name exists", "choose another name")
        }
        other => other.into(),
    })?;
    for n in rules::member_names(&prefix, body.size) {
        crate::db::query(
            "INSERT INTO desktop_pool_members (id, pool_id, vm_name) VALUES (?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(n)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    write_audit(
        &state,
        &actor.username,
        "desktop_pool.create",
        "desktop_pool",
        Some(id),
        serde_json::json!({ "name": body.name, "size": body.size, "persistent": body.persistent }),
    )
    .await?;
    reconcile_pool(&state, id).await?;
    detail(&state, id).await.map(Json)
}

pub async fn list(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<PoolRow>>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(
        crate::db::query_as(&format!(
            "SELECT {POOL_COLS} FROM desktop_pools ORDER BY name LIMIT 500"
        ))
        .fetch_all(&state.pool)
        .await?,
    ))
}

pub async fn get(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<PoolDetail>, ApiError> {
    require_operator(&actor)?;
    detail(&state, id).await.map(Json)
}

pub async fn reconcile(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<PoolDetail>, ApiError> {
    require_operator(&actor)?;
    reconcile_pool(&state, id).await?;
    detail(&state, id).await.map(Json)
}

#[derive(Debug, Deserialize)]
pub struct ScaleBody {
    pub size: i64,
}

pub async fn scale(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ScaleBody>,
) -> Result<Json<PoolDetail>, ApiError> {
    require_operator(&actor)?;
    rules::validate_size(body.size).map_err(ApiError::bad_request)?;
    let pool = load_pool(&state, id).await?;
    let existing: Vec<rules::Member> = members_db(&state, id)
        .await?
        .into_iter()
        .map(|m| rules::Member {
            id: m.id,
            vm_name: m.vm_name,
            assigned_user: m.assigned_user,
            state: m.state,
        })
        .collect();
    let plan = rules::plan_scale(&existing, &pool.prefix, body.size);
    for n in &plan.create {
        if vm_by_name(&state, n).await?.is_some() {
            return Err(ApiError::conflict(
                format!("a VM named {n} already exists"),
                "rename or delete it first",
            ));
        }
    }
    for n in &plan.create {
        crate::db::query(
            "INSERT INTO desktop_pool_members (id, pool_id, vm_name) VALUES (?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(n)
        .execute(&state.pool)
        .await?;
    }
    crate::db::query(
        "UPDATE desktop_pools SET size = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(body.size)
    .bind(id)
    .execute(&state.pool)
    .await?;
    write_audit(
        &state,
        &actor.username,
        "desktop_pool.scale",
        "desktop_pool",
        Some(id),
        serde_json::json!({ "size": body.size, "blocked": plan.blocked }),
    )
    .await?;
    // Removes the unassigned surplus; assigned ones stay until released.
    reconcile_pool(&state, id).await?;
    detail(&state, id).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub struct UserBody {
    /// Defaults to the caller. Acting for someone else needs the operator role.
    #[serde(default)]
    pub user: Option<String>,
}

fn acting_user(actor: &AuthUser, body: &Option<Json<UserBody>>) -> Result<String, ApiError> {
    let user = body
        .as_ref()
        .and_then(|b| b.0.user.clone())
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| actor.username.clone());
    if user != actor.username {
        require_operator(actor)?;
    }
    rules::validate_user(&user).map_err(ApiError::bad_request)?;
    Ok(user.trim().to_string())
}

#[derive(Debug, Serialize)]
pub struct CheckoutResult {
    pub pool: String,
    pub user: String,
    pub desktop: MemberView,
}

/// Give the user a desktop (the one they hold, or the first free one) and power it on.
pub async fn checkout(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<UserBody>>,
) -> Result<Json<CheckoutResult>, ApiError> {
    let user = acting_user(&actor, &body)?;
    let pool = load_pool(&state, id).await?;
    reconcile_pool(&state, id).await?;

    let mut chosen: Option<Uuid> = None;
    for _ in 0..3 {
        let infos: Vec<rules::Member> = members_db(&state, id)
            .await?
            .into_iter()
            .map(|m| rules::Member {
                id: m.id,
                vm_name: m.vm_name,
                assigned_user: m.assigned_user,
                state: m.state,
            })
            .collect();
        match rules::pick_for_user(&infos, &user) {
            rules::Pick::Existing(m) => {
                chosen = Some(m);
                break;
            }
            rules::Pick::Free(m) => {
                // Claim it only if still free; the unique index also stops a second desktop per user.
                let res = crate::db::query(
                    "UPDATE desktop_pool_members SET assigned_user = ?, assigned_at = datetime('now'),
                            state = 'assigned', message = '', updated_at = datetime('now')
                     WHERE id = ? AND assigned_user = ''",
                )
                .bind(&user)
                .bind(m)
                .execute(&state.pool)
                .await?;
                if res.rows_affected() == 1 {
                    chosen = Some(m);
                    break;
                }
            }
            rules::Pick::None => break,
        }
    }
    let Some(member_id) = chosen else {
        return Err(ApiError::conflict(
            "no desktop is free right now",
            "wait for clones to finish, release an idle desktop, or scale the pool up",
        ));
    };

    let m = members_db(&state, id)
        .await?
        .into_iter()
        .find(|m| m.id == member_id)
        .ok_or_else(|| ApiError::internal("desktop vanished"))?;
    let vm = vm_by_name(&state, &m.vm_name).await?;
    if let Some((vm_id, vm_state)) = &vm {
        if vm_state != "running" {
            power(&state, *vm_id, "start").await?;
        }
    }
    write_audit(
        &state,
        &actor.username,
        "desktop_pool.checkout",
        "desktop_pool",
        Some(id),
        serde_json::json!({ "user": user, "desktop": m.vm_name }),
    )
    .await?;
    Ok(Json(CheckoutResult {
        pool: pool.name,
        user,
        desktop: MemberView {
            id: m.id,
            vm_id: vm.as_ref().map(|v| v.0),
            vm_state: vm.map(|v| v.1),
            vm_name: m.vm_name,
            assigned_user: m.assigned_user,
            assigned_at: m.assigned_at,
            state: m.state,
            message: m.message,
        },
    }))
}

/// Give the desktop back: a persistent pool stops it and keeps the disk; otherwise it is deleted
/// and re-cloned from the template.
pub async fn release(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    body: Option<Json<UserBody>>,
) -> Result<Json<PoolDetail>, ApiError> {
    let user = acting_user(&actor, &body)?;
    let pool = load_pool(&state, id).await?;
    let m = members_db(&state, id)
        .await?
        .into_iter()
        .find(|m| m.assigned_user == user)
        .ok_or_else(|| ApiError::not_found("that user holds no desktop in this pool"))?;
    let vm = vm_by_name(&state, &m.vm_name).await?;
    if pool.persistent {
        set_member(&state, m.id, "ready", "", true).await?;
        if let Some((vm_id, _)) = vm {
            power(&state, vm_id, "shutdown").await?;
        }
    } else {
        set_member(&state, m.id, "recycling", "", true).await?;
        if let Some((vm_id, _)) = vm {
            delete_vm(&state, vm_id).await?;
        }
    }
    write_audit(
        &state,
        &actor.username,
        "desktop_pool.release",
        "desktop_pool",
        Some(id),
        serde_json::json!({ "user": user, "desktop": m.vm_name, "persistent": pool.persistent }),
    )
    .await?;
    detail(&state, id).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub struct DeleteQuery {
    /// Also delete the desktop VMs. Without it only the pool record goes and the VMs stay.
    #[serde(default)]
    pub delete_vms: bool,
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(q): Query<DeleteQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let pool = load_pool(&state, id).await?;
    if q.delete_vms {
        for m in members_db(&state, id).await? {
            if let Some((vm_id, _)) = vm_by_name(&state, &m.vm_name).await? {
                delete_vm(&state, vm_id).await?;
            }
        }
    }
    crate::db::query("DELETE FROM desktop_pools WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "desktop_pool.delete",
        "desktop_pool",
        Some(id),
        serde_json::json!({ "name": pool.name, "delete_vms": q.delete_vms }),
    )
    .await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_desktop_per_user_and_cascade() {
        let pool = crate::db::testing::pool().await;
        let p = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO desktop_pools (id, name, template_vm_id, prefix, size) VALUES (?, 'sales', ?, 'sales', 2)",
        )
        .bind(p)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for (id, name) in [(a, "sales-01"), (b, "sales-02")] {
            crate::db::query(
                "INSERT INTO desktop_pool_members (id, pool_id, vm_name) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(p)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
        }
        crate::db::query("UPDATE desktop_pool_members SET assigned_user = 'alice', state = 'assigned' WHERE id = ?")
            .bind(a)
            .execute(&pool)
            .await
            .unwrap();
        // The same user cannot hold a second desktop in the pool...
        let second = crate::db::query(
            "UPDATE desktop_pool_members SET assigned_user = 'alice' WHERE id = ?",
        )
        .bind(b)
        .execute(&pool)
        .await;
        assert!(second.is_err());
        // ...but any number of unassigned members is fine (empty string is excluded by the index).
        crate::db::query(
            "INSERT INTO desktop_pool_members (id, pool_id, vm_name) VALUES (?, ?, 'sales-03')",
        )
        .bind(Uuid::new_v4())
        .bind(p)
        .execute(&pool)
        .await
        .unwrap();
        // Defaults: a new pool is non-persistent.
        let persistent: bool =
            crate::db::query_scalar("SELECT persistent FROM desktop_pools WHERE id = ?")
                .bind(p)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!persistent);
        crate::db::query("DELETE FROM desktop_pools WHERE id = ?")
            .bind(p)
            .execute(&pool)
            .await
            .unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM desktop_pool_members")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
