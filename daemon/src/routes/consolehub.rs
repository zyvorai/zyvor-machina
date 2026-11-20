// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Classic-mode ConsoleHub: console plan and native console sessions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_core::libvirt::{guest_health, vnc};
use machina_core::LibvirtManager;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::{AuthSource, RequestActor};
use crate::conn_query::{spawn_libvirt_actor, ConnQuery};
use crate::error::AppError;

#[derive(Clone)]
pub struct ConsoleSessionStore {
    inner: Arc<RwLock<HashMap<Uuid, LiveConsoleSession>>>,
    history: Arc<RwLock<Vec<SessionHistoryRow>>>,
}

#[derive(Clone)]
struct LiveConsoleSession {
    vm_name: String,
    actor: String,
    protocol: String,
    backend: String,
    expires: Instant,
}

#[derive(Clone, Serialize)]
struct SessionHistoryRow {
    session_id: String,
    vm_name: String,
    actor: String,
    protocol: String,
    backend: String,
    started_at: String,
    ended_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConsoleHubPlan {
    pub vm_name: String,
    pub recommended: String,
    pub native: NativeConsoleInfo,
    pub guest_ip: Option<String>,
    pub ssh_user: Option<String>,
    pub os_hint: String,
    pub protocols: Vec<String>,
    pub webrtc_spice_available: bool,
}

#[derive(Debug, Serialize)]
pub struct NativeConsoleInfo {
    pub console_type: String,
    pub ws_path: String,
    pub serial_ws_path: String,
    pub available: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionBody {
    pub protocol: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConsoleSessionResponse {
    pub session_id: String,
    pub vm_name: String,
    pub protocol: String,
    pub backend: String,
    pub embed_path: String,
    pub emergency_url: Option<String>,
    pub audit_id: String,
    pub expires_at: String,
}

impl ConsoleSessionStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    async fn insert(&self, session: LiveConsoleSession) -> Uuid {
        let id = Uuid::new_v4();
        let row = SessionHistoryRow {
            session_id: id.to_string(),
            vm_name: session.vm_name.clone(),
            actor: session.actor.clone(),
            protocol: session.protocol.clone(),
            backend: session.backend.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
            ended_at: None,
        };
        let mut map = self.inner.write().await;
        map.retain(|_, v| v.expires > Instant::now());
        map.insert(id, session);
        drop(map);
        let mut hist = self.history.write().await;
        hist.push(row);
        if hist.len() > 200 {
            let excess = hist.len() - 200;
            hist.drain(0..excess);
        }
        id
    }

    async fn end(&self, id: Uuid, actor: &str) -> bool {
        let mut map = self.inner.write().await;
        // Verify ownership BEFORE removing, so a caller who only knows the
        // session UUID cannot tear down another user's console.
        match map.get(&id) {
            None => return false,
            Some(session) if session.actor != actor => return false,
            Some(_) => {}
        }
        map.remove(&id);
        drop(map);
        let mut hist = self.history.write().await;
        if let Some(row) = hist
            .iter_mut()
            .rev()
            .find(|r| r.session_id == id.to_string())
        {
            row.ended_at = Some(chrono::Utc::now().to_rfc3339());
        }
        true
    }

    async fn list_for_vm(&self, vm_name: &str) -> Vec<SessionHistoryRow> {
        let hist = self.history.read().await;
        hist.iter()
            .rev()
            .filter(|r| r.vm_name == vm_name)
            .take(50)
            .cloned()
            .collect()
    }
}

fn build_protocol_list(
    console_type: &str,
    _vnc_port: i32,
    guest_ip: &str,
    guest_ip_host_observed: bool,
    os_hint: &str,
    serial_available: bool,
) -> Vec<String> {
    let mut out = vec!["novnc".into()];
    if console_type == "spice" {
        out.push("spice".into());
        out.push("webrtc_spice".into());
    }
    if !guest_ip.is_empty() {
        // In-browser shell over the daemon's PTY terminal. Previously only ever
        // synthesized client-side, which left classic mode with no shell at all.
        out.push("native_ssh".into());
        // Probed, not assumed: Remote Desktop is off by default in Windows, and
        // offering a console that dials a closed port is worse than offering none.
        // Only probed when guest_ip is host-observed (DHCP lease / ARP), not a
        // bare guest-agent self-report — dialing a guest-controlled address
        // turns this into a port-scan oracle against whatever network the host
        // can reach.
        if os_hint == "windows"
            && guest_ip_host_observed
            && machina_core::guest_os::rdp_reachable(guest_ip)
        {
            out.push("rdp".into());
        }
    }
    // Only advertise serial when the domain XML has a console/serial PTY path.
    // Otherwise the WS opens, prints "No console PTY…", and immediately closes —
    // worse than hiding the lens.
    if serial_available {
        out.push("serial".into());
    }
    out
}

async fn build_plan(
    manager: &LibvirtManager,
    actor: &RequestActor,
    conn_q: ConnQuery,
    vm_name: &str,
) -> Result<ConsoleHubPlan, AppError> {
    let name2 = vm_name.to_string();
    let conn_str = conn_q.connection.clone().unwrap_or_default();
    let (
        _vnc_host,
        vnc_port,
        console_type,
        serial_available,
        guest_ip,
        guest_ip_host_observed,
        os_hint,
    ) = spawn_libvirt_actor(manager.clone(), Some(actor), conn_q, move |conn| {
        let xml = machina_core::libvirt::domain::get_vm_xml(conn, &name2).unwrap_or_default();
        let has_spice = machina_core::libvirt::graphics_convert::domain_has_spice_graphics(&xml);
        let (vnc_host, vnc_port) =
            vnc::resolve_vnc_tcp_xml(conn, &name2, &xml).unwrap_or(("".into(), 0));
        let console_type = if vnc_port > 0 {
            "vnc".to_string()
        } else if has_spice {
            "spice".to_string()
        } else {
            "unknown".to_string()
        };
        let serial_available = machina_core::xml::extract_attr(&xml, "console", "tty")
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
            })
            .is_some();
        let mut guest_ip = String::new();
        let mut guest_ip_host_observed = false;
        let mut os_hint = machina_core::guest_os::detect_os_hint(&xml, &name2);
        if let Ok(health) = guest_health::gather_guest_health(conn, &name2) {
            if let Some(guest) = &health.guest {
                // Prefer a host-observed address (DHCP lease or kernel ARP
                // table) over one the guest agent self-reports: `guest_ip`
                // is what `rdp_reachable` below dials over the network, and
                // trusting a guest-controlled value there lets a malicious
                // guest make the host probe arbitrary addresses and read
                // back whether a port is open — a port-scan oracle.
                let host_observed = guest.ip_addresses.iter().find(|a| {
                    a.ip_type == "ipv4" && !a.address.starts_with("127.") && a.source != "agent"
                });
                guest_ip = host_observed
                    .or_else(|| {
                        guest
                            .ip_addresses
                            .iter()
                            .find(|a| a.ip_type == "ipv4" && !a.address.starts_with("127."))
                    })
                    .map(|a| a.address.clone())
                    .unwrap_or_default();
                guest_ip_host_observed = host_observed.is_some();
            }
            if let Some(ref pretty) = health.os_pretty_name {
                os_hint = machina_core::guest_os::refine_os_hint(&os_hint, pretty);
            }
        }
        Ok((
            vnc_host,
            vnc_port,
            console_type,
            serial_available,
            guest_ip,
            guest_ip_host_observed,
            os_hint,
        ))
    })
    .await?;

    // Serial is always the last resort — only when no graphical display and no SSH/RDP alternative.
    let recommended = if console_type == "spice" {
        "spice".into()
    } else if console_type == "vnc" && vnc_port > 0 {
        "novnc".into()
    } else if serial_available {
        "serial".into()
    } else {
        "novnc".into()
    };

    let ssh_user = std::env::var("MACHINA_DEFAULT_SSH_USER").unwrap_or_else(|_| "ubuntu".into());
    let ws_suffix = if conn_str.is_empty() || conn_str == "system" {
        String::new()
    } else {
        format!("&connection={}", urlencoding_light(&conn_str))
    };
    let guest_ip_for_protocols = guest_ip.clone();
    let protocols = build_protocol_list(
        &console_type,
        vnc_port as i32,
        &guest_ip_for_protocols,
        guest_ip_host_observed,
        &os_hint,
        serial_available,
    );

    Ok(ConsoleHubPlan {
        vm_name: vm_name.to_string(),
        recommended,
        native: NativeConsoleInfo {
            console_type: console_type.clone(),
            ws_path: format!(
                "/ws/v1/vnc/{}?token=__WS_TOKEN__{ws_suffix}",
                urlencoding_light(vm_name)
            ),
            serial_ws_path: format!(
                "/ws/v1/console/{}?token=__WS_TOKEN__{ws_suffix}",
                urlencoding_light(vm_name)
            ),
            available: vnc_port > 0,
        },
        guest_ip: if guest_ip.is_empty() {
            None
        } else {
            Some(guest_ip)
        },
        ssh_user: Some(ssh_user),
        os_hint,
        protocols,
        webrtc_spice_available: console_type == "spice",
    })
}

fn urlencoding_light(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

pub fn api_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/vms/{name}/consolehub/plan", get(consolehub_plan))
        .route(
            "/vms/{name}/consolehub/sessions",
            get(list_sessions).post(create_session),
        )
        .route("/consolehub/sessions/{session_id}/end", post(end_session))
}

async fn consolehub_plan(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
) -> Result<Json<ConsoleHubPlan>, AppError> {
    let _ = &actor;
    Ok(Json(build_plan(&manager, &actor, conn_q, &name).await?))
}

async fn list_sessions(
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<ConsoleSessionStore>,
    Path(name): Path<String>,
) -> Result<Json<Vec<SessionHistoryRow>>, AppError> {
    // Session history (actor/vm_name/protocol/timestamps) is audit data, same
    // as `/audit` — and the controller's equivalent endpoint
    // (`controller/src/consolehub.rs::list_sessions`) requires an operator
    // role. Mirror that here rather than leaving it open to every
    // authenticated user, including read-only accounts.
    if actor.role == machina_core::libvirt::automation::Role::ReadOnly {
        return Err(AppError::from(machina_core::LibvirtError::Forbidden(
            "Read-only role cannot view console session history".into(),
        )));
    }
    Ok(Json(store.list_for_vm(&name).await))
}

fn check_oidc_for_console(actor: &RequestActor) -> Result<(), AppError> {
    let required = std::env::var("CONSOLEHUB_REQUIRE_OIDC")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if !required {
        return Ok(());
    }
    if actor.auth_source == AuthSource::Oidc {
        return Ok(());
    }
    Err(AppError::from(machina_core::LibvirtError::Forbidden(
        "ConsoleHub requires OIDC federation — sign in via SSO before opening a console.".into(),
    )))
}

async fn create_session(
    State(manager): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<ConsoleSessionStore>,
    Path(name): Path<String>,
    Query(conn_q): Query<ConnQuery>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<ConsoleSessionResponse>, AppError> {
    check_oidc_for_console(&actor)?;
    let plan = build_plan(&manager, &actor, conn_q.clone(), &name).await?;
    let protocol = body
        .protocol
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| plan.recommended.clone());
    // Without this, a caller could request e.g. "rdp" for a Linux VM, or for a
    // Windows VM with RDP unreachable, and get a 200 with a session_id/embed_path
    // that looks like success — there's no browser RDP renderer at all
    // (Guacamole was removed), so the caller has no other signal the session is
    // unusable until they try to actually use it.
    if !plan.protocols.iter().any(|p| p == &protocol) {
        return Err(AppError::from(machina_core::LibvirtError::Invalid(
            format!(
                "protocol '{protocol}' is not available for this VM (available: {})",
                plan.protocols.join(", ")
            ),
        )));
    }

    let ttl = Duration::from_secs(
        std::env::var("CONSOLEHUB_SESSION_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600),
    );
    let audit_id = Uuid::new_v4();
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl.as_secs() as i64);

    // Every console session is native.
    let (backend, emergency_url): (String, Option<String>) = ("native".to_string(), None);

    let session_id = store
        .insert(LiveConsoleSession {
            vm_name: name.clone(),
            actor: actor.username.clone(),
            protocol: protocol.clone(),
            backend: backend.clone(),
            expires: Instant::now() + ttl,
        })
        .await;

    let embed_path = format!(
        "/vms/{}/consolehub?session={session_id}&native=1",
        urlencoding_light(&name)
    );

    Ok(Json(ConsoleSessionResponse {
        session_id: session_id.to_string(),
        vm_name: name,
        protocol,
        backend,
        embed_path,
        emergency_url,
        audit_id: audit_id.to_string(),
        expires_at: expires_at.to_rfc3339(),
    }))
}

async fn end_session(
    Extension(actor): Extension<RequestActor>,
    Extension(store): Extension<ConsoleSessionStore>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ended = store.end(session_id, &actor.username).await;
    Ok(Json(serde_json::json!({
        "ended": ended,
        "session_id": session_id.to_string(),
    })))
}
