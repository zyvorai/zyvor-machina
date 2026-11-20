// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Serial-over-LAN console for bare-metal servers: `ipmitool sol activate` on a pseudo
//! terminal, bridged to a WebSocket the web terminal (xterm) already knows how to speak.
//!
//! The browser gets a short-lived token from `POST .../sol-token` (admin, audited) and
//! connects to `/ws/v1/baremetal/{id}/sol?token=…`. One SOL session per BMC can be active, so
//! a stale session is deactivated before a new one starts. The BMC password goes to
//! `ipmitool` through `IPMI_PASSWORD`, never argv.

use std::process::Stdio;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use uuid::Uuid;

use crate::engine::baremetal_bmc::{
    split_host_port, validate_address, validate_username, BmcKind, BmcTarget,
};
use crate::state::AppState;

fn ipmitool_bin() -> String {
    std::env::var("MACHINA_IPMITOOL").unwrap_or_else(|_| "ipmitool".into())
}

/// `'...'` for a POSIX shell; a value with a `'` is refused rather than escaped.
fn shell_quote(s: &str) -> anyhow::Result<String> {
    if s.contains('\'') || s.chars().any(|c| c.is_control()) {
        anyhow::bail!("value cannot be put on a command line safely");
    }
    Ok(format!("'{s}'"))
}

/// The `ipmitool` command line (without the password) for `sol <verb>`.
fn sol_command(target: &BmcTarget<'_>, verb: &str) -> anyhow::Result<String> {
    validate_address(target.address)?;
    validate_username(target.username)?;
    let (host, port) = split_host_port(target.address);
    let mut cmd = format!(
        "{} -I lanplus -H {} -U {} -E",
        shell_quote(&ipmitool_bin())?,
        shell_quote(host)?,
        shell_quote(target.username)?
    );
    if let Some(p) = port {
        cmd.push_str(&format!(" -p {p}"));
    }
    cmd.push_str(&format!(" sol {verb}"));
    Ok(cmd)
}

pub struct SolProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}

/// Free a stale session, then `sol activate` under a pty (`script` allocates one: ipmitool
/// wants a terminal).
pub async fn start(target: &BmcTarget<'_>) -> anyhow::Result<SolProcess> {
    if target.kind != BmcKind::Ipmi {
        anyhow::bail!(
            "serial-over-LAN needs an IPMI BMC; this server uses {:?}",
            target.kind
        );
    }
    let deactivate = sol_command(target, "deactivate")?;
    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("sh")
            .args(["-c", &deactivate])
            .env("IPMI_PASSWORD", target.password)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;

    let activate = sol_command(target, "activate")?;
    let mut child = Command::new("script")
        .args(["-q", "-f", "-c", &activate, "/dev/null"])
        .env("IPMI_PASSWORD", target.password)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            anyhow::anyhow!("cannot start the SOL session (is `script` installed?): {e}")
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    Ok(SolProcess {
        child,
        stdin,
        stdout,
    })
}

/// Best-effort release of the BMC's SOL payload when a session ends.
async fn deactivate(target_cmd: String, password: String) {
    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("sh")
            .args(["-c", &target_cmd])
            .env("IPMI_PASSWORD", password)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
}

#[derive(Debug, Deserialize)]
pub struct SolQuery {
    pub token: String,
}

/// `GET /ws/v1/baremetal/{id}/sol?token=` (WebSocket upgrade).
pub async fn sol_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<SolQuery>,
) -> Response {
    let Some(grant) = state
        .ws_tokens
        .validate(&q.token)
        .await
        .filter(|g| g.vm_id == id)
    else {
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid token").into_response();
    };
    if grant.read_only {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "a serial console is interactive; read-only grants may not open one",
        )
            .into_response();
    }
    let creds = match crate::engine::baremetal::load_bmc(&state.pool, id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "BMC credentials are not set",
            )
                .into_response()
        }
        Err(e) => return (axum::http::StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };
    let proc = match start(&creds.target()).await {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let release = sol_command(&creds.target(), "deactivate").ok();
    let password = creds.password.clone();
    ws.on_upgrade(move |socket| async move {
        bridge(socket, proc).await;
        if let Some(cmd) = release {
            deactivate(cmd, password).await;
        }
    })
}

async fn bridge(socket: WebSocket, mut p: SolProcess) {
    let (mut sink, mut stream) = socket.split();
    let mut stdin = p.stdin;
    let mut stdout = p.stdout;

    let to_bmc = async {
        while let Some(Ok(msg)) = stream.next().await {
            let data: Vec<u8> = match msg {
                Message::Binary(b) => b.to_vec(),
                Message::Text(t) => t.as_bytes().to_vec(),
                Message::Close(_) => break,
                _ => continue,
            };
            if stdin.write_all(&data).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
        }
    };
    let from_bmc = async {
        let mut buf = vec![0u8; 4096];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if sink
                        .send(Message::Binary(buf[..n].to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = sink.close().await;
    };
    tokio::select! {
        _ = to_bmc => {},
        _ = from_bmc => {},
    }
    let _ = p.child.kill().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t<'a>(addr: &'a str, user: &'a str) -> BmcTarget<'a> {
        BmcTarget {
            kind: BmcKind::Ipmi,
            address: addr,
            username: user,
            password: "pw",
        }
    }

    #[test]
    fn the_command_line_is_quoted_carries_the_port_and_no_password() {
        let c = sol_command(&t("10.0.0.5:6230", "admin"), "activate").unwrap();
        assert!(c.contains("-H '10.0.0.5'"), "{c}");
        assert!(c.contains("-p 6230"), "{c}");
        assert!(c.ends_with("sol activate"), "{c}");
        assert!(!c.contains("pw"), "{c}");
        assert!(sol_command(&t("10.0.0.5", "ad'min"), "activate").is_err());
        assert!(sol_command(&t("-oProxyCommand=x", "admin"), "activate").is_err());
        assert!(sol_command(&t("10.0.0.5", "-U"), "activate").is_err());
    }

    #[tokio::test]
    async fn only_ipmi_servers_get_a_sol_session() {
        let r = start(&BmcTarget {
            kind: BmcKind::Redfish,
            ..t("10.0.0.5", "admin")
        })
        .await;
        assert!(r.is_err());
    }

    async fn read_until(out: &mut ChildStdout, needle: &str) -> String {
        let mut acc = String::new();
        let mut b = [0u8; 256];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !acc.contains(needle) && tokio::time::Instant::now() < deadline {
            if let Ok(Ok(n)) =
                tokio::time::timeout(Duration::from_millis(500), out.read(&mut b)).await
            {
                if n == 0 {
                    break;
                }
                acc.push_str(&String::from_utf8_lossy(&b[..n]));
            }
        }
        acc
    }

    /// A stand-in for ipmitool: `sol activate` echoes what it is sent, as upper case.
    #[tokio::test]
    async fn a_session_reads_and_writes_through_a_pty() {
        if std::process::Command::new("script")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // no util-linux `script` on this machine
        }
        let dir = std::env::temp_dir().join(format!("machina-sol-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fake-ipmitool");
        std::fs::write(
            &fake,
            "#!/bin/sh\nfor a in \"$@\"; do last=$a; done\n\
             [ \"$last\" = activate ] && [ -n \"$IPMI_PASSWORD\" ] && \
             { echo ready; while read l; do echo \"$l\" | tr a-z A-Z; done; }\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::env::set_var("MACHINA_IPMITOOL", fake.display().to_string());
        let mut p = start(&t("10.0.0.5", "admin")).await.unwrap();
        let banner = read_until(&mut p.stdout, "ready").await;
        assert!(banner.contains("ready"), "{banner}");
        p.stdin.write_all(b"hello\n").await.unwrap();
        let echoed = read_until(&mut p.stdout, "HELLO").await;
        assert!(echoed.contains("HELLO"), "{echoed}");
        let _ = p.child.kill().await;
        std::env::remove_var("MACHINA_IPMITOOL");
        std::fs::remove_dir_all(&dir).ok();
    }
}
