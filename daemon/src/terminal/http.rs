// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Duration;

use axum::extract::Extension;
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_core::{LibvirtError, LibvirtManager, SshTerminalConfig};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::auth::RequestActor;
use crate::error::AppError;
use crate::terminal::TerminalSessionStore;

#[derive(Debug, Deserialize)]
pub struct CreateSessionBody {
    pub target_id: Option<String>,
    pub host: Option<String>,
    #[serde(default = "default_ssh_user")]
    pub ssh_user: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_ssh_user() -> String {
    "root".into()
}

fn session_ttl(cfg: &SshTerminalConfig) -> Duration {
    let s = cfg.session_ttl_secs.clamp(30, 3600);
    Duration::from_secs(s)
}

fn validate_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ' ' | '\0' | ';' | '|' | '&' | '$' | '`' | '\n' | '\r' | '<' | '>'
            )
        })
}

fn validate_ssh_user(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 32
        // Bar a leading '-' so the user token can't become an ssh flag (e.g. -oProxyCommand=).
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

async fn list_targets_handler(
    Extension(cfg): Extension<SshTerminalConfig>,
) -> Json<serde_json::Value> {
    let rows: Vec<_> = cfg
        .targets
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "host": t.host,
                "default_ssh_user": t.ssh_user,
            })
        })
        .collect();
    Json(json!({ "targets": rows }))
}

async fn create_session_handler(
    Extension(store): Extension<TerminalSessionStore>,
    Extension(cfg): Extension<SshTerminalConfig>,
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if actor.from_api_token {
        return Err(LibvirtError::Forbidden(
            "SSH browser terminal requires a browser session (API tokens cannot create terminal sessions)".into(),
        )
        .into());
    }
    // Interactive host SSH is at least as sensitive as USB/PCI passthrough — gate on
    // operator/admin so a read-only session can't open a shell to configured hosts.
    if !actor.role.can_write() {
        return Err(LibvirtError::Forbidden(
            "Opening an SSH terminal requires the operator or admin role.".into(),
        )
        .into());
    }

    let ttl = session_ttl(&cfg);
    let mut resolved_user = body.ssh_user.trim().to_string();
    if resolved_user.is_empty() {
        resolved_user = default_ssh_user();
    }
    if !validate_ssh_user(&resolved_user) {
        return Err(LibvirtError::Invalid(
            "ssh_user must be 1–32 ASCII letters, digits, '_', '-', or '.'".into(),
        )
        .into());
    }

    let resolved_host = if let Some(tid) = body
        .target_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let t =
            cfg.targets.iter().find(|t| t.id == tid).ok_or_else(|| {
                LibvirtError::NotFound(format!("Unknown terminal target_id '{tid}'"))
            })?;
        if !t.ssh_user.trim().is_empty() {
            resolved_user = t.ssh_user.trim().to_string();
        }
        if !validate_ssh_user(&resolved_user) {
            return Err(LibvirtError::Invalid("Resolved ssh_user is invalid".into()).into());
        }
        t.host.clone()
    } else if cfg.allow_adhoc_hosts {
        let h = body
            .host
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                LibvirtError::Invalid("Provide host (ad-hoc) or target_id (mapped target)".into())
            })?;
        if !validate_host(h) {
            return Err(LibvirtError::Invalid("Invalid or disallowed host".into()).into());
        }
        h.to_string()
    } else {
        return Err(LibvirtError::Forbidden(
            "Ad-hoc SSH hosts are disabled; set ssh_terminal.allow_adhoc_hosts or use target_id"
                .into(),
        )
        .into());
    };

    let ssh_port = if body.ssh_port == 0 {
        22
    } else {
        body.ssh_port
    };

    let session_id = store.insert_session(
        resolved_host.clone(),
        resolved_user.clone(),
        ssh_port,
        actor.username.clone(),
        ttl,
    );

    info!(
        target: "audit",
        event = "ssh_terminal_session_created",
        username = %actor.username,
        session_id = %session_id,
        ssh_host = %resolved_host,
        ssh_user = %resolved_user,
    );

    Ok(Json(json!({
        "session_id": session_id,
        "expires_in_secs": ttl.as_secs(),
    })))
}

pub fn routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/terminal/targets", get(list_targets_handler))
        .route("/terminal/sessions", post(create_session_handler))
}
