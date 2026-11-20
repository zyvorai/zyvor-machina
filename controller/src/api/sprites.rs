// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Read-only fleet view of sprites — see `engine::sprite_inventory` for why
//! this never touches the `vms` table/reconciler. Data comes entirely from
//! the in-memory cache populated as a side effect of `host.inventory`; this
//! handler never talks to a host directly, so it's always fast and never
//! blocks on a slow/unreachable agent.

use std::collections::{HashMap, HashSet};

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct SpriteFleetRow {
    pub host_id: Uuid,
    pub host_name: Option<String>,
    pub sprite_id: String,
    pub state: String,
    pub backend: String,
    pub created_at: String,
    pub expires_at: String,
    pub vsock_cid: Option<u32>,
    pub network_egress: bool,
}

#[derive(Debug, Serialize)]
pub struct HostSpriteStatus {
    pub host_id: Uuid,
    pub host_name: Option<String>,
    pub reachable: bool,
    pub error: Option<String>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
    pub sprite_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SpriteFleetResponse {
    pub sprites: Vec<SpriteFleetRow>,
    pub hosts: Vec<HostSpriteStatus>,
}

#[derive(Debug, Deserialize)]
pub struct SpriteFleetQuery {
    #[serde(default)]
    pub host_id: Option<Uuid>,
}

pub async fn list_fleet_sprites(
    State(state): State<AppState>,
    Query(q): Query<SpriteFleetQuery>,
) -> Json<SpriteFleetResponse> {
    let snapshots = match q.host_id {
        Some(host_id) => state
            .sprite_inventory
            .snapshot_host(host_id)
            .await
            .into_iter()
            .collect::<Vec<_>>(),
        None => state.sprite_inventory.snapshot_all().await,
    };

    let unique_host_ids: HashSet<Uuid> = snapshots.iter().map(|s| s.host_id).collect();
    let mut names: HashMap<Uuid, String> = HashMap::new();
    for host_id in unique_host_ids {
        if let Ok(Some(name)) =
            crate::db::query_scalar::<_, String>("SELECT hostname FROM hosts WHERE id = ?")
                .bind(host_id)
                .fetch_optional(&state.pool)
                .await
        {
            names.insert(host_id, name);
        }
    }

    let mut sprites = Vec::new();
    let mut hosts = Vec::new();
    for snap in snapshots {
        let host_name = names.get(&snap.host_id).cloned();
        hosts.push(HostSpriteStatus {
            host_id: snap.host_id,
            host_name: host_name.clone(),
            reachable: snap.reachable,
            error: snap.error.clone(),
            fetched_at: snap.fetched_at,
            sprite_count: snap.sprites.len(),
        });
        for s in snap.sprites {
            sprites.push(SpriteFleetRow {
                host_id: snap.host_id,
                host_name: host_name.clone(),
                sprite_id: s.sprite_id,
                state: s.state,
                backend: s.backend,
                created_at: s.created_at,
                expires_at: s.expires_at,
                vsock_cid: (s.vsock_cid != 0).then_some(s.vsock_cid),
                network_egress: s.network_egress,
            });
        }
    }

    Json(SpriteFleetResponse { sprites, hosts })
}
