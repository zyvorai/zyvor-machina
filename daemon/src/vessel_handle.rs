// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared Vessel (Podman/Docker) client handle for daemon routes.

use std::sync::Arc;

use machina_core::VesselConfig;
use tokio::sync::RwLock;
use tracing::{info, warn};
use vessel_core::VesselClient;

#[derive(Clone, Default)]
pub struct VesselHandle {
    inner: Arc<RwLock<VesselState>>,
}

#[derive(Default)]
struct VesselState {
    enabled: bool,
    client: Option<VesselClient>,
    last_error: Option<String>,
    socket: Option<String>,
}

impl VesselHandle {
    pub async fn from_config(cfg: &VesselConfig) -> Self {
        let handle = Self::default();
        {
            let mut g = handle.inner.write().await;
            g.enabled = cfg.enabled;
        }
        if cfg.enabled {
            handle.try_connect(cfg).await;
        } else {
            let mut g = handle.inner.write().await;
            g.last_error = Some("vessel.enabled is false in machina config".into());
        }
        handle
    }

    pub async fn try_connect(&self, cfg: &VesselConfig) {
        let socket_override = if cfg.socket.trim().is_empty() {
            if cfg.auto_discover {
                None
            } else {
                Some("")
            }
        } else {
            Some(cfg.socket.as_str())
        };

        match VesselClient::connect_local(socket_override).await {
            Ok(client) => {
                let sock = client.socket().to_string();
                let engine = format!("{:?}", client.engine_kind());
                info!(socket = %sock, engine = %engine, "Vessel connected");
                let mut g = self.inner.write().await;
                g.client = Some(client);
                g.socket = Some(sock);
                g.last_error = None;
                g.enabled = true;
            }
            Err(e) => {
                warn!(error = %e, "Vessel not available");
                let mut g = self.inner.write().await;
                g.client = None;
                g.socket = None;
                g.last_error = Some(e.to_string());
            }
        }
    }

    pub async fn client(&self) -> Result<VesselClient, VesselUnavailable> {
        let g = self.inner.read().await;
        if !g.enabled {
            return Err(VesselUnavailable {
                message: g
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "Vessel disabled".into()),
            });
        }
        g.client.clone().ok_or_else(|| VesselUnavailable {
            message: g
                .last_error
                .clone()
                .unwrap_or_else(|| "Vessel engine not connected".into()),
        })
    }

    pub async fn status_snapshot(&self) -> VesselStatusSnapshot {
        let g = self.inner.read().await;
        VesselStatusSnapshot {
            enabled: g.enabled,
            connected: g.client.is_some(),
            socket: g.socket.clone(),
            error: g.last_error.clone(),
            client: g.client.clone(),
        }
    }
}

#[derive(Debug)]
pub struct VesselUnavailable {
    pub message: String,
}

pub struct VesselStatusSnapshot {
    pub enabled: bool,
    pub connected: bool,
    pub socket: Option<String>,
    pub error: Option<String>,
    pub client: Option<VesselClient>,
}
