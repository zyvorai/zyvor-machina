// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::libvirt_ops::LibvirtCtx;

#[derive(Clone)]
pub struct ConsoleProxyState {
    pub libvirt: Arc<Mutex<LibvirtCtx>>,
    /// Shared console token. When non-empty, every console WS
    /// reverse-proxy) requires a matching `?token=` — otherwise any local process on
    /// the agent host could open a VM's serial/VNC/SPICE console (serial = an
    /// interactive root shell) directly, bypassing the controller. Empty = accept
    /// unauthenticated (dev/backward-compat), warned about at startup. Populated from
    /// `MACHINA_AGENT_TOKEN`, the same shared secret the gRPC surface uses.
    pub secret: String,
}

#[derive(serde::Deserialize)]
pub struct ConsoleAuthQuery {
    #[serde(default)]
    pub token: String,
}

/// Constant-time token check. An empty configured secret means "unauthenticated"
/// (dev mode) and is accepted, matching the gRPC surface's behavior.
pub fn console_authorized(secret: &str, provided: &str) -> bool {
    if secret.is_empty() {
        return true;
    }
    if secret.len() != provided.len() {
        return false;
    }
    secret
        .as_bytes()
        .iter()
        .zip(provided.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub fn vnc_router(state: ConsoleProxyState) -> Router {
    Router::new()
        .route("/ws/vnc/{name}", get(vnc_ws))
        .route("/ws/spice/{name}", get(spice_ws))
        .route("/ws/serial/{name}", get(serial_ws))
        .with_state(state)
}

async fn vnc_ws(
    ws: WebSocketUpgrade,
    State(st): State<ConsoleProxyState>,
    Path(name): Path<String>,
    Query(q): Query<ConsoleAuthQuery>,
) -> Response {
    if !console_authorized(&st.secret, &q.token) {
        return (StatusCode::UNAUTHORIZED, "invalid console token").into_response();
    }
    let libvirt = st.libvirt.clone();
    ws.on_upgrade(move |socket| handle_vnc(socket, name, libvirt))
        .into_response()
}

async fn serial_ws(
    ws: WebSocketUpgrade,
    State(st): State<ConsoleProxyState>,
    Path(name): Path<String>,
    Query(q): Query<ConsoleAuthQuery>,
) -> Response {
    if !console_authorized(&st.secret, &q.token) {
        return (StatusCode::UNAUTHORIZED, "invalid console token").into_response();
    }
    let libvirt = st.libvirt.clone();
    ws.on_upgrade(move |socket| handle_serial(socket, name, libvirt))
        .into_response()
}

async fn spice_ws(
    ws: WebSocketUpgrade,
    State(st): State<ConsoleProxyState>,
    Path(name): Path<String>,
    Query(q): Query<ConsoleAuthQuery>,
) -> Response {
    if !console_authorized(&st.secret, &q.token) {
        return (StatusCode::UNAUTHORIZED, "invalid console token").into_response();
    }
    let libvirt = st.libvirt.clone();
    ws.on_upgrade(move |socket| handle_spice(socket, name, libvirt))
        .into_response()
}

/// Cap on how long a console handler waits on the blocking libvirt call that
/// resolves a VM's console endpoint. A wedged libvirtd (or a socket call that
/// blocks forever) previously left the WS handshake — and the client — hanging
/// indefinitely with no error. Note this only bounds the *async* side: the
/// spawned blocking-pool thread itself keeps running until the libvirt call
/// returns, since a synchronous FFI call can't be cancelled from the outside.
const LIBVIRT_RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);

fn resolve_console_pty(xml: &str) -> Option<String> {
    machina_core::xml::extract_attr(xml, "console", "tty")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            for block in machina_core::xml::split_blocks(xml, "console") {
                if let Some(p) = machina_core::xml::extract_attr(&block, "source", "path") {
                    if !p.is_empty() {
                        return Some(p);
                    }
                }
            }
            None
        })
}

async fn handle_vnc(socket: WebSocket, name: String, libvirt: Arc<Mutex<LibvirtCtx>>) {
    let resolved = tokio::time::timeout(
        LIBVIRT_RESOLVE_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let mut ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            ctx.resolve_vnc(&name)
        }),
    )
    .await;

    let (host, port) = match resolved {
        Ok(Ok(Ok((h, p)))) if p > 0 => (h, p),
        _ => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    let tcp = match tokio::net::TcpStream::connect(format!("{host}:{port}")).await {
        Ok(s) => s,
        Err(_) => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };
    let _ = tcp.set_nodelay(true);

    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut ws_sink, mut ws_stream) = socket.split();

    let read_task = tokio::spawn(async move {
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

    let write_task = tokio::spawn(async move {
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
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let read_abort = read_task.abort_handle();
    let write_abort = write_task.abort_handle();
    tokio::select! {
        _ = read_task => { write_abort.abort(); },
        _ = write_task => { read_abort.abort(); },
    }
}

async fn handle_serial(socket: WebSocket, name: String, libvirt: Arc<Mutex<LibvirtCtx>>) {
    let display_name = name.clone();
    let pty_path = tokio::time::timeout(
        LIBVIRT_RESOLVE_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            let xml = ctx.get_domain_xml(&name)?;
            resolve_console_pty(&xml).ok_or_else(|| {
                machina_core::LibvirtError::Operation(format!("no serial PTY for VM '{name}'"))
            })
        }),
    )
    .await;

    let pty_path = match pty_path {
        Ok(Ok(Ok(p))) => p,
        _ => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!("\r\nNo console PTY found for VM '{display_name}'. Is it running?\r\n")
                        .into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    let pty = match std::fs::canonicalize(&pty_path) {
        Ok(canonical) if canonical.starts_with("/dev/pts/") => {
            canonical.to_string_lossy().to_string()
        }
        Ok(canonical) => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!(
                        "\r\nInvalid PTY path '{}' resolved to '{}'\r\n",
                        pty_path,
                        canonical.display()
                    )
                    .into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
        Err(e) => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!("\r\nFailed to open console PTY '{pty_path}': {e}\r\n").into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    let pty_file = match tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pty)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            let (mut sink, _) = socket.split();
            let _ = sink
                .send(Message::Text(
                    format!("\r\nFailed to open console PTY: {e}\r\n").into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    let (mut pty_read, mut pty_write) = tokio::io::split(pty_file);
    // Wake idle getty/login prompts so the first browser frame is not a blank canvas.
    let _ = pty_write.write_all(b"\r").await;

    let (mut ws_sink, mut ws_stream) = socket.split();

    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match pty_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if ws_sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let write_task = tokio::spawn(async move {
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
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let read_abort = read_task.abort_handle();
    let write_abort = write_task.abort_handle();
    tokio::select! {
        _ = read_task => { write_abort.abort(); },
        _ = write_task => { read_abort.abort(); },
    }
}

async fn handle_spice(socket: WebSocket, name: String, libvirt: Arc<Mutex<LibvirtCtx>>) {
    let lookup = tokio::time::timeout(
        LIBVIRT_RESOLVE_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let ctx = libvirt
                .lock()
                .map_err(|e| machina_core::LibvirtError::Internal(e.to_string()))?;
            let xml = ctx.get_domain_xml(&name)?;
            machina_core::libvirt::spice::resolve_spice_endpoint(&name, &xml).ok_or_else(|| {
                machina_core::LibvirtError::Operation(format!("no SPICE endpoint for VM '{name}'"))
            })
        }),
    )
    .await;

    let endpoint = match lookup {
        Ok(Ok(Ok(ep))) => ep,
        _ => {
            let (mut sink, _) = socket.split();
            let _ = sink.close().await;
            return;
        }
    };

    // Bridge to whichever transport the SPICE server actually exposes: a TCP
    // autoport listen or (libvirt's modern default) a local unix socket. The
    // old resolver only understood a numeric TCP port, so socket/TLS-only guests
    // fell through to a closed WebSocket ("Disconnected · SPICE").
    use machina_core::libvirt::spice::SpiceEndpoint;
    match endpoint {
        SpiceEndpoint::Tcp(host, port) => {
            match tokio::net::TcpStream::connect(format!("{host}:{port}")).await {
                Ok(tcp) => {
                    let _ = tcp.set_nodelay(true);
                    bridge_ws_stream(socket, tcp).await;
                }
                Err(_) => {
                    let (mut sink, _) = socket.split();
                    let _ = sink.close().await;
                }
            }
        }
        SpiceEndpoint::Unix(path) => match tokio::net::UnixStream::connect(&path).await {
            Ok(sock) => bridge_ws_stream(socket, sock).await,
            Err(_) => {
                let (mut sink, _) = socket.split();
                let _ = sink.close().await;
            }
        },
    }
}

/// Pump bytes both ways between a client WebSocket and an upstream byte stream
/// (TCP or unix socket). Either side closing aborts the other so we never leak
/// the upstream console connection.
async fn bridge_ws_stream<S>(socket: WebSocket, stream: S)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (mut ws_sink, mut ws_stream) = socket.split();

    let read_task = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match rd.read(&mut buf).await {
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

    let write_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    if wr.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Message::Text(text) => {
                    if wr.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let read_abort = read_task.abort_handle();
    let write_abort = write_task.abort_handle();
    tokio::select! {
        _ = read_task => { write_abort.abort(); },
        _ = write_task => { read_abort.abort(); },
    }
}

pub fn router(state: ConsoleProxyState) -> Router {
    vnc_router(state)
}

pub async fn serve(listen: std::net::SocketAddr, state: ConsoleProxyState) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("machina-agent console proxy on {listen}");
    axum::serve(listener, app).await?;
    Ok(())
}
