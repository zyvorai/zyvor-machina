// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Read-only, in-memory fleet view of disposable "sprite" microVMs.
//!
//! Sprites are deliberately never written into the `vms` table or touched
//! by the reconciler (see `daemon/src/sprite_registry.rs`'s doc comment) —
//! they're TTL-reaped, per-host, and wiped on daemon restart, so treating
//! them as reconciler-managed resources would fight that design instead of
//! just adding visibility. This cache is refreshed as a side effect of the
//! existing per-host `host.inventory` task (see `tasks/worker.rs`) and
//! never persisted — a controller restart just starts the cache empty
//! again until the next inventory tick repopulates it.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use machina_agent::pb::SpriteSummary;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct HostSpriteSnapshot {
    pub host_id: Uuid,
    pub fetched_at: DateTime<Utc>,
    /// False when the agent couldn't reach its co-located daemon (or the
    /// agent itself is unreachable) — `sprites` is empty in that case and
    /// must not be read as "this host has no sprites running".
    pub reachable: bool,
    pub error: Option<String>,
    pub sprites: Vec<SpriteSummary>,
}

#[derive(Clone, Default)]
pub struct SpriteInventoryCache {
    inner: Arc<RwLock<HashMap<Uuid, HostSpriteSnapshot>>>,
}

impl SpriteInventoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn update(&self, snapshot: HostSpriteSnapshot) {
        self.inner.write().await.insert(snapshot.host_id, snapshot);
    }

    /// Drop a host's cached snapshot (e.g. when the host is deleted) so a
    /// removed host doesn't linger in fleet-visibility output forever.
    pub async fn remove(&self, host_id: Uuid) {
        self.inner.write().await.remove(&host_id);
    }

    pub async fn snapshot_all(&self) -> Vec<HostSpriteSnapshot> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn snapshot_host(&self, host_id: Uuid) -> Option<HostSpriteSnapshot> {
        self.inner.read().await.get(&host_id).cloned()
    }
}
