// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! vGPU (mediated device) partitions across the fleet. The agent on each host lists, creates and
//! removes partitions and attaches them to VMs (see `machina_core::libvirt::mdev`); this module is
//! the controller's view of that and the bookkeeping that keeps a VM holding a partition on its host
//! (`vm_mdev`: migration, DRS and HA all leave such a VM alone). A partition is not persistent
//! across a host reboot, so a recorded assignment whose partition is gone is reported as `missing`.

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::state::AppState;
use crate::tasks::enqueue::write_audit;

/// A partition the host reports.
#[derive(Debug, Clone, Deserialize)]
pub struct HostInstance {
    pub uuid: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub type_id: String,
}

/// A recorded VM assignment.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AssignmentRow {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub uuid: String,
    pub mdev_type: String,
    pub parent: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InstanceView {
    pub uuid: String,
    pub parent: String,
    pub type_id: String,
    pub vm_id: Option<Uuid>,
    pub vm_name: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AssignmentView {
    pub vm_id: Uuid,
    pub vm_name: String,
    pub uuid: String,
    pub mdev_type: String,
    pub parent: String,
    /// The recorded partition no longer exists on the host (for example after a reboot).
    pub missing: bool,
}

/// Join what the host reports with what the database recorded.
pub fn merge_view(
    instances: &[HostInstance],
    assignments: &[AssignmentRow],
) -> (Vec<InstanceView>, Vec<AssignmentView>) {
    let inst = instances
        .iter()
        .map(|i| {
            let owner = assignments.iter().find(|a| a.uuid == i.uuid);
            InstanceView {
                uuid: i.uuid.clone(),
                parent: i.parent.clone(),
                type_id: i.type_id.clone(),
                vm_id: owner.map(|a| a.vm_id),
                vm_name: owner.map(|a| a.vm_name.clone()),
            }
        })
        .collect();
    let asg = assignments
        .iter()
        .map(|a| AssignmentView {
            vm_id: a.vm_id,
            vm_name: a.vm_name.clone(),
            uuid: a.uuid.clone(),
            mdev_type: a.mdev_type.clone(),
            parent: a.parent.clone(),
            missing: !instances.iter().any(|i| i.uuid == a.uuid),
        })
        .collect();
    (inst, asg)
}

#[derive(Debug, Serialize)]
pub struct HostMdev {
    /// Partition types each GPU offers, as the agent reports them.
    pub types: Value,
    pub instances: Vec<InstanceView>,
    pub assignments: Vec<AssignmentView>,
}

async fn connect(
    state: &AppState,
    host_id: Uuid,
) -> Result<crate::agent_client::AgentClient, ApiError> {
    let (_, addr) = crate::engine::host_os::resolve_agent_addr(&state.pool, &state.config, host_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    crate::agent_client::connect(&addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
}

async fn assignments_on(state: &AppState, host_id: Uuid) -> Result<Vec<AssignmentRow>, ApiError> {
    Ok(crate::db::query_as(
        "SELECT m.vm_id, v.name AS vm_name, m.uuid, m.mdev_type, m.parent
         FROM vm_mdev m JOIN vms v ON v.id = m.vm_id WHERE m.host_id = ? ORDER BY v.name, m.uuid",
    )
    .bind(host_id)
    .fetch_all(&state.pool)
    .await?)
}

/// What the host reports: `{types, instances}`.
async fn host_report(
    state: &AppState,
    host_id: Uuid,
) -> Result<(Value, Vec<HostInstance>), ApiError> {
    let mut client = connect(state, host_id).await?;
    let report = crate::agent_client::host_libvirt_query(&mut client, "host.mdev", &json!({}))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let instances: Vec<HostInstance> =
        serde_json::from_value(report.get("instances").cloned().unwrap_or(Value::Null))
            .unwrap_or_default();
    Ok((
        report.get("types").cloned().unwrap_or(Value::Null),
        instances,
    ))
}

pub async fn host_mdev(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(host_id): Path<Uuid>,
) -> Result<Json<HostMdev>, ApiError> {
    require_operator(&actor)?;
    let (types, instances) = host_report(&state, host_id).await?;
    let assignments = assignments_on(&state, host_id).await?;
    let (instances, assignments) = merge_view(&instances, &assignments);
    Ok(Json(HostMdev {
        types,
        instances,
        assignments,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    /// PCI address of the GPU, `0000:03:00.0`.
    pub parent: String,
    /// Partition type id from the host's type list, for example `nvidia-222`.
    #[serde(rename = "type")]
    pub type_id: String,
}

pub async fn create_mdev(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(host_id): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let mut client = connect(&state, host_id).await?;
    let res = crate::agent_client::host_libvirt_invoke(
        &mut client,
        "mdev.create",
        &json!({ "parent": body.parent, "type": body.type_id }),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    write_audit(
        &state,
        &actor.username,
        "vgpu.create",
        "host",
        Some(host_id),
        json!({ "parent": body.parent, "type": body.type_id, "uuid": res.get("uuid") }),
    )
    .await?;
    Ok(Json(res))
}

pub async fn delete_mdev(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((host_id, uuid)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let uuid = machina_core::libvirt::mdev::validate_uuid(&uuid)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    if assignments_on(&state, host_id)
        .await?
        .iter()
        .any(|a| a.uuid == uuid)
    {
        return Err(ApiError::conflict(
            "the partition is attached to a VM",
            "detach it from the VM first",
        ));
    }
    let mut client = connect(&state, host_id).await?;
    let res = crate::agent_client::host_libvirt_invoke(
        &mut client,
        "mdev.remove",
        &json!({ "uuid": uuid }),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    write_audit(
        &state,
        &actor.username,
        "vgpu.remove",
        "host",
        Some(host_id),
        json!({ "uuid": uuid }),
    )
    .await?;
    Ok(Json(res))
}

#[derive(Debug, Deserialize)]
pub struct AttachBody {
    pub uuid: String,
}

async fn vm_name_and_host(state: &AppState, vm_id: Uuid) -> Result<(String, Uuid), ApiError> {
    let row: Option<(String, Option<Uuid>)> =
        crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(&state.pool)
            .await?;
    let (name, host) = row.ok_or_else(|| ApiError::not_found("VM not found"))?;
    Ok((
        name,
        host.ok_or_else(|| ApiError::bad_request("the VM has no host"))?,
    ))
}

/// Give a VM a partition that exists on its host. The VM is then pinned to that host.
pub async fn attach_vm_mdev(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(vm_id): Path<Uuid>,
    Json(body): Json<AttachBody>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let uuid = machina_core::libvirt::mdev::validate_uuid(&body.uuid)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (vm_name, host_id) = vm_name_and_host(&state, vm_id).await?;
    let (_, instances) = host_report(&state, host_id).await?;
    let inst = instances
        .iter()
        .find(|i| i.uuid == uuid)
        .ok_or_else(|| ApiError::bad_request("that partition does not exist on the VM's host"))?
        .clone();
    if assignments_on(&state, host_id)
        .await?
        .iter()
        .any(|a| a.uuid == uuid)
    {
        return Err(ApiError::conflict(
            "the partition is already attached to a VM",
            "pick a free one",
        ));
    }
    let mut client = connect(&state, host_id).await?;
    let res = crate::agent_client::vm_libvirt_invoke(
        &mut client,
        &vm_name,
        "mdev.attach",
        &json!({ "uuid": uuid }),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    crate::db::query(
        "INSERT INTO vm_mdev (vm_id, host_id, uuid, mdev_type, parent) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(vm_id)
    .bind(host_id)
    .bind(&uuid)
    .bind(&inst.type_id)
    .bind(&inst.parent)
    .execute(&state.pool)
    .await?;
    write_audit(
        &state,
        &actor.username,
        "vgpu.attach",
        "vm",
        Some(vm_id),
        json!({ "uuid": uuid }),
    )
    .await?;
    Ok(Json(json!({ "attached": res, "pinned_to_host": host_id })))
}

pub async fn detach_vm_mdev(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path((vm_id, uuid)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiError> {
    require_admin(&actor)?;
    let uuid = machina_core::libvirt::mdev::validate_uuid(&uuid)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (vm_name, host_id) = vm_name_and_host(&state, vm_id).await?;
    let recorded: Option<(String,)> =
        crate::db::query_as("SELECT uuid FROM vm_mdev WHERE vm_id = ? AND uuid = ?")
            .bind(vm_id)
            .bind(&uuid)
            .fetch_optional(&state.pool)
            .await?;
    if recorded.is_none() {
        return Err(ApiError::not_found(
            "that partition is not attached to this VM",
        ));
    }
    let mut client = connect(&state, host_id).await?;
    let res = crate::agent_client::vm_libvirt_invoke(
        &mut client,
        &vm_name,
        "mdev.detach",
        &json!({ "uuid": uuid }),
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    crate::db::query("DELETE FROM vm_mdev WHERE vm_id = ? AND uuid = ?")
        .bind(vm_id)
        .bind(&uuid)
        .execute(&state.pool)
        .await?;
    write_audit(
        &state,
        &actor.username,
        "vgpu.detach",
        "vm",
        Some(vm_id),
        json!({ "uuid": uuid }),
    )
    .await?;
    Ok(Json(res))
}

/// True when the VM holds a vGPU partition and so cannot leave its host.
pub async fn is_pinned(pool: &crate::db::DbPool, vm_id: Uuid) -> bool {
    crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM vm_mdev WHERE vm_id = ?")
        .bind(vm_id)
        .fetch_one(pool)
        .await
        .map(|n| n > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(uuid: &str) -> HostInstance {
        HostInstance {
            uuid: uuid.into(),
            parent: "0000:03:00.0".into(),
            type_id: "nvidia-222".into(),
        }
    }

    fn asg(vm: Uuid, name: &str, uuid: &str) -> AssignmentRow {
        AssignmentRow {
            vm_id: vm,
            vm_name: name.into(),
            uuid: uuid.into(),
            mdev_type: "nvidia-222".into(),
            parent: "0000:03:00.0".into(),
        }
    }

    #[test]
    fn view_marks_owners_free_partitions_and_missing_ones() {
        let vm = Uuid::new_v4();
        let (i, a) = merge_view(
            &[inst("u1"), inst("u2")],
            &[asg(vm, "gpu-vm", "u1"), asg(vm, "gpu-vm", "u9")],
        );
        assert_eq!(i[0].vm_name.as_deref(), Some("gpu-vm"));
        assert_eq!(i[0].vm_id, Some(vm));
        assert_eq!(i[1].vm_id, None); // u2 is free
        assert!(!a[0].missing);
        assert!(a[1].missing); // u9 is recorded but no longer on the host
    }

    #[test]
    fn the_agent_report_parses_and_tolerates_missing_fields() {
        let v = json!({ "types": [], "instances": [{ "uuid": "u1", "parent": "0000:03:00.0", "type_id": "nvidia-222" }, { "uuid": "u2" }] });
        let got: Vec<HostInstance> = serde_json::from_value(v["instances"].clone()).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].type_id, "");
    }

    #[tokio::test]
    async fn a_partition_belongs_to_one_vm_and_pins_it_until_removed() {
        let pool = crate::db::testing::pool().await;
        let host = Uuid::new_v4();
        crate::db::query("INSERT INTO hosts (id, hostname, address) VALUES (?, 'h1', '10.0.0.5')")
            .bind(host)
            .execute(&pool)
            .await
            .unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for (id, name) in [(a, "vm-a"), (b, "vm-b")] {
            crate::db::query(
                "INSERT INTO vms (id, name, spec_json, host_id) VALUES (?, ?, '{}', ?)",
            )
            .bind(id)
            .bind(name)
            .bind(host)
            .execute(&pool)
            .await
            .unwrap();
        }
        assert!(!is_pinned(&pool, a).await);
        crate::db::query("INSERT INTO vm_mdev (vm_id, host_id, uuid) VALUES (?, ?, 'u1')")
            .bind(a)
            .bind(host)
            .execute(&pool)
            .await
            .unwrap();
        assert!(is_pinned(&pool, a).await);
        assert!(!is_pinned(&pool, b).await);
        // The same partition cannot be given to a second VM.
        let dup =
            crate::db::query("INSERT INTO vm_mdev (vm_id, host_id, uuid) VALUES (?, ?, 'u1')")
                .bind(b)
                .bind(host)
                .execute(&pool)
                .await;
        assert!(dup.is_err());
        // Deleting the VM releases the pin.
        crate::db::query("DELETE FROM vms WHERE id = ?")
            .bind(a)
            .execute(&pool)
            .await
            .unwrap();
        let n: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM vm_mdev")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
