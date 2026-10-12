// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use super::{access, audit, conflict, invalid};
use crate::{api::ApiError, auth::AuthUser, state::AppState};
use axum::{
    extract::{Path, State},
    Extension, Json,
};
use machina_spec::{ScaleIn, ScalingPolicy, VirtualMachine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Serialize, sqlx::FromRow)]
pub struct LaunchTemplate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplate {
    name: String,
    vm: VirtualMachine,
}
pub async fn create_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
    Json(body): Json<CreateTemplate>,
) -> Result<Json<LaunchTemplate>, ApiError> {
    machina_spec::validate_name(&body.name).map_err(invalid)?;
    // The group supplies its subnet, so a launch template can omit networking.
    // Validate the remaining spec through the existing VM validator using a
    // placeholder attachment; it is never persisted or provisioned.
    let mut validation_vm = body.vm.clone();
    if validation_vm.spec.network.is_empty() {
        validation_vm
            .spec
            .network
            .push(machina_spec::NetworkAttachmentSpec {
                network: "default".into(),
                ip_mode: "dhcp".into(),
                firewall_profile: None,
            });
    }
    validation_vm
        .validate_operator_submission()
        .map_err(invalid)?;
    if body.vm.spec.ha.enabled {
        return Err(invalid(
            "host-local cloud templates do not support cross-host HA",
        ));
    }
    // Usernames/passwords and public console opt-ins are not copied into a
    // reusable template. Use SSH public keys and the authenticated console.
    if body
        .vm
        .spec
        .cloud_init
        .as_ref()
        .is_some_and(|c| c.password.is_some())
        || body.vm.spec.graphics.allow_public_listen
    {
        return Err(invalid(
            "templates require SSH keys and private console listeners",
        ));
    }
    let mut tx = crate::db::begin_write(&state.pool).await?;
    access(&mut tx, &actor, project, true).await?;
    let duplicate: bool = crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cloud_launch_templates WHERE project_id=? AND name=?)",
    )
    .bind(project)
    .bind(&body.name)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Err(conflict(
            "template name already exists; templates are immutable",
        ));
    }
    let row = LaunchTemplate {
        id: Uuid::new_v4(),
        project_id: project,
        name: body.name,
    };
    crate::db::query(
        "INSERT INTO cloud_launch_templates (id,project_id,name,spec_json) VALUES (?,?,?,?)",
    )
    .bind(row.id)
    .bind(project)
    .bind(&row.name)
    .bind(serde_json::to_string(&body.vm).map_err(invalid)?)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &actor, "cloud.template.create", row.id).await?;
    tx.commit().await?;
    Ok(Json(row))
}
pub async fn list_templates(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
) -> Result<Json<Vec<LaunchTemplate>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    access(&mut conn, &actor, project, false).await?;
    // Listing never returns cloud-init data (keys/userdata may be sensitive).
    Ok(Json(crate::db::query_as("SELECT id,project_id,name FROM cloud_launch_templates WHERE project_id=? ORDER BY name LIMIT 500").bind(project).fetch_all(&mut *conn).await?))
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Group {
    pub id: Uuid,
    pub project_id: Uuid,
    pub template_id: Uuid,
    pub subnet_id: Uuid,
    pub name: String,
    pub policy_json: String,
    pub paused: bool,
    pub last_scaled_at: String,
    pub last_error: String,
}
const GROUPS:&str="SELECT id,project_id,template_id,subnet_id,name,policy_json,paused,last_scaled_at,last_error FROM cloud_instance_groups";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGroup {
    name: String,
    template_id: Uuid,
    subnet_id: Uuid,
    policy: ScalingPolicy,
}
pub async fn create_group(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
    Json(body): Json<CreateGroup>,
) -> Result<Json<Value>, ApiError> {
    machina_spec::validate_name(&body.name).map_err(invalid)?;
    body.policy.validate().map_err(invalid)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    access(&mut tx, &actor, project, true).await?;
    let template: Option<Uuid> =
        crate::db::query_scalar("SELECT project_id FROM cloud_launch_templates WHERE id=?")
            .bind(body.template_id)
            .fetch_optional(&mut *tx)
            .await?;
    let subnet:Option<Uuid>=crate::db::query_scalar("SELECT v.project_id FROM cloud_subnets s JOIN cloud_vpcs v ON v.id=s.vpc_id WHERE s.id=? AND s.status='ready'").bind(body.subnet_id).fetch_optional(&mut *tx).await?;
    if template != Some(project) || subnet != Some(project) {
        return Err(invalid(
            "ready subnet and template from the same project required",
        ));
    }
    check_lb(&mut tx, project, body.subnet_id, &body.policy).await?;
    let duplicate: bool = crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cloud_instance_groups WHERE project_id=? AND name=?)",
    )
    .bind(project)
    .bind(&body.name)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        return Err(conflict("instance group name already exists"));
    }
    let id = Uuid::new_v4();
    crate::db::query("INSERT INTO cloud_instance_groups (id,project_id,template_id,subnet_id,name,policy_json) VALUES (?,?,?,?,?,?)").bind(id).bind(project).bind(body.template_id).bind(body.subnet_id).bind(&body.name).bind(serde_json::to_string(&body.policy).map_err(invalid)?).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.group.create", id).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"status":"pending","policy":body.policy}),
    ))
}
pub async fn list_groups(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(project): Path<Uuid>,
) -> Result<Json<Vec<Group>>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    access(&mut conn, &actor, project, false).await?;
    Ok(Json(
        crate::db::query_as(&format!(
            "{GROUPS} WHERE project_id=? ORDER BY name LIMIT 500"
        ))
        .bind(project)
        .fetch_all(&mut *conn)
        .await?,
    ))
}
pub async fn get_group(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let row: Group = crate::db::query_as(&format!("{GROUPS} WHERE id=?"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    access(&mut conn, &actor, row.project_id, false).await?;
    type Member = (
        i64,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let members: Vec<Member> = crate::db::query_as(
        "SELECT m.slot, m.vm_id, v.name, v.observed_state, v.desired_state, m.draining_since
         FROM cloud_group_members m LEFT JOIN vms v ON v.id = m.vm_id
         WHERE m.group_id = ? ORDER BY m.slot",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let policy: Option<ScalingPolicy> = serde_json::from_str(&row.policy_json).ok();
    let scale_in = match policy.as_ref().map(|p| p.scale_in) {
        Some(ScaleIn::Sleep) => "sleep",
        _ => "stop-and-retain",
    };
    Ok(Json(json!({
        "group": row,
        "members": members
            .into_iter()
            .map(|(slot, vm_id, name, state, desired, draining)| json!({
                "slot": slot,
                "vm_id": vm_id,
                "name": name,
                "observed_state": state,
                "desired_state": desired,
                "draining_since": draining,
            }))
            .collect::<Vec<_>>(),
        "scale_in": scale_in,
        "network_backend": "host-local-isolated",
    })))
}

/// A group's load balancer must be on the group's host (its rules DNAT to the
/// members from there) and belong to the group's project.
async fn check_lb(
    conn: &mut crate::db::DbConn,
    project: Uuid,
    subnet: Uuid,
    policy: &ScalingPolicy,
) -> Result<(), ApiError> {
    let Some(lb) = policy.load_balancer else {
        return Ok(());
    };
    let ok: bool = crate::db::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM load_balancers l, cloud_subnets s
           JOIN cloud_vpcs v ON v.id = s.vpc_id
         WHERE l.id = ? AND s.id = ? AND l.host_id = v.host_id
           AND (l.project_id IS NULL OR l.project_id = ?))",
    )
    .bind(lb.id)
    .bind(subnet)
    .bind(project)
    .fetch_one(&mut *conn)
    .await?;
    if !ok {
        return Err(invalid(
            "the load balancer must exist on the subnet's host and belong to the project",
        ));
    }
    Ok(())
}

/// Demand history and the seasonal forecast for the next day: summed member
/// CPU per hour, and the instances the target would need.
pub async fn group_forecast(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    use crate::engine::ai::forecast;
    let mut conn = state.pool.acquire().await?;
    let row: Group = crate::db::query_as(&format!("{GROUPS} WHERE id=?"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    access(&mut conn, &actor, row.project_id, false).await?;
    drop(conn);
    let policy: ScalingPolicy = serde_json::from_str(&row.policy_json).map_err(invalid)?;
    let hourly = forecast::hourly(
        &state.pool,
        &forecast::group_subject(id),
        "cpu_sum",
        35,
        true,
    )
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let now = chrono::Utc::now().timestamp();
    let history: Vec<Value> = hourly
        .range(now - 48 * 3600..)
        .map(|(h, v)| json!({ "hour": h, "demand": v }))
        .collect();
    let ahead: Vec<Value> = (0..24)
        .filter_map(|k| {
            let h = now / 3600 * 3600 + k * 3600;
            forecast::seasonal_at(&hourly, h).map(|s| {
                json!({
                    "hour": h,
                    "demand": s.value,
                    "basis": s.basis,
                    "needed": policy.needed_for(s.value),
                })
            })
        })
        .collect();
    let peak = forecast::seasonal_peak(&hourly, now, 1).map(|(h, s)| {
        json!({ "hour": h, "demand": s.value, "basis": s.basis, "needed": policy.needed_for(s.value) })
    });
    Ok(Json(json!({
        "group_id": id,
        "predictive": policy.predictive,
        "target_cpu": policy.target_cpu,
        "hours_of_history": hourly.len(),
        "history": history,
        "forecast": ahead,
        "next_hour_peak": peak,
    })))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroup {
    policy: ScalingPolicy,
    paused: bool,
}
pub async fn update_group(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGroup>,
) -> Result<Json<Value>, ApiError> {
    body.policy.validate().map_err(invalid)?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let (project, subnet): (Uuid, Uuid) =
        crate::db::query_as("SELECT project_id, subnet_id FROM cloud_instance_groups WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    access(&mut tx, &actor, project, true).await?;
    check_lb(&mut tx, project, subnet, &body.policy).await?;
    crate::db::query("UPDATE cloud_instance_groups SET policy_json=?,paused=?,last_scaled_at=CURRENT_TIMESTAMP,last_error='' WHERE id=?").bind(serde_json::to_string(&body.policy).map_err(invalid)?).bind(body.paused).bind(id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.group.update", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"updated":true})))
}

/// Why a group cannot be deleted yet (None = it can).
pub(crate) fn group_delete_blocker(active_members: i64) -> Option<String> {
    (active_members > 0).then(|| {
        format!("{active_members} member instance(s) are still running; set the group's min and desired to 0, wait for them to stop, then delete it")
    })
}

/// Delete an instance group. Its stopped member instances and their disks are kept (they just stop being managed);
/// alarms that scaled it lose their action.
pub async fn delete_group(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let project: Uuid = crate::db::query_scalar("SELECT project_id FROM cloud_instance_groups WHERE id=?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("instance group not found"))?;
    access(&mut tx, &actor, project, true).await?;
    // Stop the reconciler touching it while we check.
    crate::db::query("UPDATE cloud_instance_groups SET paused = TRUE WHERE id=?").bind(id).execute(&mut *tx).await?;
    let active: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM cloud_group_members m JOIN vms v ON v.id = m.vm_id \
         WHERE m.group_id = ? AND v.observed_state NOT IN ('shutoff', 'stopped', 'missing')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(why) = group_delete_blocker(active) {
        // Roll back the pause: a refused delete must not leave the group frozen.
        tx.rollback().await?;
        return Err(conflict(why));
    }
    crate::db::query("DELETE FROM cloud_group_members WHERE group_id=?").bind(id).execute(&mut *tx).await?;
    crate::db::query("UPDATE cloud_alarms SET action='none', group_id=NULL, step=0 WHERE group_id=?").bind(id).execute(&mut *tx).await?;
    crate::db::query("DELETE FROM cloud_instance_groups WHERE id=?").bind(id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.group.delete", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}

/// Delete a launch template that no group uses.
pub async fn delete_template(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let project: Uuid = crate::db::query_scalar("SELECT project_id FROM cloud_launch_templates WHERE id=?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("launch template not found"))?;
    access(&mut tx, &actor, project, true).await?;
    let used: i64 = crate::db::query_scalar("SELECT COUNT(*) FROM cloud_instance_groups WHERE template_id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if used > 0 {
        return Err(conflict(format!("{used} instance group(s) still use this launch template")));
    }
    crate::db::query("DELETE FROM cloud_launch_templates WHERE id=?").bind(id).execute(&mut *tx).await?;
    audit(&mut tx, &actor, "cloud.template.delete", id).await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted":true})))
}
