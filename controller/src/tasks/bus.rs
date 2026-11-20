// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::{TaskBus, TaskMessage};

pub struct InMemoryTaskBus {
    tx: mpsc::UnboundedSender<TaskMessage>,
}

impl InMemoryTaskBus {
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<TaskMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Arc::new(Self { tx }), rx)
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<TaskMessage> {
        self.tx.clone()
    }
}

#[async_trait]
impl TaskBus for InMemoryTaskBus {
    async fn publish(&self, _subject: &str, msg: &TaskMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg.clone())
            .map_err(|e| anyhow::anyhow!("task bus send: {e}"))?;
        Ok(())
    }
}

pub struct NatsTaskBus {
    client: async_nats::Client,
    /// This controller instance's id, stamped as a message header on every
    /// publish so our own `nats_subscriber` can recognize (and drop) the echo
    /// of our own publish instead of re-forwarding it into the local worker
    /// channel — see `FanoutTaskBus::publish` for why that matters.
    controller_id: String,
}

impl NatsTaskBus {
    pub async fn connect(url: &str, controller_id: String) -> anyhow::Result<Arc<Self>> {
        let client = async_nats::connect(url).await?;
        Ok(Arc::new(Self {
            client,
            controller_id,
        }))
    }
}

#[async_trait]
impl TaskBus for NatsTaskBus {
    async fn publish(&self, subject: &str, msg: &TaskMessage) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(msg)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("X-Machina-Origin", self.controller_id.as_str());
        self.client
            .publish_with_headers(subject.to_string(), headers, payload.into())
            .await?;
        Ok(())
    }
}

pub struct FanoutTaskBus {
    local: Arc<InMemoryTaskBus>,
    nats: Option<Arc<NatsTaskBus>>,
}

impl FanoutTaskBus {
    pub fn new(local: Arc<InMemoryTaskBus>, nats: Option<Arc<NatsTaskBus>>) -> Arc<Self> {
        Arc::new(Self { local, nats })
    }
}

#[async_trait]
impl TaskBus for FanoutTaskBus {
    async fn publish(&self, subject: &str, msg: &TaskMessage) -> anyhow::Result<()> {
        // Always deliver locally first: this controller's own worker must pick up
        // the task even if NATS is down/slow, so local delivery never depends on
        // the broker round-trip.
        //
        // The NATS publish (below) is for FAN-OUT to peer controllers only.
        // async_nats::Client::subscribe also delivers this same publish back to
        // OUR OWN nats_subscriber (NATS has no built-in "don't echo to publisher"
        // semantics), which would otherwise forward it into `local_tx` a second
        // time — double-delivering every task this controller enqueues to its own
        // worker. `NatsTaskBus::publish` stamps an `X-Machina-Origin` header with
        // this controller's id specifically so `nats_subscriber` can recognize and
        // drop that echo instead of re-forwarding it; do not remove that stamping
        // without also removing the corresponding local.publish() call here, or
        // double-delivery comes back.
        self.local.publish(subject, msg).await?;
        if let Some(nats) = &self.nats {
            let _ = nats.publish(subject, msg).await;
        }
        Ok(())
    }
}
