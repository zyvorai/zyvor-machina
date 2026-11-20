// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use crate::state::AppState;

use super::{detection, forward_all_integrations, ingest};

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
        loop {
            interval.tick().await;
            if !state.leader.is_leader() {
                continue;
            }
            let pool = state.pool.clone();
            let cfg = state.config.clone();
            let controller_id = cfg.controller_id.clone();

            if let Err(e) = ingest::ingest_recent(&pool, &cfg).await {
                tracing::warn!("soc ingest: {e:#}");
            }
            if let Err(e) = detection::run_detection(&pool).await {
                tracing::warn!("soc detection: {e:#}");
            }
            if let Err(e) = forward_all_integrations(&pool, &controller_id).await {
                tracing::warn!("soc siem forward: {e:#}");
            }
        }
    });
}
