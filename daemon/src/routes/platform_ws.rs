// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Same-origin WebSocket proxy to machina-controller platform VNC/serial (controller validates ?token=).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use machina_core::LibvirtManager;
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message as TsMessage};

use super::platform_controller::controller_base;

#[derive(Debug, Deserialize)]
struct PlatformWsQuery {
    token: String,
}

fn controller_ws_base() -> String {
    let base = controller_base();
    if base.starts_with("https://") {
        base.replacen("https://", "wss://", 1)
    } else {
        base.replacen("http://", "ws://", 1)
    }
}

async fn relay_platform_ws(socket: WebSocket, upstream_path: String, token: String) {
    let url = format!("{upstream_path}?token={token}");

    let upstream = match connect_async(&url).await {
        Ok((stream, _)) => stream,
        Err(_) => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    let (mut client_sink, mut client_stream) = socket.split();
    let (mut upstream_sink, mut upstream_stream) = upstream.split();

    let c2u = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let up = match msg {
                Message::Binary(b) => TsMessage::Binary(b),
                Message::Text(t) => TsMessage::Text(t.as_str().into()),
                Message::Close(_) => {
                    let _ = upstream_sink.send(TsMessage::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if upstream_sink.send(up).await.is_err() {
                break;
            }
        }
    });

    let u2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = upstream_stream.next().await {
            let down = match msg {
                TsMessage::Binary(b) => Message::Binary(b),
                TsMessage::Text(t) => Message::Text(t.as_str().into()),
                TsMessage::Close(_) => {
                    let _ = client_sink.send(Message::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if client_sink.send(down).await.is_err() {
                break;
            }
        }
    });

    // Abort the surviving direction when either ends — dropping a JoinHandle
    // detaches rather than cancels, so without this a closed client tab left the
    // u2c task blocked on upstream.next(), leaking the upstream WS + task.
    let c2u_abort = c2u.abort_handle();
    let u2c_abort = u2c.abort_handle();
    tokio::select! {
        _ = c2u => { u2c_abort.abort(); },
        _ = u2c => { c2u_abort.abort(); },
    }
}

async fn platform_vnc_ws_proxy(
    ws: WebSocketUpgrade,
    Path(vm_id): Path<String>,
    Query(q): Query<PlatformWsQuery>,
) -> impl IntoResponse {
    let path = format!("{}/ws/v1/platform/vnc/{vm_id}", controller_ws_base());
    // See daemon/src/routes/ws.rs::vnc_handler — novnc-core requests the
    // 'binary' subprotocol and Chrome fails the connection (code 1006) unless
    // the server echoes it back, even though the HTTP upgrade itself succeeds.
    ws.protocols(["binary"])
        .on_upgrade(move |socket| relay_platform_ws(socket, path, q.token))
}

async fn platform_serial_ws_proxy(
    ws: WebSocketUpgrade,
    Path(vm_id): Path<String>,
    Query(q): Query<PlatformWsQuery>,
) -> impl IntoResponse {
    let path = format!("{}/ws/v1/platform/serial/{vm_id}", controller_ws_base());
    ws.on_upgrade(move |socket| relay_platform_ws(socket, path, q.token))
}

async fn platform_spice_ws_proxy(
    ws: WebSocketUpgrade,
    Path(vm_id): Path<String>,
    Query(q): Query<PlatformWsQuery>,
) -> impl IntoResponse {
    let path = format!("{}/ws/v1/platform/spice/{vm_id}", controller_ws_base());
    ws.on_upgrade(move |socket| relay_platform_ws(socket, path, q.token))
}

/// `GET /ws/v1/baremetal/{id}/sol?token=` -> the controller's serial-over-LAN WebSocket, so
/// clients use one origin (and one pinned certificate). The controller validates the
/// single-use token; only a UUID id and a URL-safe token are forwarded.
async fn baremetal_sol_ws_proxy(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(q): Query<PlatformWsQuery>,
) -> axum::response::Response {
    let token_ok = !q.token.is_empty()
        && q.token.len() <= 512
        && q
            .token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    let Ok(id) = uuid::Uuid::parse_str(&id) else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid server id").into_response();
    };
    if !token_ok {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid token").into_response();
    }
    let path = format!("{}/ws/v1/baremetal/{id}/sol", controller_ws_base());
    ws.on_upgrade(move |socket| relay_platform_ws(socket, path, q.token))
        .into_response()
}

pub fn platform_ws_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/platform/vnc/{vm_id}", get(platform_vnc_ws_proxy))
        .route("/platform/serial/{vm_id}", get(platform_serial_ws_proxy))
        .route("/platform/spice/{vm_id}", get(platform_spice_ws_proxy))
        .route("/baremetal/{id}/sol", get(baremetal_sol_ws_proxy))
}
