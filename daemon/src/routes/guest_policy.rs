// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Per-container eBPF policy inside a guest, relayed to the in-guest GuestKit
//! agent (`guestkitctl call guestkit.netpolicy.* / guestkit.lsm.*` over QGA
//! guest-exec). The guest agent owns validation, its local `ebpf` capability
//! gate, audit-by-default and the enforcement lease.

use axum::extract::{Extension, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use machina_core::libvirt::domain::lookup_domain;
use machina_core::libvirt::guest_agent_actions::guestkit_call;
use machina_core::LibvirtManager;
use serde::Deserialize;
use serde_json::{json, Value};

use super::bpf::require_admin;
use crate::auth::RequestActor;
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;

#[derive(Deserialize, Default)]
struct TargetQuery {
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    cgroup: Option<String>,
}

impl TargetQuery {
    fn params(&self) -> Value {
        let mut p = json!({});
        if let Some(c) = self.container.as_deref().filter(|s| !s.is_empty()) {
            p["container"] = json!(c);
        }
        if let Some(c) = self.cgroup.as_deref().filter(|s| !s.is_empty()) {
            p["cgroup"] = json!(c);
        }
        p
    }
}

async fn relay(
    manager: LibvirtManager,
    actor: &RequestActor,
    vm: String,
    method: &'static str,
    params: Value,
) -> Result<Json<Value>, AppError> {
    spawn_libvirt_actor(manager, Some(actor), ConnQuery::default(), move |conn| {
        lookup_domain(conn, &vm)?;
        guestkit_call(&vm, method, &params)
    })
    .await
    .map(Json)
}

async fn netpolicy_status(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(q): Query<TargetQuery>,
) -> Result<Json<Value>, AppError> {
    relay(
        manager,
        &actor,
        name,
        "guestkit.netpolicy.status",
        q.params(),
    )
    .await
}

async fn netpolicy_apply(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Guest network policy")?;
    relay(manager, &actor, name, "guestkit.netpolicy.apply", body).await
}

async fn lsm_status(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(q): Query<TargetQuery>,
) -> Result<Json<Value>, AppError> {
    relay(manager, &actor, name, "guestkit.lsm.status", q.params()).await
}

async fn lsm_apply(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    require_admin(&actor, "Guest LSM policy")?;
    relay(manager, &actor, name, "guestkit.lsm.apply", body).await
}

pub fn guest_policy_routes() -> Router<LibvirtManager> {
    Router::new()
        .route(
            "/vms/{name}/guest-policy",
            get(netpolicy_status).put(netpolicy_apply),
        )
        .route("/vms/{name}/guest-lsm", get(lsm_status).put(lsm_apply))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_query_params() {
        assert_eq!(TargetQuery::default().params(), json!({}));
        let q = TargetQuery {
            container: Some("web".into()),
            cgroup: Some(String::new()),
        };
        assert_eq!(q.params(), json!({ "container": "web" }));
    }
}
