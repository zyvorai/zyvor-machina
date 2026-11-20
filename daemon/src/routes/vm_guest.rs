// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Cockpit-parity helpers: send keys, screenshot, TPM, firmware, device tuning, extra devices.

use axum::extract::{Extension, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::collections::HashMap;

use machina_core::libvirt::{device_tune, domain, extra_devices, firmware, guest_input, vnc};
use machina_core::{LibvirtError, LibvirtManager};

use crate::auth::{require_write, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;
use machina_core::xml::{extract_attr, split_blocks};

#[derive(serde::Deserialize)]
struct SendKeyRequest {
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    keycodes: Option<Vec<u32>>,
    #[serde(default = "default_hold")]
    holdtime_ms: u32,
}
fn default_hold() -> u32 {
    100
}

async fn send_key_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<SendKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let codes: Vec<u32> = match (&req.preset, &req.keycodes) {
        (Some(p), _) if p.eq_ignore_ascii_case("ctrl_alt_del") => vec![
            guest_input::KEY_LEFTCTRL,
            guest_input::KEY_LEFTALT,
            guest_input::KEY_DELETE,
        ],
        (Some(p), _) if p.eq_ignore_ascii_case("esc") => vec![guest_input::KEY_ESC],
        (Some(p), _) if p.eq_ignore_ascii_case("alt_tab") => {
            vec![guest_input::KEY_LEFTALT, guest_input::KEY_TAB]
        }
        (_, Some(k)) if !k.is_empty() => k.clone(),
        _ => {
            return Err(LibvirtError::Invalid(
                "Provide preset (ctrl_alt_del, esc, alt_tab) or keycodes[] (Linux evdev)".into(),
            )
            .into());
        }
    };
    let name2 = name.clone();
    let ht = req.holdtime_ms;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_input::send_linux_keycodes(conn, &name2, &codes, ht)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

async fn screenshot_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let screen: u32 = q.get("screen").and_then(|s| s.parse().ok()).unwrap_or(0);
    let name2 = name.clone();
    let (bytes, mime) = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        guest_input::screenshot(conn, &name2, screen)
    })
    .await?;
    let hv = HeaderValue::from_str(&mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    Ok(([(header::CONTENT_TYPE, hv)], bytes))
}

#[derive(serde::Deserialize)]
struct FirmwareRequest {
    /// `true` = UEFI (OVMF), `false` = BIOS.
    uefi: bool,
}

async fn set_firmware_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<FirmwareRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let uefi = req.uefi;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        firmware::set_guest_firmware(conn, &name2, uefi)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "uefi": req.uefi }),
    ))
}

async fn attach_tpm_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extra_devices::attach_tpm_emulator(conn, &name2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "tpm": "attached" }),
    ))
}

async fn detach_tpm_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let outcome = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extra_devices::detach_tpm(conn, &name2)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "tpm": "detached", "live_removed": outcome.live_removed }),
    ))
}

#[derive(serde::Deserialize)]
struct WatchdogRequest {
    model: String,
    action: String,
}

async fn attach_watchdog_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<WatchdogRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let model = req.model.clone();
    let action = req.action.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extra_devices::attach_watchdog(conn, &name2, &model, &action)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

#[derive(serde::Deserialize)]
struct SoundRequest {
    model: String,
}

async fn attach_sound_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<SoundRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let model = req.model.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extra_devices::attach_sound(conn, &name2, &model)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

#[derive(serde::Deserialize)]
struct SerialPortRequest {
    port: u32,
}

async fn attach_serial_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<SerialPortRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let port = req.port;
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        extra_devices::attach_serial_pty(conn, &name2, port)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "port": port }),
    ))
}

#[derive(serde::Deserialize)]
struct VideoModelRequest {
    model: String,
}

async fn set_video_model_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<VideoModelRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let model = req.model.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device_tune::set_video_model(conn, &name2, &model)
    })
    .await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "name": name, "model": req.model }),
    ))
}

async fn disk_tune_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<device_tune::DiskTuneRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let tune = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device_tune::update_disk_tune(conn, &name2, &tune)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

async fn nic_tune_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    Path(name): Path<String>,
    Json(req): Json<device_tune::NicTuneRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;
    let name2 = name.clone();
    let tune = req.clone();
    spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        device_tune::update_nic_tune(conn, &name2, &tune)
    })
    .await?;
    Ok(Json(serde_json::json!({ "status": "ok", "name": name })))
}

/// Virt-Viewer `.vv` file (SPICE or VNC).
async fn virt_viewer_vv_handler(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Query(conn_q): Query<ConnQuery>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let name2 = name.clone();
    let (xml, vnc_resolved) = spawn_libvirt_actor(manager, Some(&actor), conn_q, move |conn| {
        let xml = domain::get_vm_xml(conn, &name2)?;
        let vnc = vnc::resolve_vnc_tcp_xml(conn, &name2, &xml).ok();
        Ok::<_, LibvirtError>((xml, vnc))
    })
    .await?;

    let mut console_type = String::from("vnc");
    let mut port: i32 = -1;
    for block in split_blocks(&xml, "graphics") {
        let gtype = extract_attr(&block, "graphics", "type").unwrap_or_default();
        let gport = extract_attr(&block, "graphics", "port")
            .and_then(|s| s.parse().ok())
            .unwrap_or(-1);
        if gtype == "vnc" || gtype == "spice" {
            console_type = gtype.clone();
            port = gport;
            if gtype == "vnc" {
                break;
            }
        }
    }

    let mut host = headers
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
            host = h;
            port = i32::from(p);
        }
    }

    if port <= 0 {
        return Err(LibvirtError::Operation(
            "Graphics port not available (is the VM running?)".into(),
        )
        .into());
    }

    let body = if console_type == "spice" {
        format!(
            "[virt-viewer]\ntype=spice\nhost={host}\nport={port}\ntitle={name}\ndelete-this-file=0\n"
        )
    } else {
        format!(
            "[virt-viewer]\ntype=vnc\nhost={host}\nport={port}\ntitle={name}\ndelete-this-file=0\n"
        )
    };

    let cd = format!("attachment; filename=\"{name}.vv\"");
    let cdv = HeaderValue::from_str(&cd)
        .unwrap_or_else(|_| HeaderValue::from_static("attachment; filename=\"guest.vv\""));

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, cdv),
        ],
        body,
    ))
}

pub fn vm_guest_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/vms/{name}/guest/send-key", post(send_key_handler))
        .route("/vms/{name}/guest/screenshot", get(screenshot_handler))
        .route("/vms/{name}/firmware", post(set_firmware_handler))
        .route(
            "/vms/{name}/devices/tpm",
            post(attach_tpm_handler).delete(detach_tpm_handler),
        )
        .route(
            "/vms/{name}/devices/watchdog",
            post(attach_watchdog_handler),
        )
        .route("/vms/{name}/devices/sound", post(attach_sound_handler))
        .route("/vms/{name}/devices/serial", post(attach_serial_handler))
        .route(
            "/vms/{name}/devices/video-model",
            post(set_video_model_handler),
        )
        .route("/vms/{name}/disk/tune", post(disk_tune_handler))
        .route("/vms/{name}/nic/tune", post(nic_tune_handler))
        .route("/vms/{name}/virt-viewer.vv", get(virt_viewer_vv_handler))
}
