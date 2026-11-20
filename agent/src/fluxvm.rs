// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! This host's `fluxvm-api`, for the controller: inventory, lifecycle and the
//! two halves of a FluxVM live migration.
//!
//! Endpoint: `MACHINA_FLUXVM_URL` (+ `MACHINA_FLUXVM_TOKEN`,
//! `MACHINA_FLUXVM_INSECURE_TLS=1`), else the daemon's `[fluxvm]` section when
//! it is enabled.

#![allow(clippy::result_large_err)]

use machina_core::fluxvm::migrate::{progress, Progress};
use machina_core::fluxvm::{FluxRecord, FluxvmClient};
use machina_core::{FluxvmConfig, LibvirtError, MachinaConfig};
use serde_json::Value;
use tonic::Status;

use crate::pb::VmSummary;

fn env_config() -> Option<FluxvmConfig> {
    let url = std::env::var("MACHINA_FLUXVM_URL").ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    Some(FluxvmConfig {
        enabled: true,
        base_url: url.to_string(),
        token: std::env::var("MACHINA_FLUXVM_TOKEN").unwrap_or_default(),
        insecure_tls: matches!(
            std::env::var("MACHINA_FLUXVM_INSECURE_TLS").as_deref(),
            Ok("1") | Ok("true")
        ),
        ..FluxvmConfig::default()
    })
}

/// `None` when FluxVM isn't configured on this host.
pub fn client() -> Option<FluxvmClient> {
    let cfg = env_config().unwrap_or_else(|| MachinaConfig::load().fluxvm);
    if !cfg.enabled {
        return None;
    }
    FluxvmClient::from_config(&cfg).ok()
}

pub fn require() -> Result<FluxvmClient, Status> {
    client().ok_or_else(|| {
        Status::failed_precondition(
            "FluxVM is not configured on this host (MACHINA_FLUXVM_URL or [fluxvm] enabled)",
        )
    })
}

pub fn status(e: LibvirtError) -> Status {
    match e {
        LibvirtError::NotFound(m) => Status::not_found(m),
        LibvirtError::Invalid(m) => Status::invalid_argument(m),
        LibvirtError::Forbidden(m) => Status::permission_denied(m),
        LibvirtError::Connection(m) => Status::unavailable(m),
        other => {
            let m = other.to_string();
            if m.contains("409") || m.to_ascii_lowercase().contains("conflict") {
                Status::failed_precondition(m)
            } else {
                Status::internal(m)
            }
        }
    }
}

pub fn summary(raw: &Value) -> Option<VmSummary> {
    let rec: FluxRecord = serde_json::from_value(raw.clone()).ok()?;
    let info = rec.to_vm_info();
    Some(VmSummary {
        name: rec.name.clone(),
        uuid: rec.id.clone(),
        state: info.state,
        vcpus: info.vcpus,
        memory_mb: info.memory_mb,
        guest_ip: rec.guest_ip.clone().unwrap_or_default(),
        guest_ips: info.guest_ips,
        backend: machina_core::fluxvm::BACKEND_NAME.into(),
        fluxvm_engine: rec.backend.clone(),
        fluxvm_storage: rec.request.storage.clone(),
        disk: rec.disk.clone(),
        fluxvm_record_json: raw.to_string(),
        ..Default::default()
    })
}

/// FluxVM VMs on this host; `None` when unconfigured or unreachable.
pub async fn summaries() -> Option<Vec<VmSummary>> {
    let c = client()?;
    match c.list_raw().await {
        Ok(items) => Some(items.iter().filter_map(summary).collect()),
        Err(e) => {
            tracing::warn!("fluxvm: list failed: {e}");
            None
        }
    }
}

/// Agent power actions → FluxVM verbs.
pub fn verb(action: &str) -> Result<&'static str, Status> {
    Ok(match action {
        "start" => "start",
        "stop" | "shutdown" | "destroy" => "stop",
        "reboot" | "reset" | "restart" => "restart",
        "pause" | "suspend" => "pause",
        "resume" => "resume",
        other => {
            return Err(Status::invalid_argument(format!(
                "power action '{other}' is not supported for FluxVM VMs"
            )))
        }
    })
}

pub fn progress_of(status: &Value) -> (&'static str, String) {
    match progress(status) {
        Progress::Running => ("running", String::new()),
        Progress::Completed => ("completed", String::new()),
        Progress::Failed(e) => ("failed", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_carries_engine_storage_and_record() {
        let raw = json!({
            "id": "abc", "name": "web-1", "backend": "qemu", "status": "running",
            "disk": "/srv/shared/web-1.raw",
            "request": {"vcpus": 2, "memory_mib": 1024, "storage": "shared"}
        });
        let s = summary(&raw).unwrap();
        assert_eq!((s.name.as_str(), s.uuid.as_str()), ("web-1", "abc"));
        assert_eq!(s.backend, "fluxvm");
        assert_eq!(s.fluxvm_engine, "qemu");
        assert_eq!(s.fluxvm_storage, "shared");
        assert_eq!(s.disk, "/srv/shared/web-1.raw");
        assert_eq!(s.state, "running");
        assert_eq!((s.vcpus, s.memory_mb), (2, 1024));
        let back: Value = serde_json::from_str(&s.fluxvm_record_json).unwrap();
        assert_eq!(back["id"], "abc");
    }

    #[test]
    fn power_actions_map_to_fluxvm_verbs() {
        assert_eq!(verb("shutdown").unwrap(), "stop");
        assert_eq!(verb("stop").unwrap(), "stop");
        assert_eq!(verb("reboot").unwrap(), "restart");
        assert_eq!(verb("resume").unwrap(), "resume");
        assert!(verb("managedsave").is_err());
    }
}
