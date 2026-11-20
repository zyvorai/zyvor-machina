// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, Query, State};
use axum::http::header::HeaderValue;
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

use machina_core::libvirt::domain;
use machina_core::libvirt::vnc;
use machina_core::xml::{extract_attr, split_blocks};
use machina_core::LibvirtManager;

use crate::auth::RequestActor;
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;

#[derive(serde::Serialize)]
struct ConsoleInfo {
    name: String,
    console_type: String,
    host: String,
    port: i32,
    websocket_port: i32,
}

async fn get_console_info(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<ConsoleInfo>, AppError> {
    let name2 = name.clone();
    let (xml, vnc_resolved) = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let xml = domain::get_vm_xml(conn, &name2)?;
        let vnc = vnc::resolve_vnc_tcp_xml(conn, &name2, &xml).ok();
        Ok((xml, vnc))
    })
    .await?;

    // Find VNC graphics first, then fall back to any graphics type
    let mut console_type = machina_core::unknown_string();
    let mut port: i32 = -1;
    let mut ws_port: i32 = -1;

    for block in split_blocks(&xml, "graphics") {
        let gtype = extract_attr(&block, "graphics", "type").unwrap_or_default();
        let gport = extract_attr(&block, "graphics", "port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);
        let gwsport = extract_attr(&block, "graphics", "websocket")
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);

        if gtype == "vnc" {
            // Prefer VNC
            console_type = gtype;
            port = gport;
            ws_port = gwsport;
            break;
        }
        // Store first graphics entry as fallback
        if console_type == "unknown" {
            console_type = gtype;
            port = gport;
            ws_port = gwsport;
        }
    }

    // hyper2kvm-style: real TCP port/host from XML or `virsh vncdisplay` (fixes autoport -1).
    let mut listen_host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.split(':').next())
        .filter(|h| {
            h.chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
        })
        .unwrap_or("127.0.0.1")
        .to_string();

    if console_type == "vnc" {
        if let Some((h, p)) = vnc_resolved {
            listen_host = h;
            port = i32::from(p);
        }
    }

    Ok(Json(ConsoleInfo {
        name,
        console_type,
        host: listen_host,
        port,
        websocket_port: ws_port,
    }))
}

/// Download a `virt-viewer` / Remote Desktop `.vv` file (same idea as Cockpit-machines).
async fn viewer_vv_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let name2 = name.clone();
    let (xml, vnc_resolved) = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let xml = domain::get_vm_xml(conn, &name2)?;
        let vnc = vnc::resolve_vnc_tcp_xml(conn, &name2, &xml).ok();
        Ok((xml, vnc))
    })
    .await?;

    let mut console_type = machina_core::unknown_string();
    let mut port: i32 = -1;

    for block in split_blocks(&xml, "graphics") {
        let gtype = extract_attr(&block, "graphics", "type").unwrap_or_default();
        let gport = extract_attr(&block, "graphics", "port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);
        if gtype == "vnc" {
            console_type = gtype;
            port = gport;
            break;
        }
        if console_type == "unknown" {
            console_type = gtype;
            port = gport;
        }
    }

    let mut listen_host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.split(':').next())
        .filter(|h| {
            h.chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
        })
        .unwrap_or("127.0.0.1")
        .to_string();

    if console_type == "vnc" {
        if let Some((h, p)) = vnc_resolved {
            listen_host = h;
            port = i32::from(p);
        }
    }

    let vv = format!(
        "[virt-viewer]\ntype={console_type}\nhost={listen_host}\nport={port}\ndelete-this-file=1\nfullscreen=0\n"
    );

    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/x-virt-viewer"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"console.vv\""),
            ),
        ],
        vv,
    ))
}

pub fn console_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/vms/console-info/{name}", get(get_console_info))
        .route("/vms/{name}/viewer.vv", get(viewer_vv_handler))
}
