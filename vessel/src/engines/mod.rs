// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#[cfg(unix)]
mod bollard_engine;
#[cfg(unix)]
mod libpod;

#[cfg(unix)]
pub use bollard_engine::{connect_local, discover_socket, BollardEngine};

#[cfg(not(unix))]
use crate::error::VesselError;
#[cfg(not(unix))]
use crate::models::EngineKind;
#[cfg(not(unix))]
use std::sync::Arc;

#[cfg(not(unix))]
pub async fn connect_local(
    _socket_override: Option<&str>,
) -> Result<(Arc<dyn crate::engine::Engine>, EngineKind, String), VesselError> {
    Err(VesselError::Connection(
        "Vessel container engine requires a Unix socket (Linux/macOS with Podman/Docker)".into(),
    ))
}

#[cfg(not(unix))]
pub fn discover_socket(_override: Option<&str>) -> Option<String> {
    None
}
