// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Project-scoped access. Machina's global roles (admin/operator/viewer) say what a person may do; project roles say
//! *where*. With `MACHINA_PROJECT_RBAC=enforce`, a non-admin local user can only see and act on machines in projects
//! they have been added to (`project_role_assignments`), and only change them with an `operator`/`admin` project role.
//! `audit` mode logs what enforce would have denied without blocking anyone, so an existing site can look before it leaps.
//!
//! Scope of this layer (v1): every `/api/v1/vms/{id}[/…]` route and the `/api/v1/vms` list. Identities that are not
//! rows in `users` (API keys, service accounts) keep their global role: automation is scoped by the key's role,
//! not by project membership. Platform admins are never restricted.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::AuthUser;
use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Off,
    Audit,
    Enforce,
}

pub fn mode() -> Mode {
    match std::env::var("MACHINA_PROJECT_RBAC").as_deref() {
        Ok("enforce") => Mode::Enforce,
        Ok("audit") => Mode::Audit,
        _ => Mode::Off,
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    Allow,
    Deny(&'static str),
}

fn is_read(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The access decision for one request on a machine in some project. Pure, so it is unit-tested.
pub fn decide(method: &Method, global_role: &str, project_role: Option<&str>) -> Decision {
    if global_role == "admin" {
        return Decision::Allow;
    }
    match project_role {
        None => Decision::Deny("you are not a member of this machine's project"),
        Some(_) if is_read(method) => Decision::Allow,
        Some("operator") | Some("admin") => Decision::Allow,
        Some(_) => Decision::Deny("your role in this project is read-only"),
    }
}

/// What a project-scoped API key may call. `vm_project` is the project of the machine in the path, when there is one.
/// Cloud routes check the project themselves (`cloud::access`); instance routes are checked here; everything else is global
/// and is closed to scoped keys. Creating machines through the plain instance API is closed too, because the project is in the
/// request body: scoped keys launch through the cloud API (launch templates and groups) instead.
pub fn scoped_decision(scope: &[String], method: &Method, path: &str, vm_project: Option<&str>) -> Decision {
    if path.starts_with("/api/v1/cloud/") {
        return Decision::Allow;
    }
    if path == "/api/v1/vms" {
        return if is_read(method) {
            Decision::Allow // the list is filtered to the key's projects by the caller
        } else {
            Decision::Deny("scoped API keys launch machines through the cloud API")
        };
    }
    if vm_id_in_path(path).is_some() {
        return match vm_project {
            Some(p) if scope.iter().any(|s| s == p) => Decision::Allow,
            _ => Decision::Deny("that machine is not in this API key's projects"),
        };
    }
    Decision::Deny("this API key is limited to project APIs")
}

/// `/api/v1/vms/<uuid>[/…]` → the machine id.
pub fn vm_id_in_path(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/api/v1/vms/")?;
    Uuid::parse_str(rest.split('/').next()?).ok()
}

/// The strongest project role among a user's assignments ("admin" > "operator" > "viewer").
pub fn strongest<'a>(roles: &'a [String]) -> Option<&'a str> {
    ["admin", "operator", "viewer"]
        .into_iter()
        .find(|r| roles.iter().any(|x| x == r))
}

async fn is_managed_user(state: &AppState, username: &str) -> bool {
    crate::db::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(&state.pool)
        .await
        .map(|n| n > 0)
        .unwrap_or(false)
}

async fn project_roles(state: &AppState, username: &str, project: &str) -> Vec<String> {
    crate::db::query_scalar(
        "SELECT a.role FROM project_role_assignments a
         JOIN users u ON u.id = a.user_id JOIN projects p ON p.id = a.project_id
         WHERE u.username = ? AND p.name = ? AND p.enabled = TRUE",
    )
    .bind(username)
    .bind(project)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
}

/// Names of the projects this user belongs to.
async fn member_projects(state: &AppState, username: &str) -> Vec<String> {
    crate::db::query_scalar(
        "SELECT DISTINCT p.name FROM project_role_assignments a
         JOIN users u ON u.id = a.user_id JOIN projects p ON p.id = a.project_id
         WHERE u.username = ? AND p.enabled = TRUE",
    )
    .bind(username)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
}

pub async fn middleware(
    State(state): State<AppState>,
    actor: Option<Extension<AuthUser>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let m = mode();
    let Some(Extension(actor)) = actor else {
        return next.run(req).await;
    };
    // Project-scoped API keys are limited regardless of MACHINA_PROJECT_RBAC: the scope was asked for when the key was issued.
    let scope = crate::api::apikeys::scope_of(&state.pool, &actor.username).await;
    if !scope.is_empty() {
        let path = req.uri().path().to_string();
        let method = req.method().clone();
        let vm_project: Option<String> = match vm_id_in_path(&path) {
            Some(vm) => crate::db::query_scalar("SELECT COALESCE(project, 'default') FROM vms WHERE id = ?")
                .bind(vm)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten(),
            None => None,
        };
        return match scoped_decision(&scope, &method, &path, vm_project.as_deref()) {
            Decision::Deny(why) => ApiError::forbidden(why)
                .with_code("key_scope_forbidden")
                .with_remediation("Use a key without a project scope, or call an API of one of this key's projects.")
                .into_response(),
            Decision::Allow if path == "/api/v1/vms" => filter_machine_list(next.run(req).await, &scope).await,
            Decision::Allow => next.run(req).await,
        };
    }
    if m == Mode::Off || actor.role == "admin" {
        return next.run(req).await;
    }
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let is_list = path == "/api/v1/vms" && method == Method::GET;
    let vm_id = vm_id_in_path(&path);
    if !is_list && vm_id.is_none() {
        return next.run(req).await;
    }
    if !is_managed_user(&state, &actor.username).await {
        return next.run(req).await; // API keys and other non-user identities: global role only
    }

    if let Some(vm) = vm_id {
        let project: Option<String> =
            crate::db::query_scalar("SELECT COALESCE(project, 'default') FROM vms WHERE id = ?")
                .bind(vm)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten();
        let Some(project) = project else {
            return next.run(req).await; // unknown id: let the handler answer 404
        };
        let roles = project_roles(&state, &actor.username, &project).await;
        match decide(&method, &actor.role, strongest(&roles)) {
            Decision::Allow => return next.run(req).await,
            Decision::Deny(why) => {
                tracing::warn!(user = %actor.username, %project, %path, "project RBAC: {why} ({m:?})");
                if m == Mode::Enforce {
                    return ApiError::forbidden(why)
                        .with_code("project_forbidden")
                        .with_remediation("Ask a project admin to add you to this project.")
                        .into_response();
                }
                return next.run(req).await;
            }
        }
    }

    // The machine list: show only machines in the user's projects.
    let resp = next.run(req).await;
    if m != Mode::Enforce || resp.status() != StatusCode::OK {
        return resp;
    }
    let allowed = member_projects(&state, &actor.username).await;
    filter_machine_list(resp, &allowed).await
}

/// Keep only the machines whose `project` is in `allowed` (the machine list is a JSON array).
async fn filter_machine_list(resp: Response, allowed: &[String]) -> Response {
    if resp.status() != StatusCode::OK {
        return resp;
    }
    let (parts, body) = resp.into_parts();
    let bytes = match axum::body::to_bytes(body, 64 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not read the machine list",
            )
                .into_response()
        }
    };
    let filtered = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Array(items)) => {
            let kept: Vec<_> = items
                .into_iter()
                .filter(|v| {
                    let p = v
                        .get("project")
                        .and_then(|p| p.as_str())
                        .unwrap_or("default");
                    allowed.iter().any(|a| a == p)
                })
                .collect();
            serde_json::to_vec(&kept).unwrap_or_else(|_| bytes.to_vec())
        }
        _ => bytes.to_vec(),
    };
    let mut parts = parts;
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(filtered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admins_are_never_restricted() {
        assert_eq!(decide(&Method::DELETE, "admin", None), Decision::Allow);
    }

    #[test]
    fn a_non_member_is_denied_even_to_read() {
        assert!(matches!(
            decide(&Method::GET, "operator", None),
            Decision::Deny(_)
        ));
        assert!(matches!(
            decide(&Method::POST, "operator", None),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn viewers_read_but_do_not_change_and_operators_do() {
        assert_eq!(
            decide(&Method::GET, "viewer", Some("viewer")),
            Decision::Allow
        );
        assert!(matches!(
            decide(&Method::POST, "viewer", Some("viewer")),
            Decision::Deny(_)
        ));
        assert_eq!(
            decide(&Method::POST, "viewer", Some("operator")),
            Decision::Allow
        );
        assert_eq!(
            decide(&Method::PATCH, "operator", Some("admin")),
            Decision::Allow
        );
    }

    #[test]
    fn project_role_is_what_matters_not_the_global_role() {
        // A global operator who is only a project viewer cannot change the machine.
        assert!(matches!(
            decide(&Method::POST, "operator", Some("viewer")),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn finds_the_machine_id_in_a_path() {
        let id = "3b2803c9-68e9-4235-b0f8-ef46a42c7a80";
        assert_eq!(
            vm_id_in_path(&format!("/api/v1/vms/{id}")),
            Uuid::parse_str(id).ok()
        );
        assert_eq!(
            vm_id_in_path(&format!("/api/v1/vms/{id}/start")),
            Uuid::parse_str(id).ok()
        );
        assert_eq!(vm_id_in_path("/api/v1/vms/prune-missing"), None);
        assert_eq!(vm_id_in_path("/api/v1/vms"), None);
        assert_eq!(vm_id_in_path("/api/v1/hosts/x"), None);
    }

    #[test]
    fn the_strongest_project_role_wins() {
        let r = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(strongest(&r(&["viewer", "operator"])), Some("operator"));
        assert_eq!(strongest(&r(&["viewer"])), Some("viewer"));
        assert_eq!(strongest(&r(&[])), None);
    }
}

#[cfg(test)]
mod scoped_key_tests {
    use super::*;

    fn scope() -> Vec<String> {
        vec!["lab".into(), "web".into()]
    }

    #[test]
    fn cloud_routes_pass_through_to_their_own_project_check() {
        assert_eq!(scoped_decision(&scope(), &Method::POST, "/api/v1/cloud/projects/x/vpcs", None), Decision::Allow);
    }

    #[test]
    fn instance_routes_need_the_machine_to_be_in_scope() {
        let id = "/api/v1/vms/6f9619ff-8b86-d011-b42d-00cf4fc964ff/stop";
        assert_eq!(scoped_decision(&scope(), &Method::POST, id, Some("lab")), Decision::Allow);
        assert!(matches!(scoped_decision(&scope(), &Method::POST, id, Some("prod")), Decision::Deny(_)));
        assert!(matches!(scoped_decision(&scope(), &Method::GET, id, None), Decision::Deny(_)), "unknown machine: closed");
    }

    #[test]
    fn the_list_is_readable_but_creating_machines_is_not() {
        assert_eq!(scoped_decision(&scope(), &Method::GET, "/api/v1/vms", None), Decision::Allow);
        assert!(matches!(scoped_decision(&scope(), &Method::POST, "/api/v1/vms", None), Decision::Deny(_)));
        assert!(matches!(scoped_decision(&scope(), &Method::POST, "/api/v1/vms/from-template", None), Decision::Deny(_)));
    }

    #[test]
    fn everything_global_is_closed() {
        for p in ["/api/v1/hosts", "/api/v1/api-keys", "/api/v1/storage/pools", "/api/v1/zeus-security/hosts"] {
            assert!(matches!(scoped_decision(&scope(), &Method::GET, p, None), Decision::Deny(_)), "{p}");
        }
    }
}
