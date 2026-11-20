// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMessage {
    pub task_id: Uuid,
    pub operation: String,
    pub payload: serde_json::Value,
}

#[async_trait]
pub trait TaskBus: Send + Sync {
    async fn publish(&self, subject: &str, msg: &TaskMessage) -> anyhow::Result<()>;
}

pub mod bus;
pub mod enqueue;
pub mod nats_subscriber;
pub mod worker;
