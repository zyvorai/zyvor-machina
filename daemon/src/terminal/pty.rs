// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::PendingSession;

type WsTx = SplitSink<WebSocket, Message>;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Ping,
}

enum PtyOut {
    Bytes(Vec<u8>),
    Err(String),
    Exit(i32),
}

async fn send_json_err(ws_tx: &mut WsTx, msg: String) {
    let payload = serde_json::json!({ "type": "error", "message": msg }).to_string();
    let _ = ws_tx.send(Message::Text(payload.into())).await;
}

/// Run PTY-backed OpenSSH until the WebSocket closes or the child exits.
pub async fn run_ssh_terminal(ws: WebSocket, session: PendingSession) {
    let peer = if session.ssh_port != 0 && session.ssh_port != 22 {
        format!("{}@{}:{}", session.ssh_user, session.host, session.ssh_port)
    } else {
        format!("{}@{}", session.ssh_user, session.host)
    };
    info!(
        "SSH terminal WebSocket starting for {} (created by {})",
        peer, session.created_by
    );

    let (mut ws_tx, mut ws_rx) = ws.split();

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            send_json_err(&mut ws_tx, format!("PTY allocation failed: {e}")).await;
            return;
        }
    };

    let mut cmd = CommandBuilder::new("ssh");
    cmd.arg("-o");
    cmd.arg("StrictHostKeyChecking=accept-new");
    cmd.arg("-o");
    cmd.arg("ServerAliveInterval=30");
    cmd.arg("-o");
    cmd.arg("LogLevel=ERROR");
    if session.ssh_port != 0 && session.ssh_port != 22 {
        cmd.arg("-p");
        cmd.arg(session.ssh_port.to_string());
    }
    cmd.arg(format!("{}@{}", session.ssh_user, session.host));

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            send_json_err(&mut ws_tx, format!("ssh spawn failed: {e}")).await;
            return;
        }
    };
    drop(pair.slave);

    let mut killer = child.clone_killer();

    let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));

    let writer_result = {
        let m = master.lock().unwrap_or_else(|e| e.into_inner());
        m.take_writer()
    };
    let writer = match writer_result {
        Ok(w) => Arc::new(Mutex::new(w)),
        Err(e) => {
            send_json_err(&mut ws_tx, format!("PTY writer: {e}")).await;
            let _ = killer.kill();
            return;
        }
    };

    let reader_result = {
        let m = master.lock().unwrap_or_else(|e| e.into_inner());
        m.try_clone_reader()
    };
    let reader = match reader_result {
        Ok(r) => r,
        Err(e) => {
            send_json_err(&mut ws_tx, format!("PTY reader: {e}")).await;
            let _ = killer.kill();
            return;
        }
    };

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<PtyOut>();

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.send(PtyOut::Bytes(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = out_tx.send(PtyOut::Err(format!("PTY read: {e}")));
                    break;
                }
            }
        }
        let final_msg = match child.wait() {
            Ok(st) => PtyOut::Exit(if st.success() {
                0
            } else {
                st.exit_code() as i32
            }),
            Err(e) => PtyOut::Err(format!("ssh wait: {e}")),
        };
        let _ = out_tx.send(final_msg);
    });

    loop {
        tokio::select! {
            out = out_rx.recv() => {
                match out {
                    None => break,
                    Some(PtyOut::Bytes(b)) => {
                        if ws_tx.send(Message::Binary(b.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(PtyOut::Err(msg)) => {
                        let payload = serde_json::json!({ "type": "error", "message": msg }).to_string();
                        let _ = ws_tx.send(Message::Text(payload.into())).await;
                        break;
                    }
                    Some(PtyOut::Exit(code)) => {
                        let payload = serde_json::json!({ "type": "exit", "code": code }).to_string();
                        let _ = ws_tx.send(Message::Text(payload.into())).await;
                        break;
                    }
                }
            }
            ws_msg = ws_rx.next() => {
                match ws_msg {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(cm) = serde_json::from_str::<ClientMsg>(&t) {
                            match cm {
                                ClientMsg::Input { data } => {
                                    let w = writer.clone();
                                    let buf = data.into_bytes();
                                    if tokio::task::spawn_blocking(move || {
                                        if let Ok(mut g) = w.lock() {
                                            let _ = g.write_all(&buf);
                                            let _ = g.flush();
                                        }
                                    })
                                    .await
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                                ClientMsg::Resize { cols, rows } => {
                                    let m = master.clone();
                                    let sz = PtySize {
                                        rows,
                                        cols,
                                        pixel_width: 0,
                                        pixel_height: 0,
                                    };
                                    if tokio::task::spawn_blocking(move || {
                                        if let Ok(ms) = m.lock() {
                                            if let Err(e) = ms.resize(sz) {
                                                warn!("PTY resize: {e}");
                                            }
                                        }
                                    })
                                    .await
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                                ClientMsg::Ping => {
                                    let pong = serde_json::json!({ "type": "pong" }).to_string();
                                    if ws_tx.send(Message::Text(pong.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        let w = writer.clone();
                        let chunk = b.to_vec();
                        if tokio::task::spawn_blocking(move || {
                            if let Ok(mut g) = w.lock() {
                                let _ = g.write_all(&chunk);
                                let _ = g.flush();
                            }
                        })
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                }
            }
        }
    }

    let _ = killer.kill();
    info!("SSH terminal WebSocket finished for {}", peer);
}
