// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! WebSocket proxy from the machina UI to KubeVirt VNC / serial console subresources.
//! Spawns a short-lived `kubectl proxy` on localhost (uses kubeconfig auth) and dials the
//! upstream `ws://127.0.0.1:…/apis/subresources.kubevirt.io/…/virtualmachineinstances/…/{vnc,console}`.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::protocol::Message as UpMsg;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

/// Safe Kubernetes resource name segment (same rules as `daemon/src/routes/k8s.rs`).
pub fn validate_k8s_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

struct KubectlProxy {
    child: tokio::process::Child,
    port: u16,
}

impl Drop for KubectlProxy {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.try_wait();
    }
}

/// `kubectl proxy` only takes a `--port` number — it binds the port itself, so
/// there is no way to hand it an already-bound file descriptor (the only real
/// fix for the underlying bind-then-drop TOCTOU gap below). Instead, retry a
/// bounded number of times with a freshly chosen ephemeral port whenever the
/// spawned `kubectl proxy` fails to bind, so a one-off collision (another
/// concurrent console request, or any other local process) produces a working
/// proxy on the next attempt instead of one opaque failure to the caller.
const KUBECTL_PROXY_BIND_RETRIES: u32 = 3;

async fn spawn_kubectl_proxy() -> Result<KubectlProxy, String> {
    let mut last_err = String::new();
    for attempt in 1..=KUBECTL_PROXY_BIND_RETRIES {
        match try_spawn_kubectl_proxy_once().await {
            Ok(p) => return Ok(p),
            Err(e) => {
                debug!(
                    "kubectl proxy spawn attempt {attempt}/{KUBECTL_PROXY_BIND_RETRIES} failed: {e}"
                );
                last_err = e;
            }
        }
    }
    Err(format!(
        "kubectl proxy failed to start after {KUBECTL_PROXY_BIND_RETRIES} attempts: {last_err}"
    ))
}

/// Bind an ephemeral port, drop the listener, and spawn `kubectl proxy` on it.
///
/// The bind-then-drop-then-spawn sequence is an inherent TOCTOU: the port is
/// free at the `.local_addr()` check but nothing prevents another process
/// from claiming it before `kubectl proxy` gets to `bind()`. When that
/// happens `kubectl proxy` exits immediately, which the poll loop below
/// detects via `child.try_wait()` and reports as an error — the caller
/// (`spawn_kubectl_proxy`) retries with a new port rather than surfacing that
/// directly.
async fn try_spawn_kubectl_proxy_once() -> Result<KubectlProxy, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("bind ephemeral port: {e}"))?
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();

    let choice = crate::k8s_kubeconfig::kubectl_kubeconfig_choice().await;
    let mut proxy_args = choice.prefix.clone();
    proxy_args.extend([
        "proxy".into(),
        "--address=127.0.0.1".into(),
        format!("--port={port}"),
        "--disable-filter".into(),
    ]);
    let mut child = tokio::process::Command::new("kubectl")
        .args(&proxy_args)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("kubectl proxy spawn: {e}"))?;

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                debug!(target: "kubectl_proxy", "{}", line.trim_end());
                line.clear();
            }
        });
    }

    for _ in 0..60 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            sleep(Duration::from_millis(80)).await;
            return Ok(KubectlProxy { child, port });
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                return Err(format!("kubectl proxy exited before listen (status={st})"));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("kubectl proxy try_wait: {e}")),
        }
        sleep(Duration::from_millis(100)).await;
    }

    let _ = child.start_kill();
    Err("kubectl proxy did not accept connections in time".into())
}

type UpWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn dial_kubevirt_upstream(
    port: u16,
    namespace: &str,
    name: &str,
    tail: &str,
) -> Result<UpWs, String> {
    for ver in ["v1", "v1alpha3", "v1beta1"] {
        let url = format!(
            "ws://127.0.0.1:{port}/apis/subresources.kubevirt.io/{ver}/namespaces/{namespace}/virtualmachineinstances/{name}/{tail}"
        );
        let uri: http::Uri = match url.parse() {
            Ok(u) => u,
            Err(e) => return Err(format!("invalid ws url: {e}")),
        };
        match connect_async(uri).await {
            Ok((ws, _resp)) => {
                debug!("kubevirt {tail} upstream connected (api={ver})");
                return Ok(ws);
            }
            Err(e) => {
                debug!("kubevirt {tail} dial {ver} failed: {e}");
            }
        }
    }
    Err(format!(
        "could not open KubeVirt {tail} WebSocket for {namespace}/{name} (VMI running? API version?)"
    ))
}

fn axum_to_upstream(msg: Message) -> Option<UpMsg> {
    match msg {
        Message::Text(t) => Some(UpMsg::Text(t.as_str().into())),
        Message::Binary(b) => Some(UpMsg::Binary(b)),
        Message::Ping(p) => Some(UpMsg::Ping(p)),
        Message::Pong(p) => Some(UpMsg::Pong(p)),
        Message::Close(_) => None,
    }
}

fn upstream_to_axum(msg: UpMsg) -> Option<Message> {
    match msg {
        UpMsg::Text(t) => Some(Message::Text(t.as_str().into())),
        UpMsg::Binary(b) => Some(Message::Binary(b)),
        UpMsg::Ping(p) => Some(Message::Ping(p)),
        UpMsg::Pong(p) => Some(Message::Pong(p)),
        UpMsg::Close(_) => None,
        UpMsg::Frame(_) => None,
    }
}

pub async fn proxy_kubevirt_ws(
    mut browser: WebSocket,
    namespace: String,
    name: String,
    tail: &'static str,
) {
    info!("KubeVirt {tail} WS: starting kubectl proxy for namespace={namespace} name={name}");

    let proxy = match spawn_kubectl_proxy().await {
        Ok(p) => p,
        Err(e) => {
            warn!("kubevirt {tail}: kubectl proxy failed: {e}");
            let _ = browser.close().await;
            return;
        }
    };

    let upstream = match dial_kubevirt_upstream(proxy.port, &namespace, &name, tail).await {
        Ok(u) => u,
        Err(e) => {
            warn!("kubevirt {tail}: upstream: {e}");
            let _ = browser.close().await;
            return;
        }
    };

    info!("KubeVirt {tail} WS: bidirectional proxy up for {namespace}/{name}");

    let (mut br_send, mut br_recv) = browser.split();
    let (mut up_send, mut up_recv) = upstream.split();

    let up_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = br_recv.next().await {
            if matches!(msg, Message::Close(_)) {
                let _ = up_send.send(UpMsg::Close(None)).await;
                break;
            }
            let Some(out) = axum_to_upstream(msg) else {
                continue;
            };
            if up_send.send(out).await.is_err() {
                break;
            }
        }
    });

    let down_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = up_recv.next().await {
            if matches!(msg, UpMsg::Frame(_)) {
                continue;
            }
            if matches!(msg, UpMsg::Close(_)) {
                let _ = br_send.send(Message::Close(None)).await;
                break;
            }
            let Some(out) = upstream_to_axum(msg) else {
                continue;
            };
            if br_send.send(out).await.is_err() {
                break;
            }
        }
    });

    // Abort the surviving direction when either ends. join! waited for BOTH, so a
    // browser tab closing on an idle console left down_task parked on
    // up_recv.next() forever — leaking the upstream WS, both tasks, AND the
    // KubectlProxy child (its Drop/start_kill runs only after this returns).
    // Mirrors relay_platform_ws.
    let up_abort = up_task.abort_handle();
    let down_abort = down_task.abort_handle();
    tokio::select! {
        _ = up_task => { down_abort.abort(); }
        _ = down_task => { up_abort.abort(); }
    }
    drop(proxy);
    info!("KubeVirt {tail} WS closed for {namespace}/{name}");
}
