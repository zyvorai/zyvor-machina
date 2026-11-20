// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Optional `?connection=system|session` for Cockpit-style dual libvirt hypervisors,
//! and `?backend=fluxvm` for VMs managed by `fluxvm-api` instead of libvirt.

use serde::Deserialize;
use virt::connect::Connect;

use machina_core::{LibvirtError, LibvirtManager, LibvirtTarget, MachinaConfig};

use crate::auth::{effective_linux_user, RequestActor};
use crate::error::AppError;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConnQuery {
    #[serde(default)]
    pub connection: Option<String>,
    /// `fluxvm` routes the call to `fluxvm-api`; absent / `libvirt` = libvirt.
    #[serde(default)]
    pub backend: Option<String>,
}

impl ConnQuery {
    pub fn is_fluxvm(&self) -> bool {
        machina_core::fluxvm::is_fluxvm(self.backend.as_deref())
    }

    pub fn target(&self, mgr: &LibvirtManager) -> LibvirtTarget {
        mgr.resolve_query(self.connection.as_deref())
    }
}

pub fn connection_label(dual: bool, target: LibvirtTarget) -> Option<String> {
    if !dual {
        return None;
    }
    Some(
        match target {
            LibvirtTarget::Session => "session",
            _ => "system",
        }
        .to_string(),
    )
}

/// When OIDC run-as-user impersonation is active, default empty `?connection=` to session.
pub fn apply_impersonation_session_default(
    actor: &RequestActor,
    mut conn_q: ConnQuery,
) -> ConnQuery {
    let cfg = MachinaConfig::load();
    if !cfg.auth.run_as_user.prefer_session_libvirt_on_impersonation {
        return conn_q;
    }
    if !cfg.auth.run_as_user.impersonation_active() {
        return conn_q;
    }
    if effective_linux_user(actor).is_none() {
        return conn_q;
    }
    if !cfg.libvirt.dual_connection {
        return conn_q;
    }
    let empty = conn_q
        .connection
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    if empty {
        conn_q.connection = Some("session".into());
    }
    conn_q
}

pub fn impersonation_prefers_session_only(actor: &RequestActor) -> bool {
    let cfg = MachinaConfig::load();
    cfg.auth.run_as_user.prefer_session_libvirt_on_impersonation
        && cfg.auth.run_as_user.impersonation_active()
        && effective_linux_user(actor).is_some()
        && cfg.libvirt.dual_connection
}

/// Run a libvirt call on the hypervisor selected by [`ConnQuery`], applying
/// session defaults for impersonated OIDC users (blocking pool).
pub async fn spawn_libvirt_actor<R>(
    manager: LibvirtManager,
    actor: Option<&RequestActor>,
    conn_q: ConnQuery,
    op: impl FnOnce(&Connect) -> Result<R, LibvirtError> + Send + 'static,
) -> Result<R, AppError>
where
    R: Send + 'static,
{
    if conn_q.is_fluxvm() {
        return Err(AppError::from(LibvirtError::Invalid(
            "this operation is not supported for FluxVM VMs (backend=fluxvm)".into(),
        )));
    }
    let conn_q = match actor {
        Some(a) => apply_impersonation_session_default(a, conn_q),
        None => conn_q,
    };
    let mgr = manager;
    tokio::task::spawn_blocking(move || {
        let t = conn_q.target(&mgr);
        mgr.with_conn_target(t, op)
    })
    .await
    .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?
    .map_err(AppError::from)
}
