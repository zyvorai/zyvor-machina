// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use futures_util::StreamExt;
use tokio::sync::mpsc;

use super::TaskMessage;

/// Sentinel error returned by `run()` when the local delivery channel is
/// closed (the receiving end — the task worker — has been dropped). This is
/// distinct from a NATS-connection-level failure: reconnecting to NATS can
/// never fix a closed local channel, so the outer retry loop must stop
/// looping (not just log-and-retry-forever) when it sees this specific error.
const LOCAL_CHANNEL_CLOSED: &str = "local task channel closed";

pub fn spawn(nats_url: String, tx: mpsc::UnboundedSender<TaskMessage>, self_controller_id: String) {
    tokio::spawn(async move {
        loop {
            match run(&nats_url, tx.clone(), &self_controller_id).await {
                Ok(()) => tracing::warn!("NATS task subscriber ended; reconnecting in 5s"),
                Err(e) if e.to_string() == LOCAL_CHANNEL_CLOSED => {
                    // The receiving end (task worker) is gone for good — retrying
                    // the NATS connection can never route a task anywhere again.
                    // Log loudly and distinctly from normal reconnect noise, and
                    // stop the retry loop: looping forever here would silently
                    // mask the fact that NATS-sourced tasks are now a dead end
                    // for the lifetime of this process.
                    tracing::error!(
                        "NATS task subscriber: local task channel is permanently closed; \
                         NATS-sourced task delivery has stopped for this process and will \
                         NOT recover without a restart. Giving up on reconnecting."
                    );
                    break;
                }
                Err(e) => tracing::warn!("NATS task subscriber: {e:#}; reconnecting in 5s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

async fn run(
    nats_url: &str,
    tx: mpsc::UnboundedSender<TaskMessage>,
    self_controller_id: &str,
) -> anyhow::Result<()> {
    let client = async_nats::connect(nats_url).await?;
    let mut sub = client.subscribe("machina.tasks").await?;
    tracing::info!("NATS task subscriber listening on machina.tasks");
    while let Some(msg) = sub.next().await {
        // NATS echoes every publish back to this same process's subscription.
        // `FanoutTaskBus::publish` already delivers our own tasks straight into
        // the local worker channel, so forwarding our own echo here again would
        // double-deliver (and, symmetrically, every peer controller would also
        // double-deliver its own). `NatsTaskBus::publish` stamps the publishing
        // controller's id in the `X-Machina-Origin` header; drop the message here
        // when that id is ours. Messages with no header (e.g. from an
        // old-version peer mid-rolling-upgrade) are forwarded as before — safe
        // default is "deliver", not "drop", since dropping a legit peer task
        // would strand it forever.
        let origin = msg
            .headers
            .as_ref()
            .and_then(|h| h.get("X-Machina-Origin"))
            .map(|v| v.to_string());
        if origin.as_deref() == Some(self_controller_id) {
            continue;
        }
        match serde_json::from_slice::<TaskMessage>(&msg.payload) {
            Ok(task) => {
                if tx.send(task).is_err() {
                    anyhow::bail!(LOCAL_CHANNEL_CLOSED);
                }
            }
            Err(e) => tracing::warn!("NATS task decode failed: {e}"),
        }
    }
    Ok(())
}
