// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! OpenStack-compatible REST façade (Keystone / Nova / Glance / Cinder / Neutron subset).
//! Thin JSON wrappers over Fleet Cloud handlers so `openstack` CLI and Terraform's
//! `openstack` provider can talk to Machina without Nova/Neutron wire reimplementation.

mod auth;
mod catalog;
mod cinder;
mod glance;
mod ids;
mod keystone;
mod neutron;
mod nova;
mod octavia;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub use auth::os_token_middleware;

#[derive(Debug)]
pub struct OsError {
    pub status: StatusCode,
    pub title: &'static str,
    pub message: String,
}

impl OsError {
    pub fn new(status: StatusCode, title: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            title,
            message: message.into(),
        }
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorized", msg)
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "Forbidden", msg)
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "Bad Request", msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "Not Found", msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "Conflict", msg)
    }
    pub fn not_implemented(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "Not Implemented", msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            msg,
        )
    }
}

impl From<crate::api::ApiError> for OsError {
    fn from(e: crate::api::ApiError) -> Self {
        let title = match e.status.as_u16() {
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            409 => "Conflict",
            _ => "Internal Server Error",
        };
        Self::new(e.status, title, e.message)
    }
}

impl From<sqlx::Error> for OsError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::not_found("resource not found"),
            other => {
                tracing::warn!("openstack api database error: {other}");
                Self::internal("internal error")
            }
        }
    }
}

impl IntoResponse for OsError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "message": self.message,
                "code": self.status.as_u16(),
                "title": self.title,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

/// Public Keystone auth + version discovery (rate-limited with login / EC2).
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/", get(keystone::root_versions))
        .route("/v3", get(keystone::identity_versions))
        .route(
            "/v3/auth/tokens",
            post(auth::issue_token).get(auth::validate_token),
        )
}

fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/v3/auth/catalog", get(catalog::auth_catalog))
        .route(
            "/v3/projects",
            get(keystone::list_projects).post(keystone::create_project),
        )
        .route("/v3/users", get(keystone::list_users))
        .route(
            "/v2.1/flavors",
            get(nova::list_flavors).post(nova::create_flavor),
        )
        .route("/v2.1/flavors/detail", get(nova::list_flavors_detail))
        .route(
            "/v2.1/flavors/{id}",
            get(nova::get_flavor).delete(nova::delete_flavor),
        )
        .route(
            "/v2.1/servers",
            get(nova::list_servers).post(nova::create_server),
        )
        .route("/v2.1/servers/detail", get(nova::list_servers_detail))
        .route(
            "/v2.1/servers/{id}",
            get(nova::get_server).delete(nova::delete_server),
        )
        .route("/v2.1/servers/{id}/action", post(nova::server_action))
        .route(
            "/v2.1/servers/{id}/remote-consoles",
            post(nova::get_remote_console),
        )
        .route(
            "/v2.1/servers/{id}/os-interface",
            get(nova::list_os_interfaces).post(nova::attach_os_interface),
        )
        .route(
            "/v2.1/servers/{id}/os-interface/{port_id}",
            delete(nova::detach_os_interface),
        )
        .route(
            "/v2.1/os-keypairs",
            get(nova::list_keypairs).post(nova::create_keypair),
        )
        .route(
            "/v2.1/os-keypairs/{name}",
            get(nova::get_keypair).delete(nova::delete_keypair),
        )
        .route(
            "/v2.1/os-server-groups",
            get(nova::list_server_groups).post(nova::create_server_group),
        )
        .route(
            "/v2.1/os-server-groups/{name}",
            delete(nova::delete_server_group),
        )
        .route(
            "/v2/images",
            get(glance::list_images).post(glance::create_image),
        )
        .route(
            "/v2/images/{id}",
            get(glance::get_image).delete(glance::delete_image),
        )
        .route(
            "/v3/volumes",
            get(cinder::list_volumes).post(cinder::create_volume),
        )
        .route(
            "/v3/volumes/{id}",
            get(cinder::get_volume).delete(cinder::delete_volume),
        )
        .route("/v3/volumes/{id}/action", post(cinder::volume_action))
        .route(
            "/v3/{project_id}/volumes",
            get(cinder::list_volumes).post(cinder::create_volume),
        )
        .route(
            "/v3/{project_id}/volumes/{id}",
            get(cinder::get_volume_scoped).delete(cinder::delete_volume_scoped),
        )
        .route(
            "/v3/{project_id}/volumes/{id}/action",
            post(cinder::volume_action_scoped),
        )
        .route(
            "/v3/snapshots",
            get(cinder::list_snapshots).post(cinder::create_snapshot),
        )
        .route(
            "/v3/snapshots/{id}",
            get(cinder::get_snapshot).delete(cinder::delete_snapshot),
        )
        .route(
            "/v3/{project_id}/snapshots",
            get(cinder::list_snapshots).post(cinder::create_snapshot),
        )
        .route(
            "/v3/{project_id}/snapshots/{id}",
            get(cinder::get_snapshot_scoped).delete(cinder::delete_snapshot_scoped),
        )
        .route(
            "/v2.0/networks",
            get(neutron::list_networks).post(neutron::create_network),
        )
        .route(
            "/v2.0/networks/{id}",
            get(neutron::get_network).delete(neutron::delete_network),
        )
        .route(
            "/v2.0/subnets",
            get(neutron::list_subnets).post(neutron::create_subnet),
        )
        .route(
            "/v2.0/subnets/{id}",
            get(neutron::get_subnet).delete(neutron::delete_subnet),
        )
        .route(
            "/v2.0/ports",
            get(neutron::list_ports).post(neutron::create_port),
        )
        .route(
            "/v2.0/ports/{id}",
            get(neutron::get_port)
                .put(neutron::update_port)
                .delete(neutron::delete_port),
        )
        .route(
            "/v2.0/security-groups",
            get(neutron::list_security_groups).post(neutron::create_security_group),
        )
        .route(
            "/v2.0/security-groups/{id}",
            get(neutron::get_security_group).delete(neutron::delete_security_group),
        )
        .route(
            "/v2.0/security-group-rules",
            get(neutron::list_security_group_rules).post(neutron::create_security_group_rule),
        )
        .route(
            "/v2.0/security-group-rules/{id}",
            delete(neutron::delete_security_group_rule),
        )
        .route(
            "/v2.0/floatingips",
            get(neutron::list_floating_ips).post(neutron::create_floating_ip),
        )
        .route(
            "/v2.0/floatingips/{id}",
            get(neutron::get_floating_ip)
                .put(neutron::update_floating_ip)
                .delete(neutron::delete_floating_ip),
        )
        .route(
            "/v2.0/qos/policies",
            get(neutron::list_qos_policies).post(neutron::create_qos_policy),
        )
        .route(
            "/v2.0/qos/policies/{id}",
            delete(neutron::delete_qos_policy),
        )
        .route(
            "/v2.0/qos/policies/{id}/bandwidth_limit_rules",
            post(neutron::create_qos_policy_bandwidth_rule),
        )
        .route(
            "/v2.0/lbaas/loadbalancers",
            get(octavia::list_loadbalancers).post(octavia::create_loadbalancer),
        )
        .route(
            "/v2.0/lbaas/loadbalancers/{id}",
            get(octavia::get_loadbalancer).delete(octavia::delete_loadbalancer),
        )
        .route(
            "/v2.0/lbaas/pools/{id}/members",
            get(octavia::list_members).post(octavia::create_member),
        )
        .route(
            "/v2.0/lbaas/pools/{pool_id}/members/{member_id}",
            delete(octavia::delete_member),
        )
}

/// Token-gated OpenStack service routes (mount beside the rate-limited public auth routes).
/// Returns `Router<AppState>` so the parent can `.merge` before a single `.with_state`.
pub fn protected_router(state: AppState) -> Router<AppState> {
    protected_routes().route_layer(axum::middleware::from_fn_with_state(
        state,
        os_token_middleware,
    ))
}

/// Full OpenStack surface for unit tests (public auth + protected services).
pub fn router(state: AppState) -> Router {
    public_router()
        .merge(protected_router(state.clone()))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::test_state;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}))
    }

    #[tokio::test]
    async fn token_issue_requires_password_and_project() {
        let (state, _rx) = test_state().await;
        let hash = bcrypt::hash("admin", 4).unwrap();
        crate::db::query(
            "INSERT INTO users (id, username, password_hash, role) VALUES (?, 'admin', ?, 'admin')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&hash)
        .execute(&state.pool)
        .await
        .ok();
        let pid = uuid::Uuid::new_v4();
        crate::db::query("INSERT INTO projects (id, name, description) VALUES (?, 'default', '')")
            .bind(pid)
            .execute(&state.pool)
            .await
            .ok();

        let app = router(state);
        let body = json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {
                        "user": {
                            "name": "admin",
                            "domain": {"id": "default"},
                            "password": "admin"
                        }
                    }
                },
                "scope": {
                    "project": {"name": "default", "domain": {"id": "default"}}
                }
            }
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v3/auth/tokens")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert!(resp.headers().get("x-subject-token").is_some());
        let v = json_body(resp).await;
        assert!(v["token"]["catalog"].is_array());
        assert_eq!(v["token"]["project"]["name"], "default");
    }

    #[tokio::test]
    async fn flavors_list_with_token() {
        let (state, _rx) = test_state().await;
        let hash = bcrypt::hash("admin", 4).unwrap();
        crate::db::query(
            "INSERT INTO users (id, username, password_hash, role) VALUES (?, 'admin', ?, 'admin')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&hash)
        .execute(&state.pool)
        .await
        .ok();
        let pid = uuid::Uuid::new_v4();
        crate::db::query("INSERT INTO projects (id, name, description) VALUES (?, 'default', '')")
            .bind(pid)
            .execute(&state.pool)
            .await
            .ok();
        crate::db::query(
            "INSERT INTO flavors (id, name, vcpus, memory_mib, disk_gib, description, is_public) \
             VALUES (?, 'm1.tiny', 1, 512, 1, '', TRUE)",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&state.pool)
        .await
        .unwrap();

        let token = auth::mint_token_for_test(&state, "admin", "admin", pid, "default")
            .await
            .unwrap();
        let app = router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v2.1/flavors/detail")
                    .header("x-auth-token", &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = json_body(resp).await;
        assert!(v["flavors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "m1.tiny"));
    }
}
