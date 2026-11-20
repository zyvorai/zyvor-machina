// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! In-process event bus + SSE stream `/api/v1/events/stream`.
//!
//! Lightweight pub/sub using `tokio::sync::broadcast` so any route handler can
//! emit a [`MachinaEvent`] (e.g., a successful KubeVirt qcow2 upload) and the
//! web UI can refresh affected views without polling.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::Extension;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::Router;
use futures_util::stream::{Stream, StreamExt};
use machina_core::LibvirtManager;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Single event broadcast on the bus. Kept small/JSON-stringified for SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachinaEvent {
    /// Short stable identifier, e.g. `kubevirt.qcow2.upload`.
    pub kind: String,
    /// Optional human/UI subject (qcow2 path, VM name, etc.).
    pub target: String,
    /// `ok` | `error` | `started` | … (UI-friendly).
    pub status: String,
    /// Optional sender note/message (truncated by callers as needed).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    /// Unix epoch milliseconds.
    pub timestamp_ms: i64,
}

impl MachinaEvent {
    pub fn now(
        kind: impl Into<String>,
        target: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            target: target.into(),
            status: status.into(),
            message: String::new(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Fan-out broadcaster shared between handlers and the SSE stream.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<MachinaEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity.max(8));
        Self { tx }
    }

    /// Best-effort emit (drops the event if no subscribers).
    pub fn emit(&self, ev: MachinaEvent) {
        let _ = self.tx.send(ev);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MachinaEvent> {
        self.tx.subscribe()
    }
}

async fn events_stream(
    Extension(bus): Extension<Arc<EventBus>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>> + Send> {
    let rx = bus.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|r| async move {
        match r {
            Ok(ev) => match serde_json::to_string(&ev) {
                Ok(json) => Some(Ok(Event::default().event(ev.kind.clone()).data(json))),
                Err(_) => None,
            },
            Err(_lag) => None,
        }
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub fn event_routes() -> Router<LibvirtManager> {
    Router::new().route("/events/stream", get(events_stream))
}
