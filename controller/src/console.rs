// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message as TsMessage};
use uuid::Uuid;

use axum::Extension;

use crate::agent_client;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

/// `?token=…` query string authenticating this controller to the agent's console port,
/// or empty when no token is configured: the host's own token if it joined with one, else the
/// shared `MACHINA_AGENT_TOKEN`.
fn agent_console_token_qs(console_addr: &str) -> String {
    match agent_client::token_for(console_addr) {
        Some(t) => format!("?token={}", urlencoding::encode(&t)),
        None => String::new(),
    }
}

#[derive(Debug, Serialize)]
pub struct ConsoleInfo {
    pub vm_id: String,
    pub vm_name: String,
    pub console_type: String,
    pub ws_path: String,
}

#[derive(Debug, Deserialize)]
pub struct WsTokenQuery {
    pub token: String,
}

pub async fn vm_console(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConsoleInfo>, ApiError> {
    require_operator(&actor)?;
    let row: (String, Option<Uuid>) =
        crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("vm has no host"))?;
    let agent_addr = host_agent_addr(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr).await?;
    let info = agent_client::get_console(&mut client, &row.0).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Not found") || msg.contains("not found") {
            ApiError::not_found(format!(
                "VM '{}' is not on the hypervisor (stopped, removed, or inventory stale)",
                row.0
            ))
            .with_remediation(
                "Platform → Hosts → Sync all, then open the VM from Platform → VMs (running guests only).",
            )
        } else if msg.contains("socket is closed") {
            ApiError::internal("Host agent lost its libvirt connection")
                .with_remediation("On the host: sudo systemctl restart libvirtd machina-agent")
        } else {
            ApiError::internal(msg)
        }
    })?;
    // require_operator above guarantees write capability, so this token is not read-only.
    let ws_token = state.ws_tokens.issue(id, false).await;
    Ok(Json(ConsoleInfo {
        vm_id: id.to_string(),
        vm_name: row.0,
        console_type: info.console_type,
        ws_path: format!("/ws/v1/platform/vnc/{id}?token={ws_token}"),
    }))
}

pub async fn issue_ws_token(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let _exists: Uuid = crate::db::query_scalar("SELECT id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let token = state.ws_tokens.issue(id, false).await;
    Ok(Json(serde_json::json!({ "token": token })))
}

/// KubeVirt VMs have no `host_id`/agent — their console lives on whichever
/// daemon has `kubectl` access to the cluster (`daemon/src/kubevirt_k8s_ws_proxy.rs`).
/// `None` for a libvirt VM (the common case, checked first so a plain DB
/// error doesn't misroute a normal console open).
async fn kubevirt_target(pool: &crate::db::DbPool, vm_id: Uuid) -> Option<(String, String)> {
    let row: Option<(String, Option<String>, String)> = crate::db::query_as(
        "SELECT name, k8s_namespace, COALESCE(inventory_source, 'libvirt') FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let (name, namespace, source) = row?;
    if source != "kubevirt" {
        return None;
    }
    Some((namespace.unwrap_or_else(|| "default".into()), name))
}

pub async fn vnc_ws_proxy(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(vm_id): Path<Uuid>,
    Query(q): Query<WsTokenQuery>,
) -> impl IntoResponse {
    let Some(grant) = state
        .ws_tokens
        .validate(&q.token)
        .await
        .filter(|g| g.vm_id == vm_id)
    else {
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    let read_only = grant.read_only;
    if let Some((namespace, name)) = kubevirt_target(&state.pool, vm_id).await {
        return ws.on_upgrade(move |socket| {
            proxy_to_daemon_kubevirt(socket, state, namespace, name, "vnc", read_only)
        });
    }
    ws.on_upgrade(move |socket| proxy_to_agent_vnc(socket, state, vm_id, read_only))
}

pub async fn serial_ws_proxy(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(vm_id): Path<Uuid>,
    Query(q): Query<WsTokenQuery>,
) -> impl IntoResponse {
    let Some(grant) = state
        .ws_tokens
        .validate(&q.token)
        .await
        .filter(|g| g.vm_id == vm_id)
    else {
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    // A serial console is inherently interactive input; a read-only/viewer grant
    // must not open one (matches check_console_rbac's intent).
    if grant.read_only {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "read-only session may not open an interactive serial console",
        )
            .into_response();
    }
    if let Some((namespace, name)) = kubevirt_target(&state.pool, vm_id).await {
        return ws.on_upgrade(move |socket| {
            proxy_to_daemon_kubevirt(socket, state, namespace, name, "console", false)
        });
    }
    ws.on_upgrade(move |socket| proxy_to_agent_serial(socket, state, vm_id, false))
}

async fn vm_agent_target(state: &AppState, vm_id: Uuid) -> Option<(String, String)> {
    let row = crate::db::query_as::<_, (String, Option<Uuid>)>(
        "SELECT name, host_id FROM vms WHERE id = ?",
    )
    .bind(vm_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()?;

    let (name, Some(host_id)) = row else {
        return None;
    };

    let agent_console = host_console_addr(&state.pool, host_id).await.ok()?;
    Some((name, agent_console))
}

async fn proxy_to_agent_vnc(socket: WebSocket, state: AppState, vm_id: Uuid, read_only: bool) {
    let Some((name, agent_console)) = vm_agent_target(&state, vm_id).await else {
        let (mut sink, _) = socket.split();
        let _ = sink.close().await;
        return;
    };

    let ws_url = format!(
        "ws://{}/ws/vnc/{}{}",
        agent_client::normalize_agent_addr(&agent_console),
        name,
        agent_console_token_qs(&agent_console)
    );

    let agent_ws = match connect_async(&ws_url).await {
        Ok((stream, _)) => stream,
        Err(_) => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    let (mut client_sink, mut client_stream) = socket.split();
    let (mut agent_sink, mut agent_stream) = agent_ws.split();

    let c2a = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let up = match msg {
                // Read-only grant: drop client input frames (keyboard/mouse/
                // clipboard) so a viewer can watch but not drive the guest.
                Message::Binary(b) if !read_only => {
                    TsMessage::Binary(bytes::Bytes::from(b.to_vec()))
                }
                Message::Text(t) if !read_only => TsMessage::Text(t.to_string().into()),
                Message::Close(_) => {
                    let _ = agent_sink.send(TsMessage::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if agent_sink.send(up).await.is_err() {
                break;
            }
        }
    });

    let a2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = agent_stream.next().await {
            let down = match msg {
                TsMessage::Binary(b) => Message::Binary(b),
                TsMessage::Text(t) => Message::Text(t.to_string().into()),
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

    // Abort the surviving direction when either ends, so a client that closes
    // its tab doesn't leave the guest->client task blocked forever holding the
    // upstream agent VNC/SPICE/serial connection (an fd + libvirt console leak).
    let c2a_abort = c2a.abort_handle();
    let a2c_abort = a2c.abort_handle();
    tokio::select! {
        _ = c2a => { a2c_abort.abort(); },
        _ = a2c => { c2a_abort.abort(); },
    }
}

/// Dial a `wss://` URL on the daemon's own (commonly self-signed) TLS
/// listener — accepting its cert here is no more trusting than the plain
/// HTTP `kubevirt_inventory.rs`'s self-call used to send before it grew the
/// same https-first probe.
async fn dial_daemon_kubevirt_ws(
    url: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Error,
> {
    let connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| {
            tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(e.to_string()))
        })?;
    let (stream, _resp) = tokio_tungstenite::connect_async_tls_with_config(
        url,
        None,
        false,
        Some(tokio_tungstenite::Connector::NativeTls(connector)),
    )
    .await?;
    Ok(stream)
}

/// Relay a KubeVirt VNC/serial console to the daemon's `kubectl proxy`-backed
/// endpoint (`daemon/src/kubevirt_k8s_ws_proxy.rs`) — the same shape as
/// `proxy_to_agent_vnc`/`proxy_to_agent_serial`, just with the daemon in
/// place of an agent, since KubeVirt VMs have no `host_id`/agent of their
/// own. This controller has already validated the browser's ws-token above;
/// it authenticates itself to the daemon with a short-lived platform JWT
/// (the same internal-service mechanism `kubevirt_inventory.rs` already uses
/// for its own daemon calls), which `ws_auth_middleware` accepts as a
/// fallback alongside the daemon's regular local ws-tokens.
async fn proxy_to_daemon_kubevirt(
    browser: WebSocket,
    state: AppState,
    namespace: String,
    name: String,
    tail: &'static str,
    read_only: bool,
) {
    let enc_ns = urlencoding::encode(&namespace);
    let enc_name = urlencoding::encode(&name);
    let base = state.config.daemon_base_url.trim_end_matches('/');
    let host_port = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base);
    let configured_is_https = base.starts_with("https://");

    let token = match crate::jwt::issue_token(
        &state.config.jwt_secret,
        "controller-internal-console",
        "operator",
        60,
        None,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("kubevirt {tail}: failed to mint internal service token: {e:#}");
            let (mut sink, _) = browser.split();
            let _ = sink.close().await;
            return;
        }
    };
    let path = format!("/ws/v1/k8s-kubevirt/{enc_ns}/{enc_name}/{tail}?token={token}");

    // `daemon_base_url` commonly defaults to `http://…` even though this
    // deployment's daemon is TLS-only on that same address (mirrors the
    // exact https-then-http probing kubevirt_inventory.rs already does for
    // its own daemon self-call) — a plain `ws://` handshake against a
    // TLS-only port gets back TLS bytes that the WS upgrade parser can't
    // read as HTTP at all ("invalid HTTP version"), not a clean connection
    // error, so this can't be told apart from a real failure without trying
    // wss first regardless of what's configured.
    let wss_url = format!("wss://{host_port}{path}");
    let daemon_ws = match dial_daemon_kubevirt_ws(&wss_url).await {
        Ok(stream) => stream,
        Err(e) if configured_is_https => {
            tracing::warn!("kubevirt {tail}: daemon relay dial failed: {e}");
            let (mut sink, _) = browser.split();
            let _ = sink.close().await;
            return;
        }
        Err(wss_err) => {
            let ws_url = format!("ws://{host_port}{path}");
            match connect_async(&ws_url).await {
                Ok((stream, _)) => stream,
                Err(ws_err) => {
                    tracing::warn!(
                        "kubevirt {tail}: daemon relay dial failed (wss: {wss_err}; ws: {ws_err})"
                    );
                    let (mut sink, _) = browser.split();
                    let _ = sink.close().await;
                    return;
                }
            }
        }
    };

    let (mut client_sink, mut client_stream) = browser.split();
    let (mut daemon_sink, mut daemon_stream) = daemon_ws.split();

    let c2d = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let up = match msg {
                Message::Binary(b) if !read_only => {
                    TsMessage::Binary(bytes::Bytes::from(b.to_vec()))
                }
                Message::Text(t) if !read_only => TsMessage::Text(t.to_string().into()),
                Message::Close(_) => {
                    let _ = daemon_sink.send(TsMessage::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if daemon_sink.send(up).await.is_err() {
                break;
            }
        }
    });

    let d2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = daemon_stream.next().await {
            let down = match msg {
                TsMessage::Binary(b) => Message::Binary(b),
                TsMessage::Text(t) => Message::Text(t.to_string().into()),
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

    let c2d_abort = c2d.abort_handle();
    let d2c_abort = d2c.abort_handle();
    tokio::select! {
        _ = c2d => { d2c_abort.abort(); }
        _ = d2c => { c2d_abort.abort(); }
    }
}

async fn proxy_to_agent_serial(socket: WebSocket, state: AppState, vm_id: Uuid, read_only: bool) {
    let Some((name, agent_console)) = vm_agent_target(&state, vm_id).await else {
        let (mut sink, _) = socket.split();
        let _ = sink.close().await;
        return;
    };

    let ws_url = format!(
        "ws://{}/ws/serial/{}{}",
        agent_client::normalize_agent_addr(&agent_console),
        name,
        agent_console_token_qs(&agent_console)
    );

    let agent_ws = match connect_async(&ws_url).await {
        Ok((stream, _)) => stream,
        Err(_) => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    let (mut client_sink, mut client_stream) = socket.split();
    let (mut agent_sink, mut agent_stream) = agent_ws.split();

    let c2a = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let up = match msg {
                // Read-only grant: drop client input frames (keyboard/mouse/
                // clipboard) so a viewer can watch but not drive the guest.
                Message::Binary(b) if !read_only => {
                    TsMessage::Binary(bytes::Bytes::from(b.to_vec()))
                }
                Message::Text(t) if !read_only => TsMessage::Text(t.to_string().into()),
                Message::Close(_) => {
                    let _ = agent_sink.send(TsMessage::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if agent_sink.send(up).await.is_err() {
                break;
            }
        }
    });

    let a2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = agent_stream.next().await {
            let down = match msg {
                TsMessage::Binary(b) => Message::Binary(b),
                TsMessage::Text(t) => Message::Text(t.to_string().into()),
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

    // Abort the surviving direction when either ends, so a client that closes
    // its tab doesn't leave the guest->client task blocked forever holding the
    // upstream agent VNC/SPICE/serial connection (an fd + libvirt console leak).
    let c2a_abort = c2a.abort_handle();
    let a2c_abort = a2c.abort_handle();
    tokio::select! {
        _ = c2a => { a2c_abort.abort(); },
        _ = a2c => { c2a_abort.abort(); },
    }
}

async fn host_agent_addr(pool: &crate::db::DbPool, host_id: Uuid) -> Result<String, ApiError> {
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    Ok(addr)
}

async fn host_console_addr(pool: &crate::db::DbPool, host_id: Uuid) -> Result<String, ApiError> {
    let addr: String = crate::db::query_scalar(
        "SELECT COALESCE(NULLIF(agent_console_addr, ''), agent_grpc_addr) FROM hosts WHERE id = ?",
    )
    .bind(host_id)
    .fetch_one(pool)
    .await?;
    Ok(addr)
}

pub async fn spice_ws_proxy(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(vm_id): Path<Uuid>,
    Query(q): Query<WsTokenQuery>,
) -> impl IntoResponse {
    let Some(grant) = state
        .ws_tokens
        .validate(&q.token)
        .await
        .filter(|g| g.vm_id == vm_id)
    else {
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    let read_only = grant.read_only;
    ws.on_upgrade(move |socket| proxy_to_agent_spice(socket, state, vm_id, read_only))
}

async fn proxy_to_agent_spice(socket: WebSocket, state: AppState, vm_id: Uuid, read_only: bool) {
    let Some((name, agent_console)) = vm_agent_target(&state, vm_id).await else {
        let (mut sink, _) = socket.split();
        let _ = sink.close().await;
        return;
    };

    let ws_url = format!(
        "ws://{}/ws/spice/{}{}",
        agent_client::normalize_agent_addr(&agent_console),
        name,
        agent_console_token_qs(&agent_console)
    );

    let agent_ws = match connect_async(&ws_url).await {
        Ok((stream, _)) => stream,
        Err(_) => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    let (mut client_sink, mut client_stream) = socket.split();
    let (mut agent_sink, mut agent_stream) = agent_ws.split();

    let c2a = tokio::spawn(async move {
        while let Some(Ok(msg)) = client_stream.next().await {
            let up = match msg {
                // Read-only grant: drop client input frames (keyboard/mouse/
                // clipboard) so a viewer can watch but not drive the guest.
                Message::Binary(b) if !read_only => {
                    TsMessage::Binary(bytes::Bytes::from(b.to_vec()))
                }
                Message::Text(t) if !read_only => TsMessage::Text(t.to_string().into()),
                Message::Close(_) => {
                    let _ = agent_sink.send(TsMessage::Close(None)).await;
                    break;
                }
                _ => continue,
            };
            if agent_sink.send(up).await.is_err() {
                break;
            }
        }
    });

    let a2c = tokio::spawn(async move {
        while let Some(Ok(msg)) = agent_stream.next().await {
            let down = match msg {
                TsMessage::Binary(b) => Message::Binary(b),
                TsMessage::Text(t) => Message::Text(t.to_string().into()),
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

    // Abort the surviving direction when either ends, so a client that closes
    // its tab doesn't leave the guest->client task blocked forever holding the
    // upstream agent VNC/SPICE/serial connection (an fd + libvirt console leak).
    let c2a_abort = c2a.abort_handle();
    let a2c_abort = a2c.abort_handle();
    tokio::select! {
        _ = c2a => { a2c_abort.abort(); },
        _ = a2c => { c2a_abort.abort(); },
    }
}

pub fn ws_routes() -> axum::Router<AppState> {
    use axum::routing::get;
    axum::Router::new()
        .route("/ws/v1/platform/vnc/{vm_id}", get(vnc_ws_proxy))
        .route("/ws/v1/platform/serial/{vm_id}", get(serial_ws_proxy))
        .route("/ws/v1/platform/spice/{vm_id}", get(spice_ws_proxy))
        // Bare-metal serial-over-LAN (IPMI SOL through a pty).
        .route("/ws/v1/baremetal/{id}/sol", get(crate::baremetal_sol::sol_ws))
}
