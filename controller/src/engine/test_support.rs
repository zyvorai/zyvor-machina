// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared helpers for engine unit tests: an `AppState` backed by a fresh
//! in-memory SQLite database and an in-memory task bus. Keep the returned
//! receiver alive for the duration of the test — dropping it makes every
//! `enqueue_task` publish fail (useful for exercising enqueue-failure paths).

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

use crate::config::ControllerConfig;
use crate::leader::LeaderHandle;
use crate::state::AppState;
use crate::tasks::{bus::InMemoryTaskBus, TaskBus, TaskMessage};

/// A fresh, migrated database for one test (see `db::testing`).
pub(crate) async fn test_pool() -> crate::db::DbPool {
    crate::db::testing::pool().await
}

pub(crate) async fn test_state() -> (AppState, UnboundedReceiver<TaskMessage>) {
    let pool = test_pool().await;
    let config = Arc::new(ControllerConfig::default());
    let (task_bus, rx) = InMemoryTaskBus::new();
    let task_bus = task_bus as Arc<dyn TaskBus>;
    (
        AppState::new(pool, config, task_bus, LeaderHandle::disconnected()),
        rx,
    )
}

/// Seed one online host and return its id. Keep the INSERT here so every engine
/// test survives hosts-schema changes by editing a single place.
pub(crate) async fn seed_host(pool: &crate::db::DbPool, id: Uuid) -> Uuid {
    crate::db::query("INSERT INTO hosts (id, hostname, state) VALUES (?, 'h1', 'online')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}
