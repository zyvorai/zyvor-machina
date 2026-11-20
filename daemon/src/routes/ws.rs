// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use machina_core::libvirt::domain;
use machina_core::{LibvirtManager, SshTerminalConfig, VmInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{interval, Duration};
use tracing::{info, warn};

/// Decode a raw `read()` chunk to text for `Message::Text` framing, carrying a
/// trailing incomplete multi-byte UTF-8 sequence over into the next chunk
/// instead of lossy-replacing it immediately.
///
/// Each chunk was previously converted independently via
/// `String::from_utf8_lossy`, so any multi-byte character that happened to
/// straddle a `read()` buffer boundary (routine with box-drawing glyphs,
/// non-ASCII locale output, etc.) turned into `�` on both halves even though
/// the bytes were valid all along. `pending` holds bytes left over from the
/// previous call; a genuinely incomplete lead sequence is at most 3 bytes, so
/// this cannot grow unbounded even against adversarial input — anything
/// longer is treated as truly invalid and lossy-decoded immediately rather
/// than held forever.
fn decode_utf8_chunk(pending: &mut Vec<u8>, buf: &[u8]) -> String {
    let data: Vec<u8> = if pending.is_empty() {
        buf.to_vec()
    } else {
        let mut d = std::mem::take(pending);
        d.extend_from_slice(buf);
        d
    };

    match std::str::from_utf8(&data) {
        Ok(text) => text.to_string(),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            let text = String::from_utf8_lossy(&data[..valid_up_to]).to_string();
            let tail = &data[valid_up_to..];
            match e.error_len() {
                // `None` means `tail` is a valid-so-far, still-incomplete lead
                // sequence (at most 3 bytes) — hold it for the next chunk.
                None if tail.len() <= 3 => {
                    *pending = tail.to_vec();
                    text
                }
                // Otherwise the tail bytes are genuinely invalid UTF-8, not
                // just incomplete: decode them lossily now and move on.
                _ => format!("{text}{}", String::from_utf8_lossy(tail)),
            }
        }
    }
}

fn vm_watch_key(vm: &VmInfo) -> String {
    match &vm.libvirt_connection {
        Some(c) => format!("{c}/{}", vm.name),
        None => vm.name.clone(),
    }
}

use crate::auth::RequestActor;
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::kubevirt_k8s_ws_proxy;
use crate::terminal::{run_ssh_terminal, TerminalSessionStore};

// ── KubeVirt (cluster) VNC / serial console via kubectl proxy ───────

async fn kubevirt_vnc_ws_handler(
    ws: WebSocketUpgrade,
    Path((namespace, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if !kubevirt_k8s_ws_proxy::validate_k8s_name(&namespace)
        || !kubevirt_k8s_ws_proxy::validate_k8s_name(&name)
    {
        return (StatusCode::BAD_REQUEST, "invalid namespace or VM name").into_response();
    }
    // See vnc_handler below — novnc-core requests the 'binary' subprotocol and
    // Chrome fails the connection (code 1006) unless the server echoes it back.
    ws.protocols(["binary"]).on_upgrade(move |socket| {
        kubevirt_k8s_ws_proxy::proxy_kubevirt_ws(socket, namespace, name, "vnc")
    })
}

async fn kubevirt_console_ws_handler(
    ws: WebSocketUpgrade,
    Path((namespace, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if !kubevirt_k8s_ws_proxy::validate_k8s_name(&namespace)
        || !kubevirt_k8s_ws_proxy::validate_k8s_name(&name)
    {
        return (StatusCode::BAD_REQUEST, "invalid namespace or VM name").into_response();
    }
    ws.on_upgrade(move |socket| {
        kubevirt_k8s_ws_proxy::proxy_kubevirt_ws(socket, namespace, name, "console")
    })
}

// ── VM state watch WebSocket ────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(manager): State<LibvirtManager>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, manager))
}

async fn handle_socket(mut socket: WebSocket, manager: LibvirtManager) {
    info!("WebSocket client connected");

    let mut tick = interval(Duration::from_secs(2));
    let mut prev_states: HashMap<String, String> = HashMap::new();

    loop {
        tick.tick().await;

        // Never call libvirt from the async runtime thread: list_vms can block for a long time
        // (e.g. while another thread holds the connection mutex during destroy/undefine). Blocking
        // the executor starves HTTP/WebSocket work and can look like a daemon "crash".
        let manager2 = manager.clone();
        let current = match tokio::task::spawn_blocking(move || manager2.list_all_vms()).await {
            Ok(Ok(vms)) => vms,
            Ok(Err(e)) => {
                warn!("Failed to list VMs for watch: {}", e);
                Vec::new()
            }
            Err(e) => {
                warn!("Watch list_vms task join error: {}", e);
                Vec::new()
            }
        };

        let mut changes = Vec::new();
        let mut current_names: HashMap<String, String> = HashMap::with_capacity(current.len());

        for vm in &current {
            let key = vm_watch_key(vm);
            match prev_states.get(&key) {
                Some(old_state) if *old_state != vm.state => {
                    changes.push(serde_json::json!({
                        "event": "state_change",
                        "name": vm.name,
                        "libvirt_connection": vm.libvirt_connection,
                        "old_state": old_state,
                        "new_state": vm.state,
                    }));
                }
                None => {
                    changes.push(serde_json::json!({
                        "event": "vm_added",
                        "name": vm.name,
                        "libvirt_connection": vm.libvirt_connection,
                        "state": vm.state,
                    }));
                }
                _ => {}
            }
            current_names.insert(key, vm.state.clone());
        }

        for name in prev_states.keys() {
            if !current_names.contains_key(name) {
                changes.push(serde_json::json!({
                    "event": "vm_removed",
                    "name": name,
                }));
            }
        }

        prev_states = current_names;

        let msg = if changes.is_empty() {
            serde_json::json!({ "event": "heartbeat", "vm_count": current.len() })
        } else {
            serde_json::json!({ "event": "changes", "changes": changes })
        };

        if socket
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            info!("WebSocket client disconnected");
            break;
        }
    }
}

// ── Serial console WebSocket ────────────────────────────────────────

async fn console_handler(
    ws: WebSocketUpgrade,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> axum::response::Response {
    if conn_q.is_fluxvm() {
        let upstream = match super::fluxvm::client() {
            Ok(c) => match c.resolve(&name).await {
                Ok(rec) => Ok(c.serial_ws(&rec.id)),
                Err(e) => Err(e.to_string()),
            },
            Err(_) => Err("FluxVM backend is disabled ([fluxvm] enabled = false)".to_string()),
        };
        return ws
            .on_upgrade(move |socket| handle_fluxvm_serial(socket, name, upstream))
            .into_response();
    }
    let name_xml = name.clone();
    let pty_path = match spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let xml = domain::get_vm_xml(conn, &name_xml)?;
        let path = machina_core::xml::extract_attr(&xml, "console", "tty")
            .filter(|s| !s.is_empty())
            .or_else(|| {
                for block in machina_core::xml::split_blocks(&xml, "console") {
                    if let Some(p) = machina_core::xml::extract_attr(&block, "source", "path") {
                        if !p.is_empty() {
                            return Some(p);
                        }
                    }
                }
                None
            });
        Ok(path)
    })
    .await
    {
        Ok(path) => path,
        Err(_) => {
            warn!("Failed to get console info for VM '{}'", name);
            None
        }
    };

    ws.on_upgrade(move |socket| handle_console(socket, name, pty_path))
        .into_response()
}

/// Bridge the browser console socket to `fluxvm-api`'s `/v1/vms/{id}/serial`.
/// FluxVM sends raw serial bytes as binary frames; the console UI expects text.
async fn handle_fluxvm_serial(
    socket: WebSocket,
    name: String,
    upstream: Result<(String, Option<String>), String>,
) {
    use tokio_tungstenite::tungstenite::Message as UpMsg;

    let (mut ws_sink, mut ws_stream) = socket.split();
    let (url, token) = match upstream {
        Ok(u) => u,
        Err(e) => {
            let _ = ws_sink
                .send(Message::Text(
                    format!("\r\nFluxVM serial for '{name}': {e}\r\n").into(),
                ))
                .await;
            return;
        }
    };
    let upstream = match super::fluxvm::connect_ws(&url, token.as_deref()).await {
        Ok(s) => s,
        Err(e) => {
            warn!("FluxVM serial connect failed for '{}': {}", name, e);
            let _ = ws_sink
                .send(Message::Text(
                    format!("\r\nFluxVM serial for '{name}' unavailable: {e}\r\n").into(),
                ))
                .await;
            return;
        }
    };
    info!("FluxVM serial WebSocket connected for VM '{}'", name);
    let (mut up_sink, mut up_stream) = upstream.split();

    let mut down = tokio::spawn(async move {
        let mut pending = Vec::new();
        while let Some(Ok(msg)) = up_stream.next().await {
            let text = match msg {
                UpMsg::Binary(b) => decode_utf8_chunk(&mut pending, &b),
                UpMsg::Text(t) => t.as_str().to_string(),
                UpMsg::Close(_) => break,
                _ => continue,
            };
            if !text.is_empty() && ws_sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let mut up = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let out = match msg {
                Message::Text(t) => UpMsg::Binary(t.as_bytes().to_vec().into()),
                Message::Binary(b) => UpMsg::Binary(b.to_vec().into()),
                Message::Close(_) => break,
                _ => continue,
            };
            if up_sink.send(out).await.is_err() {
                break;
            }
        }
        let _ = up_sink.send(UpMsg::Close(None)).await;
    });
    tokio::select! {
        _ = &mut down => up.abort(),
        _ = &mut up => down.abort(),
    }
}

#[derive(serde::Deserialize)]
struct AgentConsoleQuery {
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
}
fn default_cols() -> u16 {
    120
}
fn default_rows() -> u16 {
    32
}

/// `/ws/v1/fluxvm-console/{name}`: interactive shell through the FluxVM guest
/// agent (every engine, agent enabled). Binary frames are terminal data both
/// ways; a text frame from the browser is a resize `{"cols":…,"rows":…}`.
async fn fluxvm_agent_console_handler(
    ws: WebSocketUpgrade,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(q): Query<AgentConsoleQuery>,
) -> axum::response::Response {
    if let Err(e) = crate::auth::require_write(&actor, "vms:write") {
        return e.into_response();
    }
    let upstream = match super::fluxvm::client() {
        Ok(c) => match c.resolve(&name).await {
            Ok(rec) => Ok(c.console_ws(&rec.id, q.cols, q.rows)),
            Err(e) => Err(e.to_string()),
        },
        Err(_) => Err("FluxVM backend is disabled ([fluxvm] enabled = false)".to_string()),
    };
    ws.on_upgrade(move |socket| handle_fluxvm_agent_console(socket, name, upstream))
        .into_response()
}

async fn handle_fluxvm_agent_console(
    socket: WebSocket,
    name: String,
    upstream: Result<(String, Option<String>), String>,
) {
    use tokio_tungstenite::tungstenite::Message as UpMsg;

    let (mut ws_sink, mut ws_stream) = socket.split();
    let fail = |msg: String| Message::Binary(format!("\r\n{msg}\r\n").into_bytes().into());
    let upstream = match upstream {
        Ok((url, token)) => super::fluxvm::connect_ws(&url, token.as_deref()).await,
        Err(e) => Err(e),
    };
    let upstream = match upstream {
        Ok(s) => s,
        Err(e) => {
            warn!("FluxVM agent console for '{}' failed: {}", name, e);
            let _ = ws_sink
                .send(fail(format!(
                    "FluxVM agent console for '{name}' unavailable: {e} (is the guest agent enabled and up?)"
                )))
                .await;
            return;
        }
    };
    info!("FluxVM agent console connected for VM '{}'", name);
    let (mut up_sink, mut up_stream) = upstream.split();
    let mut down = tokio::spawn(async move {
        while let Some(Ok(msg)) = up_stream.next().await {
            let out = match msg {
                UpMsg::Binary(b) => Message::Binary(b.to_vec().into()),
                UpMsg::Text(t) => Message::Binary(t.as_bytes().to_vec().into()),
                UpMsg::Close(_) => break,
                _ => continue,
            };
            if ws_sink.send(out).await.is_err() {
                break;
            }
        }
    });
    let mut up = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let out = match msg {
                Message::Text(t) => UpMsg::Text(t.as_str().to_string().into()),
                Message::Binary(b) => UpMsg::Binary(b.to_vec().into()),
                Message::Close(_) => break,
                _ => continue,
            };
            if up_sink.send(out).await.is_err() {
                break;
            }
        }
        let _ = up_sink.send(UpMsg::Close(None)).await;
    });
    tokio::select! {
        _ = &mut down => up.abort(),
        _ = &mut up => down.abort(),
    }
}

async fn handle_console(socket: WebSocket, name: String, pty_path: Option<String>) {
    info!("Console WebSocket connected for VM '{}'", name);

    let pty = match pty_path {
        Some(ref p) => match std::fs::canonicalize(p) {
            Ok(canonical) if canonical.starts_with("/dev/pts/") => {
                canonical.to_string_lossy().to_string()
            }
            Ok(canonical) => {
                warn!(
                    "PTY path '{}' resolved to '{}' which is outside /dev/pts/",
                    p,
                    canonical.display()
                );
                let (mut sink, _) = socket.split();
                let _ = sink
                    .send(Message::Text(
                        format!("\r\nInvalid PTY path for VM '{}'\r\n", name).into(),
                    ))
                    .await;
                return;
            }
            Err(_) => {
                let (mut sink, _) = socket.split();
                let _ = sink
                    .send(Message::Text(
                        format!(
                            "\r\nNo serial/console PTY for VM '{}' (domain has no <serial>/<console> pty, or it is shut off).\r\n",
                            name
                        )
                        .into(),
                    ))
                    .await;
                return;
            }
        },
        _ => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!(
                        "\r\nNo serial/console PTY for VM '{}' (domain has no <serial>/<console> pty, or it is shut off).\r\n",
                        name
                    )
                    .into(),
                ))
                .await;
            return;
        }
    };

    // CRITICAL: Open PTY directly with tokio — do NOT use socat or any external process.
    // socat's OPEN: mode fails on PTY devices (/dev/pts/N). Direct async file I/O is the
    // only approach that works reliably for bidirectional serial console over WebSocket.
    // Do NOT change this to socat, Command::new, or any other process-based approach.
    // O_NOCTTY: without it, opening a PTY as a session leader without a controlling
    // terminal makes the PTY the daemon's controlling terminal, and closing it (the
    // WebSocket going away) then delivers SIGHUP to the whole daemon.
    let pty_file = match tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(&pty)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to open PTY {}: {}", pty, e);
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!("\r\nFailed to open console PTY: {}\r\n", e).into(),
                ))
                .await;
            return;
        }
    };

    let (mut pty_read, mut pty_write) = tokio::io::split(pty_file);
    let (mut ws_sink, mut ws_stream) = socket.split();

    // PTY → WebSocket
    let mut read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match pty_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = decode_utf8_chunk(&mut pending, &buf[..n]);
                    if text.is_empty() {
                        continue;
                    }
                    if ws_sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // WebSocket → PTY
    let console_name = name.clone();
    let mut write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Text(text) => {
                    if pty_write.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Binary(data) => {
                    if pty_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Ping(_) | Message::Pong(_) => { /* axum handles pong automatically */ }
                Message::Close(_) => break,
            }
        }
    });

    tokio::select! {
        _ = &mut read_task => { write_task.abort(); }
        _ = &mut write_task => { read_task.abort(); }
    }

    info!("Console WebSocket closed for VM '{}'", console_name);
}

// ── VNC WebSocket proxy ─────────────────────────────────────────────

async fn vnc_handler(
    ws: WebSocketUpgrade,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> impl IntoResponse {
    // FluxVM QEMU: mint a ws-ticket (kind=vnc) and bridge binary RFB frames to
    // fluxvm-api's `/console/ws/{ticket}` (same shape noVNC expects).
    if conn_q.is_fluxvm() {
        let upstream = match super::fluxvm::client() {
            Ok(c) => match c.ws_ticket(&name, "vnc").await {
                Ok(path) => Ok(c.ticket_ws_url(&path)),
                Err(e) => Err(e.to_string()),
            },
            Err(_) => Err("FluxVM backend is disabled ([fluxvm] enabled = false)".to_string()),
        };
        return ws
            .protocols(["binary"])
            .on_upgrade(move |socket| handle_fluxvm_vnc(socket, name, upstream))
            .into_response();
    }

    let name2 = name.clone();
    let resolved = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        machina_core::libvirt::vnc::resolve_vnc_tcp(conn, &name2)
    })
    .await;

    let (host, port) = match resolved {
        Ok((h, p)) if p > 0 => (h, p),
        Ok(_) => {
            return (StatusCode::NOT_FOUND, "No VNC display for this VM").into_response();
        }
        Err(_) => {
            warn!("VNC resolve failed for VM '{}'", name);
            return (StatusCode::NOT_FOUND, "No VNC display for this VM").into_response();
        }
    };

    // novnc-core (the bundled VNC client) opens the socket with
    // `new WebSocket(url, ['binary'])`. Per RFC 6455 §4.1, if the client offers
    // subprotocols and the server's response doesn't select one of them, the
    // client MUST fail the connection — Chrome does this immediately (code
    // 1006) even though the raw HTTP upgrade itself succeeds. Echoing the
    // offered protocol back satisfies that requirement.
    ws.protocols(["binary"])
        .on_upgrade(move |socket| handle_vnc_proxy(socket, name, host, port))
}

/// Bridge noVNC's binary WebSocket to FluxVM's ticket-redeemed VNC socket.
async fn handle_fluxvm_vnc(
    socket: WebSocket,
    name: String,
    upstream: Result<String, String>,
) {
    use tokio_tungstenite::tungstenite::Message as UpMsg;

    let (mut ws_sink, mut ws_stream) = socket.split();
    let url = match upstream {
        Ok(u) => u,
        Err(e) => {
            warn!("FluxVM VNC for '{}': {}", name, e);
            let _ = ws_sink.close().await;
            return;
        }
    };
    // Ticket path needs no Authorization (the ticket is the credential).
    let upstream = match super::fluxvm::connect_ws(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            warn!("FluxVM VNC connect failed for '{}': {}", name, e);
            let _ = ws_sink.close().await;
            return;
        }
    };
    info!("FluxVM VNC WebSocket connected for VM '{}'", name);
    let (mut up_sink, mut up_stream) = upstream.split();

    let mut down = tokio::spawn(async move {
        while let Some(Ok(msg)) = up_stream.next().await {
            let out = match msg {
                UpMsg::Binary(b) => Message::Binary(b.to_vec().into()),
                UpMsg::Text(t) => Message::Binary(t.as_bytes().to_vec().into()),
                UpMsg::Close(_) => break,
                _ => continue,
            };
            if ws_sink.send(out).await.is_err() {
                break;
            }
        }
    });
    let mut up = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let out = match msg {
                Message::Binary(b) => UpMsg::Binary(b.to_vec().into()),
                Message::Text(t) => UpMsg::Binary(t.as_bytes().to_vec().into()),
                Message::Close(_) => break,
                _ => continue,
            };
            if up_sink.send(out).await.is_err() {
                break;
            }
        }
        let _ = up_sink.send(UpMsg::Close(None)).await;
    });
    tokio::select! {
        _ = &mut down => up.abort(),
        _ = &mut up => down.abort(),
    }
    info!("FluxVM VNC WebSocket closed for VM '{}'", name);
}

async fn handle_vnc_proxy(socket: WebSocket, name: String, host: String, port: u16) {
    info!(
        "VNC WebSocket proxy connecting to {}:{} for VM '{}'",
        host, port, name
    );

    let tcp = match tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to connect to VNC port {}: {}", port, e);
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    if let Err(e) = tcp.set_nodelay(true) {
        warn!(
            "VNC TCP set_nodelay failed for {}:{} VM '{}': {}",
            host, port, name, e
        );
    }

    info!("VNC TCP connected to {}:{} for VM '{}'", host, port, name);

    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut ws_sink, mut ws_stream) = socket.split();

    // TCP → WebSocket (binary frames)
    let mut read_task = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match tcp_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if ws_sink
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // WebSocket → TCP
    let mut write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    if tcp_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    if tcp_write.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Ping(_) | Message::Pong(_) => { /* axum handles pong automatically */ }
                Message::Close(_) => break,
            }
        }
    });

    tokio::select! {
        _ = &mut read_task => { write_task.abort(); }
        _ = &mut write_task => { read_task.abort(); }
    }

    info!("VNC WebSocket proxy closed for VM '{}' port {}", name, port);
}

// ── RDP WebSocket proxy (TCP 3389, binary frames — use with built-in client or external) ──

async fn rdp_handler(
    ws: WebSocketUpgrade,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> impl IntoResponse {
    let name2 = name.clone();
    let resolved = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        machina_core::libvirt::rdp::resolve_rdp_endpoint(conn, &name2)
    })
    .await;

    let (host, port) = match resolved {
        Ok((h, p)) if p > 0 => (h, p),
        Ok(_) => {
            return (StatusCode::NOT_FOUND, "No RDP endpoint for this VM").into_response();
        }
        Err(_) => {
            warn!("RDP resolve failed for VM '{}'", name);
            return (StatusCode::NOT_FOUND, "No RDP endpoint for this VM").into_response();
        }
    };

    ws.on_upgrade(move |socket| handle_rdp_proxy(socket, name, host, port))
}

async fn handle_rdp_proxy(socket: WebSocket, name: String, host: String, port: u16) {
    info!(
        "RDP WebSocket proxy connecting to {}:{} for VM '{}'",
        host, port, name
    );

    let tcp = match tokio::net::TcpStream::connect(format!("{}:{}", host, port)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to connect to RDP {}:{}: {}", host, port, e);
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    if let Err(e) = tcp.set_nodelay(true) {
        warn!(
            "RDP TCP set_nodelay failed for {}:{} VM '{}': {}",
            host, port, name, e
        );
    }

    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut ws_sink, mut ws_stream) = socket.split();

    let mut read_task = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match tcp_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if ws_sink
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    if tcp_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    if tcp_write.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => break,
            }
        }
    });

    tokio::select! {
        _ = &mut read_task => { write_task.abort(); }
        _ = &mut write_task => { read_task.abort(); }
    }

    info!("RDP WebSocket proxy closed for VM '{}' port {}", name, port);
}

// ── SPICE WebSocket proxy ──────────────────────────────────────────

async fn spice_handler(
    ws: WebSocketUpgrade,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    State(manager): State<LibvirtManager>,
) -> impl IntoResponse {
    let name2 = name.clone();
    let port = match spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let xml = domain::get_vm_xml(conn, &name2)?;
        let mut port = 0u16;
        for block in machina_core::xml::split_blocks(&xml, "graphics") {
            let gtype =
                machina_core::xml::extract_attr(&block, "graphics", "type").unwrap_or_default();
            if gtype == "spice" {
                port = machina_core::xml::extract_attr(&block, "graphics", "port")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                break;
            }
        }
        Ok(port)
    })
    .await
    {
        Ok(p) => p,
        Err(_) => {
            warn!("SPICE port lookup failed for VM '{}'", name);
            0u16
        }
    };

    ws.on_upgrade(move |socket| handle_spice_proxy(socket, name, port))
}

async fn handle_spice_proxy(socket: WebSocket, name: String, port: u16) {
    if port == 0 {
        info!("SPICE: no port for VM '{}'", name);
        let (mut sink, _) = socket.split();
        let _ = sink.close().await;
        return;
    }

    info!(
        "SPICE WebSocket proxy connecting to 127.0.0.1:{} for VM '{}'",
        port, name
    );

    let tcp = match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to connect to SPICE port {}: {}", port, e);
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    if let Err(e) = tcp.set_nodelay(true) {
        warn!(
            "SPICE TCP set_nodelay failed for port {} VM '{}': {}",
            port, name, e
        );
    }

    info!("SPICE TCP connected to port {} for VM '{}'", port, name);

    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut ws_sink, mut ws_stream) = socket.split();

    let mut read_task = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match tcp_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if ws_sink
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    if tcp_write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    if tcp_write.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => break,
            }
        }
    });

    tokio::select! {
        _ = &mut read_task => { write_task.abort(); }
        _ = &mut write_task => { read_task.abort(); }
    }

    info!(
        "SPICE WebSocket proxy closed for VM '{}' port {}",
        name, port
    );
}

// ── SSH WebSocket proxy (legacy raw I/O) ───────────────────────────

async fn ssh_handler(
    ws: WebSocketUpgrade,
    Path(host): Path<String>,
    Extension(cfg): Extension<SshTerminalConfig>,
) -> impl IntoResponse {
    if !cfg.legacy_plain_host_websocket {
        return (
            StatusCode::NOT_FOUND,
            "Legacy SSH WebSocket disabled. Use POST /api/v1/terminal/sessions then wss://…/ws/v1/terminal/{session_id}?token=…",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| handle_ssh_proxy(socket, host))
}

// ── SSH terminal (session id + JSON control, PTY + ssh) ─────────────

async fn invalid_terminal_session(socket: WebSocket) {
    let (mut tx, _) = socket.split();
    let _ = tx
        .send(Message::Text(
            r#"{"type":"error","message":"invalid or expired session"}"#.into(),
        ))
        .await;
}

async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    Extension(store): Extension<TerminalSessionStore>,
    Extension(cfg): Extension<SshTerminalConfig>,
) -> impl IntoResponse {
    let ttl = std::time::Duration::from_secs(cfg.session_ttl_secs.clamp(30, 3600));
    match store.take(&session_id, ttl) {
        Some(session) => ws.on_upgrade(move |socket| run_ssh_terminal(socket, session)),
        None => ws.on_upgrade(invalid_terminal_session),
    }
}

async fn handle_ssh_proxy(socket: WebSocket, host: String) {
    // Validate host with a strict allowlist: a hostname or IPv4/IPv6 literal only.
    // This string is interpolated into the `script -qfc "ssh … {host}"` shell command
    // below, so a blocklist is not safe — backtick/$()/newline etc. must be excluded.
    // A leading '-' is also barred so it can't become an ssh flag.
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'));
    if !host_ok {
        info!("SSH: invalid host '{}'", host);
        let (mut sink, _) = socket.split();
        let _ = sink.close().await;
        return;
    }

    info!("SSH WebSocket spawning ssh process to {}", host);

    // Spawn ssh via 'script' to allocate a PTY (ssh requires a PTY for interactive login)
    // script -qfc "ssh ..." /dev/null allocates a PTY and runs the command
    let mut child = match tokio::process::Command::new("script")
        .args([
            "-qfc",
            &format!("ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR {host}"),
            "/dev/null",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // Discard stderr rather than pipe it: nothing takes/drains child.stderr, so
        // a piped stderr's ~64 KiB kernel buffer could fill (chatty ssh
        // diagnostics) and block the child mid-session.
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to spawn ssh to {}: {}", host, e);
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!("\r\nFailed to start SSH to {}: {}\r\n", host, e).into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text("\r\nFailed to get SSH stdin\r\n".into()))
                .await;
            return;
        }
    };

    let mut stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text("\r\nFailed to get SSH stdout\r\n".into()))
                .await;
            return;
        }
    };

    let (mut ws_sink, mut ws_stream) = socket.split();

    // SSH stdout → WebSocket
    let mut read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = decode_utf8_chunk(&mut pending, &buf[..n]);
                    if text.is_empty() {
                        continue;
                    }
                    if ws_sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // WebSocket → SSH stdin
    let ssh_host = host.clone();
    let mut write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Text(text) => {
                    if stdin.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Binary(data) => {
                    if stdin.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Close(_) => break,
            }
        }
    });

    tokio::select! {
        _ = &mut read_task => { write_task.abort(); }
        _ = &mut write_task => { read_task.abort(); }
    }

    // Kill ssh process
    let _ = child.kill().await;

    info!("SSH WebSocket closed for host '{}'", ssh_host);
}

// ── Routes ──────────────────────────────────────────────────────────

pub fn ws_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/watch", get(ws_handler))
        .route(
            "/k8s-kubevirt/{namespace}/{name}/vnc",
            get(kubevirt_vnc_ws_handler),
        )
        .route(
            "/k8s-kubevirt/{namespace}/{name}/console",
            get(kubevirt_console_ws_handler),
        )
        .route("/console/{name}", get(console_handler))
        .route("/fluxvm-console/{name}", get(fluxvm_agent_console_handler))
        .route("/vnc/{name}", get(vnc_handler))
        .route("/rdp/{name}", get(rdp_handler))
        .route("/spice/{name}", get(spice_handler))
        .route("/terminal/{session_id}", get(terminal_ws_handler))
        .route("/ssh/{host}", get(ssh_handler))
}
