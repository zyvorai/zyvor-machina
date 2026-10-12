// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

pub(crate) mod elastic;
pub(crate) mod network;
pub(crate) use network::reserve_address;

use crate::{api::ApiError, auth::AuthUser, state::AppState};
use axum::routing::{get, post};
use axum::Router;
use crate::db::DbConn;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/cloud/projects/{id}/vpcs",
            get(network::list_vpcs).post(network::create_vpc),
        )
        .route(
            "/api/v1/cloud/vpcs/{id}",
            get(network::get_vpc).delete(network::delete_vpc),
        )
        .route(
            "/api/v1/cloud/vpcs/{id}/subnets",
            get(network::list_subnets).post(network::create_subnet),
        )
        .route(
            "/api/v1/cloud/subnets/{id}/nat",
            axum::routing::put(network::set_subnet_nat),
        )
        .route(
            "/api/v1/cloud/subnets/{id}",
            axum::routing::delete(network::delete_subnet),
        )
        .route(
            "/api/v1/cloud/subnets/{id}/retry",
            post(network::retry_subnet),
        )
        .route(
            "/api/v1/cloud/subnets/{id}/addresses",
            get(network::list_addresses).post(network::allocate_address),
        )
        .route(
            "/api/v1/cloud/subnets/{id}/addresses/{allocation}",
            axum::routing::delete(network::release_address),
        )
        .route(
            "/api/v1/cloud/vpcs/{id}/routes",
            get(network::list_routes).post(network::create_route),
        )
        .route(
            "/api/v1/cloud/vpcs/{id}/routes/{route}",
            axum::routing::delete(network::delete_route),
        )
        .route("/api/v1/cloud/vpcs/{id}/plan", get(network::plan))
        .route(
            "/api/v1/cloud/vpcs/{id}/peerings",
            get(network::list_peerings).post(network::create_peering),
        )
        .route(
            "/api/v1/cloud/peerings/{id}/accept",
            post(network::accept_peering),
        )
        .route(
            "/api/v1/cloud/projects/{id}/launch-templates",
            get(elastic::list_templates).post(elastic::create_template),
        )
        .route(
            "/api/v1/cloud/launch-templates/{id}",
            axum::routing::delete(elastic::delete_template),
        )
        .route(
            "/api/v1/cloud/projects/{id}/instance-groups",
            get(elastic::list_groups).post(elastic::create_group),
        )
        .route(
            "/api/v1/cloud/instance-groups/{id}",
            get(elastic::get_group)
                .patch(elastic::update_group)
                .delete(elastic::delete_group),
        )
        .route(
            "/api/v1/cloud/instance-groups/{id}/forecast",
            get(elastic::group_forecast),
        )
}

/// Cloud APIs always enforce membership, even when legacy project RBAC is off.
/// Unknown identities (including unscoped API keys) fail closed for non-admins.
pub(crate) async fn access(
    conn: &mut DbConn,
    actor: &AuthUser,
    project: Uuid,
    write: bool,
) -> Result<(), ApiError> {
    let enabled: Option<bool> = crate::db::query_scalar("SELECT enabled FROM projects WHERE id = ?")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?;
    if enabled != Some(true) {
        return Err(ApiError::not_found("enabled project not found"));
    }
    if actor.role == "admin" {
        return Ok(());
    }
    if write && actor.role != "operator" {
        return Err(ApiError::forbidden("operator role required"));
    }
    // A project-scoped API key may use exactly the projects it was issued for (its own role still caps write access).
    let scope = crate::api::apikeys::scope_of_conn(conn, &actor.username).await;
    if !scope.is_empty() {
        let name: Option<String> = crate::db::query_scalar("SELECT name FROM projects WHERE id = ?").bind(project).fetch_optional(&mut *conn).await?;
        return match name {
            Some(n) if scope.contains(&n) => Ok(()),
            _ => Err(ApiError::forbidden("this API key is not scoped to that project").with_code("key_scope_forbidden")),
        };
    }
    let roles: Vec<String> = crate::db::query_scalar("SELECT a.role FROM project_role_assignments a JOIN users u ON u.id = a.user_id WHERE u.username = ? AND a.project_id = ?")
        .bind(&actor.username).bind(project).fetch_all(&mut *conn).await?;
    let allowed = roles
        .iter()
        .any(|r| r == "admin" || r == "operator" || (!write && r == "viewer"));
    if !allowed {
        return Err(
            ApiError::forbidden("project membership required").with_code("project_forbidden")
        );
    }
    Ok(())
}

pub(crate) async fn audit(
    conn: &mut DbConn,
    actor: &AuthUser,
    action: &str,
    id: Uuid,
) -> Result<(), ApiError> {
    crate::db::query("INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail) VALUES (?, ?, ?, 'cloud', ?, '{}')")
        .bind(Uuid::new_v4()).bind(&actor.username).bind(action).bind(id).execute(conn).await?;
    Ok(())
}
pub(crate) fn invalid(e: impl ToString) -> ApiError {
    ApiError::bad_request(e.to_string())
}
pub(crate) fn conflict(e: impl ToString) -> ApiError {
    ApiError::conflict(
        e.to_string(),
        "Choose a different CIDR/name or remove the conflicting resource.",
    )
}

#[cfg(test)]
mod tests;

/// Apply VPC ownership checks to legacy VM/NIC creation too. A different API
/// entry point must not be a way around project isolation or host-local scope.
pub(crate) async fn authorize_attachment(
    state: &AppState,
    actor: &AuthUser,
    network: &str,
    project: &str,
    host: Option<Uuid>,
) -> Result<bool, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let owner: Option<(Uuid, String, Uuid, String)> = crate::db::query_as("SELECT v.project_id,p.name,v.host_id,s.status FROM cloud_subnets s JOIN networks n ON n.id=s.network_id JOIN cloud_vpcs v ON v.id=s.vpc_id JOIN projects p ON p.id=v.project_id WHERE n.name=?")
        .bind(network).fetch_optional(&mut *conn).await?;
    if let Some((project_id, name, expected_host, status)) = owner {
        access(&mut conn, actor, project_id, true).await?;
        if project != name || host != Some(expected_host) || status != "ready" {
            return Err(ApiError::forbidden(
                "cloud subnet requires its owning project, host, and ready status",
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(crate) async fn protect_network(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    let owned: bool =
        crate::db::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_subnets WHERE network_id=?)")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    if owned {
        return Err(ApiError::conflict(
            "network is owned by a cloud subnet",
            "Manage this network through its VPC; legacy network mutations are disabled.",
        ));
    }
    Ok(())
}

/// The first VPC backend has no cross-host datapath. Enforce this at the worker
/// boundary as well as public APIs so DRS, HA and old queued tasks cannot move
/// an endpoint to an unprovisioned network.
pub(crate) async fn check_vm_host(
    pool: &crate::db::DbPool,
    vm: Uuid,
    host: Uuid,
) -> Result<(), ApiError> {
    let hosts: Vec<Uuid> = crate::db::query_scalar("SELECT DISTINCT c.host_id FROM vms v, json_each(json_extract(v.spec_json,'$.spec.network')) nic JOIN networks n ON n.name=json_extract(nic.value,'$.network') JOIN cloud_subnets s ON s.network_id=n.id JOIN cloud_vpcs c ON c.id=s.vpc_id WHERE v.id=?")
        .bind(vm).fetch_all(pool).await?;
    if hosts.iter().any(|h| *h != host) {
        return Err(ApiError::conflict(
            "VM uses a host-local cloud subnet",
            "Keep this VM on its VPC host; cross-host VPC forwarding is not available.",
        ));
    }
    Ok(())
}
