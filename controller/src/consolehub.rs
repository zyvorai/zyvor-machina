// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Zeus ConsoleHub — unified console plan and native console sessions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use machina_spec::VirtualMachine;
use serde::{Deserialize, Serialize};
use crate::db::Json as SqlxJson;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::agent_client;
use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Clone)]
pub struct ConsoleSessionStore {
    inner: Arc<RwLock<HashMap<Uuid, LiveConsoleSession>>>,
}

#[derive(Clone)]
struct LiveConsoleSession {
    expires: Instant,
    /// The ws-token minted alongside this session (embedded in its embed_path).
    /// Kept here so ending the session can also revoke the token — otherwise
    /// the token stays proxyable against vnc/serial/spice until its own TTL
    /// lapses regardless of the session having been "ended".
    ws_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestAccessHints {
    /// ssh_key | password | both | unknown
    pub auth_mode: String,
    pub serial_password_login: bool,
    pub guest_ip_private: bool,
    pub ssh_nat_host_port: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct ConsolePermissions {
    pub role: String,
    pub read_only: bool,
    pub can_power: bool,
    pub can_snapshot: bool,
    pub can_send_keys: bool,
}

impl Default for ConsolePermissions {
    fn default() -> Self {
        Self {
            role: "admin".into(),
            read_only: false,
            can_power: true,
            can_snapshot: true,
            can_send_keys: true,
        }
    }
}

fn console_permissions_for(user: &AuthUser) -> ConsolePermissions {
    let read_only = user.role == "viewer" || user.role == "readonly";
    let can_power = matches!(user.role.as_str(), "admin" | "operator");
    ConsolePermissions {
        role: user.role.clone(),
        read_only,
        can_power,
        can_snapshot: can_power,
        can_send_keys: !read_only,
    }
}

#[derive(Debug, Serialize)]
pub struct ConsoleHubPlan {
    pub vm_id: String,
    pub vm_name: String,
    pub recommended: String,
    pub native: NativeConsoleInfo,
    pub guest_ip: Option<String>,
    pub ssh_user: Option<String>,
    pub os_hint: String,
    pub protocols: Vec<String>,
    pub webrtc_spice_available: bool,
    pub guest_access: GuestAccessHints,
    pub hypervisor_address: Option<String>,
    pub ssh_connect_host: Option<String>,
    pub ssh_connect_port: Option<u16>,
    /// True when `CONSOLEHUB_RECORDING_ENABLED` is set on the controller.
    pub session_recording_enabled: bool,
    pub permissions: ConsolePermissions,
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
    #[serde(default)]
    pub break_glass: bool,
}

#[derive(Debug, Serialize)]
pub struct ConsoleSessionResponse {
    pub session_id: String,
    pub vm_id: String,
    pub protocol: String,
    pub backend: String,
    pub embed_path: String,
    pub emergency_url: Option<String>,
    pub audit_id: String,
    pub expires_at: String,
    pub spectator_token: Option<String>,
    pub recording_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct SpectatorValidateQuery {
    pub session_id: Uuid,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct SpectatorValidateResponse {
    pub valid: bool,
    pub vm_id: String,
    pub actor: String,
    pub protocol: String,
    pub read_only: bool,
}

impl ConsoleSessionStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn insert(&self, session: LiveConsoleSession) -> Uuid {
        let id = Uuid::new_v4();
        let mut map = self.inner.write().await;
        map.retain(|_, v| v.expires > Instant::now());
        map.insert(id, session);
        id
    }

    /// Removes the session and returns its ws-token, if any, so the caller
    /// can also revoke it from `WsTokenStore`.
    async fn remove(&self, id: Uuid) -> Option<String> {
        let mut map = self.inner.write().await;
        map.remove(&id).map(|s| s.ws_token)
    }
}

impl Default for ConsoleSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/vms/{id}/consolehub/plan", get(consolehub_plan))
        .route(
            "/api/v1/vms/{id}/consolehub/sessions",
            get(list_sessions).post(create_session),
        )
        .route(
            "/api/v1/consolehub/sessions/{session_id}/end",
            post(end_session),
        )
        .route(
            "/api/v1/consolehub/sessions/{session_id}/replay",
            get(get_session_replay).put(upload_session_replay),
        )
        .route(
            "/api/v1/vms/{id}/consolehub/access-requests",
            post(create_access_request),
        )
        .route(
            "/api/v1/consolehub/access-requests/{request_id}/approve",
            post(approve_access_request),
        )
        .route(
            "/api/v1/vms/{id}/consolehub/break-glass",
            post(break_glass_session),
        )
        .route(
            "/api/v1/vms/{id}/consolehub/collaborate",
            post(collaborate_session),
        )
        .route(
            "/api/v1/consolehub/spectator/validate",
            get(validate_spectator),
        )
        .route(
            "/api/v1/vms/{id}/consolehub/explain",
            post(consolehub_explain),
        )
}

async fn vm_row(state: &AppState, id: Uuid) -> Result<(String, Uuid), ApiError> {
    let row: (String, Option<Uuid>) = crate::db::query_as("SELECT name, host_id FROM vms WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let host_id = row
        .1
        .ok_or_else(|| ApiError::bad_request("vm has no host"))?;
    Ok((row.0, host_id))
}

async fn vm_meta(
    state: &AppState,
    id: Uuid,
) -> Result<(String, Option<Uuid>, String, Option<String>), ApiError> {
    let row: (String, Option<Uuid>, String, Option<String>) = crate::db::query_as(
        "SELECT name, host_id, COALESCE(inventory_source, 'libvirt'), k8s_namespace FROM vms WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(row)
}

fn kubevirt_plan(vm_id: Uuid, vm_name: &str, ws_token: &str) -> ConsoleHubPlan {
    ConsoleHubPlan {
        vm_id: vm_id.to_string(),
        vm_name: vm_name.to_string(),
        recommended: "novnc".into(),
        native: NativeConsoleInfo {
            console_type: "vnc".into(),
            // Same daemon relay path libvirt VMs use — vnc_ws_proxy/serial_ws_proxy
            // in console.rs validate this controller-issued token (as always) and
            // branch on inventory_source to relay to the daemon's KubeVirt proxy
            // instead of an agent. The old direct `/ws/v1/k8s-kubevirt/...` path
            // embedded this same controller-only token, but the daemon validated
            // it against its own unrelated ws-token store and always rejected it
            // (401) before the KubeVirt handler ever ran — every console tile
            // failed silently.
            ws_path: format!("/ws/v1/platform/vnc/{vm_id}?token={ws_token}"),
            serial_ws_path: format!("/ws/v1/platform/serial/{vm_id}?token={ws_token}"),
            available: true,
        },
        guest_ip: None,
        ssh_user: Some("cloud-user".into()),
        os_hint: "kubevirt".into(),
        protocols: vec!["novnc".into(), "serial".into()],
        webrtc_spice_available: false,
        guest_access: empty_guest_access(),
        hypervisor_address: None,
        ssh_connect_host: None,
        ssh_connect_port: None,
        session_recording_enabled: false,
        permissions: ConsolePermissions::default(),
    }
}

async fn kubevirt_plan_enriched(
    pool: &crate::db::DbPool,
    daemon_base_url: &str,
    vm_id: Uuid,
    vm_name: &str,
    namespace: &str,
    ws_token: &str,
) -> ConsoleHubPlan {
    let mut plan = kubevirt_plan(vm_id, vm_name, ws_token);
    let row: Option<(Option<String>, serde_json::Value)> =
        crate::db::query_as("SELECT guest_ip, spec_json FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some((guest_ip, spec)) = row {
        if let Some(ip) = guest_ip.filter(|s| !s.trim().is_empty()) {
            plan.guest_ip = Some(ip.clone());
            plan.guest_access.guest_ip_private = is_private_guest_ip(&ip);
        }
        if let Ok(vm) = serde_json::from_value::<VirtualMachine>(spec.clone()) {
            if let Some(mode) = auth_mode_from_spec(&vm) {
                plan.guest_access.serial_password_login = mode == "password" || mode == "both";
                plan.guest_access.auth_mode = mode;
            }
            let user = vm
                .spec
                .cloud_init
                .as_ref()
                .map(|ci| ci.user.trim())
                .filter(|u| !u.is_empty());
            if let Some(user) = user {
                plan.ssh_user = Some(user.to_string());
            }
        }
        if let Some(port) = spec
            .get("machina")
            .and_then(|m| m.get("kubevirt"))
            .and_then(|k| k.get("ssh_node_port"))
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
        {
            plan.guest_access.ssh_nat_host_port = Some(port);
            plan.ssh_connect_port = Some(port);
            if let Some(node_ip) = spec
                .get("machina")
                .and_then(|m| m.get("kubevirt"))
                .and_then(|k| k.get("ssh_node_host"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                plan.hypervisor_address = Some(node_ip.trim().to_string());
                plan.ssh_connect_host = Some(node_ip.trim().to_string());
            }
        } else if let Some(expose) = crate::engine::kubevirt_ssh::discover_kubevirt_ssh_expose(
            daemon_base_url,
            namespace,
            vm_name,
        )
        .await
        {
            plan.guest_access.ssh_nat_host_port = Some(expose.port);
            plan.ssh_connect_port = Some(expose.port);
            plan.hypervisor_address = Some(expose.host.clone());
            plan.ssh_connect_host = Some(expose.host);
            plan.guest_access.guest_ip_private = true;
        }
    }
    plan
}

fn check_federated_console_auth(state: &AppState, user: &AuthUser) -> Result<(), ApiError> {
    if !state.config.consolehub_require_oidc {
        return Ok(());
    }
    match user.auth_source.as_deref() {
        Some("oidc") | Some("saml") => Ok(()),
        _ => Err(
            ApiError::bad_request("ConsoleHub requires federated SSO login")
                .with_code("console_oidc_required")
                .with_remediation(
                    "Sign in via Platform → OIDC/SAML before opening a production console.",
                ),
        ),
    }
}

async fn host_agent_grpc(pool: &crate::db::DbPool, host_id: Uuid) -> Result<String, ApiError> {
    let addr: String = crate::db::query_scalar("SELECT agent_grpc_addr FROM hosts WHERE id = ?")
        .bind(host_id)
        .fetch_one(pool)
        .await?;
    Ok(addr)
}

async fn host_agent_console(pool: &crate::db::DbPool, host_id: Uuid) -> Result<String, ApiError> {
    let addr: String = crate::db::query_scalar(
        "SELECT COALESCE(NULLIF(agent_console_addr, ''), agent_grpc_addr) FROM hosts WHERE id = ?",
    )
    .bind(host_id)
    .fetch_one(pool)
    .await?;
    Ok(addr)
}

fn empty_guest_access() -> GuestAccessHints {
    GuestAccessHints {
        auth_mode: "unknown".into(),
        serial_password_login: true,
        guest_ip_private: false,
        ssh_nat_host_port: None,
    }
}

fn is_private_guest_ip(ip: &str) -> bool {
    let Ok(addr) = ip.trim().parse::<std::net::Ipv4Addr>() else {
        return false;
    };
    let o = addr.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 169 && o[1] == 254)
}

fn auth_mode_from_spec(vm: &VirtualMachine) -> Option<String> {
    let ci = vm.spec.cloud_init.as_ref()?;
    let has_pw = ci.password.as_ref().is_some_and(|p| !p.is_empty());
    let has_key = ci.ssh_pubkey.as_ref().is_some_and(|k| !k.is_empty());
    Some(match (has_key, has_pw) {
        (true, true) => "both".into(),
        (true, false) => "ssh_key".into(),
        (false, true) => "password".into(),
        _ => "unknown".into(),
    })
}

fn serial_password_login(auth_mode: &str) -> bool {
    matches!(auth_mode, "password" | "both" | "unknown")
}

async fn build_guest_access_hints(
    pool: &crate::db::DbPool,
    host_id: Uuid,
    agent: &machina_agent::pb::GetConsoleAccessPlanResponse,
    spec_vm: Option<&VirtualMachine>,
) -> GuestAccessHints {
    let mut auth_mode = if agent.guest_auth_mode.is_empty() {
        "unknown".into()
    } else {
        agent.guest_auth_mode.clone()
    };
    if auth_mode == "unknown" {
        if let Some(vm) = spec_vm {
            if let Some(from_spec) = auth_mode_from_spec(vm) {
                auth_mode = from_spec;
            }
        }
    }
    let guest_ip = agent.guest_ip.trim();
    let guest_ip_private = is_private_guest_ip(guest_ip);
    let mut ssh_nat_host_port = None;
    if !guest_ip.is_empty() {
        if let Ok(agent_addr) = host_agent_grpc(pool, host_id).await {
            if let Ok(rules) = agent_client::list_port_forwards(&agent_addr).await {
                ssh_nat_host_port = rules
                    .iter()
                    .find(|r| {
                        r.protocol.eq_ignore_ascii_case("tcp")
                            && r.vm_ip == guest_ip
                            && r.vm_port == 22
                    })
                    .map(|r| r.host_port);
            }
        }
    }
    GuestAccessHints {
        auth_mode: auth_mode.clone(),
        serial_password_login: serial_password_login(&auth_mode),
        guest_ip_private,
        ssh_nat_host_port,
    }
}

fn plan_from_agent(
    vm_id: Uuid,
    vm_name: &str,
    agent: &machina_agent::pb::GetConsoleAccessPlanResponse,
    ws_token: &str,
    guest_access: GuestAccessHints,
    hypervisor_address: Option<String>,
) -> ConsoleHubPlan {
    let recommended = if agent.recommended.is_empty() {
        "novnc".into()
    } else {
        agent.recommended.clone()
    };
    let (ssh_host, ssh_port) =
        ssh_connect_target(&agent.guest_ip, &guest_access, &hypervisor_address);
    ConsoleHubPlan {
        vm_id: vm_id.to_string(),
        vm_name: vm_name.to_string(),
        recommended,
        native: NativeConsoleInfo {
            console_type: agent.console_type.clone(),
            ws_path: if agent.console_type == "spice" && agent.vnc_port == 0 {
                format!("/ws/v1/platform/spice/{vm_id}?token={ws_token}")
            } else {
                format!("/ws/v1/platform/vnc/{vm_id}?token={ws_token}")
            },
            serial_ws_path: format!("/ws/v1/platform/serial/{vm_id}?token={ws_token}"),
            available: agent.vnc_port > 0 || agent.has_spice,
        },
        guest_ip: if agent.guest_ip.is_empty() {
            None
        } else {
            Some(agent.guest_ip.clone())
        },
        ssh_user: if agent.ssh_user.is_empty() {
            None
        } else {
            Some(agent.ssh_user.clone())
        },
        os_hint: if agent.os_hint.is_empty() {
            "unknown".into()
        } else {
            agent.os_hint.clone()
        },
        protocols: build_protocol_list(agent),
        webrtc_spice_available: agent.console_type == "spice" || agent.has_spice,
        guest_access: guest_access.clone(),
        hypervisor_address: hypervisor_address.clone(),
        ssh_connect_host: ssh_host.clone(),
        ssh_connect_port: ssh_port,
        session_recording_enabled: false,
        permissions: ConsolePermissions::default(),
    }
}

fn ssh_connect_target(
    guest_ip: &str,
    guest_access: &GuestAccessHints,
    _hypervisor_address: &Option<String>,
) -> (Option<String>, Option<u16>) {
    if guest_access.guest_ip_private {
        if let Some(port) = guest_access.ssh_nat_host_port {
            let host = Some("127.0.0.1".into());
            return (host, Some(port));
        }
    }
    if guest_ip.trim().is_empty() {
        (None, None)
    } else {
        (Some(guest_ip.trim().to_string()), Some(22))
    }
}

fn build_protocol_list(agent: &machina_agent::pb::GetConsoleAccessPlanResponse) -> Vec<String> {
    let mut out = Vec::new();
    if agent.console_type == "spice" || agent.has_spice {
        out.push("spice".into());
        out.push("webrtc_spice".into());
    }
    out.push("novnc".into());
    if !agent.guest_ip.is_empty() {
        // In-browser shell over the daemon's PTY terminal. The daemon has always
        // emitted this; the controller did not, leaving the platform UI to
        // synthesize it client-side. Both planners now agree.
        out.push("native_ssh".into());
    }
    // Native RDP — the agent sets rdp_port only when a Windows guest is actually
    // listening on 3389.
    if agent.rdp_port > 0 && !agent.guest_ip.is_empty() {
        out.push("rdp".into());
    }
    out.push("serial".into());
    out
}

pub async fn consolehub_plan(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConsoleHubPlan>, ApiError> {
    // Stamp the ws-token with the caller's actual capability: a viewer/read-only
    // user gets a read-only grant the proxies enforce server-side (input frames
    // dropped, serial refused) — the plan's `permissions` object alone is only a
    // frontend hint and does not gate the token→proxy path.
    let read_only = console_permissions_for(&user).read_only;
    let policy = |plan: &mut ConsoleHubPlan| {
        plan.session_recording_enabled = state.config.consolehub_recording_enabled;
        plan.permissions = console_permissions_for(&user);
    };
    let (vm_name, _host_id, source, k8s_namespace) = vm_meta(&state, id).await?;
    if source == "kubevirt" {
        let ns = k8s_namespace.unwrap_or_else(|| "default".into());
        let ws_token = state.ws_tokens.issue(id, read_only).await;
        let mut plan = kubevirt_plan_enriched(
            &state.pool,
            &state.config.daemon_base_url,
            id,
            &vm_name,
            &ns,
            &ws_token,
        )
        .await;
        policy(&mut plan);
        return Ok(Json(plan));
    }
    let (vm_name, host_id) = vm_row(&state, id).await?;
    let agent_addr = host_agent_grpc(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agent_plan = agent_client::get_console_access_plan(&mut client, &vm_name)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let spec_vm: Option<VirtualMachine> =
        crate::db::query_scalar("SELECT spec_json FROM vms WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok());
    let guest_access =
        build_guest_access_hints(&state.pool, host_id, &agent_plan, spec_vm.as_ref()).await;
    let hypervisor_address: Option<String> =
        crate::db::query_scalar("SELECT NULLIF(TRIM(address), '') FROM hosts WHERE id = ?")
            .bind(host_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
    let ws_token = state.ws_tokens.issue(id, read_only).await;
    let mut plan = plan_from_agent(
        id,
        &vm_name,
        &agent_plan,
        &ws_token,
        guest_access,
        hypervisor_address,
    );
    policy(&mut plan);
    Ok(Json(plan))
}

async fn check_console_rbac(
    _state: &AppState,
    user: &AuthUser,
    protocol: &str,
) -> Result<(), ApiError> {
    // Read-only roles (viewer/readonly) may only use protocols where the server can
    // enforce input suppression (the ws-token path issues a read_only token that drops
    // input frames). RDP and serial are inherently interactive — they carry a live
    // keyboard/mouse channel with no read-only mode — so both stay blocked for
    // read-only roles.
    if console_permissions_for(user).read_only {
        let interactive = protocol.contains("rdp") || protocol == "serial";
        if interactive {
            return Err(ApiError::bad_request(
                "read-only role cannot open interactive (RDP or serial) consoles",
            )
            .with_code("console_rbac")
            .with_remediation(
                "Request operator access, or use the read-only noVNC/serial viewer.",
            ));
        }
    }
    Ok(())
}

fn check_device_posture(posture_header: Option<&str>) -> Result<(), ApiError> {
    let required = std::env::var("CONSOLEHUB_REQUIRE_POSTURE")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if !required {
        return Ok(());
    }
    match posture_header.map(str::trim).filter(|s| !s.is_empty()) {
        Some("trusted") | Some("compliant") => Ok(()),
        _ => Err(ApiError::bad_request("device posture check failed")
            .with_code("console_posture_required")
            .with_remediation(
                "Connect from a managed device with valid Zeus posture attestation.",
            )),
    }
}

async fn check_jit_approval(
    state: &AppState,
    vm_id: Uuid,
    user: &AuthUser,
    protocol: &str,
) -> Result<(), ApiError> {
    if !state.config.consolehub_require_approval {
        return Ok(());
    }
    let approved: Option<Uuid> = crate::db::query_scalar(
        "SELECT id FROM console_access_requests WHERE vm_id = ? AND requester = ? AND protocol = ? AND status = 'approved' AND (expires_at IS NULL OR expires_at > datetime('now')) ORDER BY approved_at DESC LIMIT 1",
    )
    .bind(vm_id)
    .bind(&user.username)
    .bind(protocol)
    .fetch_optional(&state.pool)
    .await?;
    if approved.is_some() {
        return Ok(());
    }
    Err(ApiError::bad_request("console access requires approval")
        .with_code("console_approval_required")
        .with_remediation("Platform → Zeus → Approvals — request JIT console access for this VM."))
}

pub async fn create_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateSessionBody>,
) -> Result<Json<ConsoleSessionResponse>, ApiError> {
    check_federated_console_auth(&state, &user)?;
    let (_vm_name, host_id, source, k8s_namespace) = vm_meta(&state, id).await?;
    if source == "kubevirt" {
        let protocol = body
            .protocol
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "novnc".into());
        check_console_rbac(&state, &user, &protocol).await?;
        if !body.break_glass {
            check_jit_approval(&state, id, &user, &protocol).await?;
        } else {
            crate::auth::require_operator(&user)?;
        }
        check_device_posture(
            headers
                .get("x-zeus-device-posture")
                .and_then(|v| v.to_str().ok()),
        )?;
        let ws_token = state
            .ws_tokens
            .issue(id, console_permissions_for(&user).read_only)
            .await;
        let audit_id = Uuid::new_v4();
        let ttl = Duration::from_secs(state.config.consolehub_session_ttl_secs);
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl.as_secs() as i64);
        let session_id = state
            .console_sessions
            .insert(LiveConsoleSession {
                expires: Instant::now() + ttl,
                ws_token: ws_token.clone(),
            })
            .await;
        // Persist the session (and its audit event) the same way the native path
        // does. Without this row, `end_session` (which only knows the DB table)
        // can never find or revoke a KubeVirt console session — the in-memory
        // entry + its ws-token stayed live until natural TTL expiry with no way
        // for the owner (or an admin) to end it early, and no audit trail was
        // ever written for the session start.
        if let Err(e) = crate::db::query(
            "INSERT INTO console_sessions (id, vm_id, host_id, actor, protocol, backend, agent_proxy_base, expires_at, audit_id, recording_enabled)
             VALUES (?,?,?,?,?,'native','',?,?,FALSE)",
        )
        .bind(session_id)
        .bind(id)
        .bind(host_id)
        .bind(&user.username)
        .bind(&protocol)
        .bind(expires_at)
        .bind(audit_id)
        .execute(&state.pool)
        .await
        {
            let sessions = state.console_sessions.clone();
            let ws_tokens = state.ws_tokens.clone();
            tokio::spawn(async move {
                if let Some(token) = sessions.remove(session_id).await {
                    ws_tokens.revoke(&token).await;
                }
            });
            return Err(ApiError::internal(e.to_string()));
        }
        if let Err(e) = crate::db::query(
            "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
             VALUES (?,?,?,?,?,?)",
        )
        .bind(audit_id)
        .bind(&user.username)
        .bind("consolehub.session.start")
        .bind("vm")
        .bind(id)
        .bind(SqlxJson(serde_json::json!({ "protocol": protocol, "backend": "native", "session_id": session_id.to_string(), "kubevirt": true })))
        .execute(&state.pool)
        .await
        {
            let pool = state.pool.clone();
            let sessions = state.console_sessions.clone();
            let ws_tokens = state.ws_tokens.clone();
            tokio::spawn(async move {
                if let Some(token) = sessions.remove(session_id).await {
                    ws_tokens.revoke(&token).await;
                }
                let _ = crate::db::query("DELETE FROM console_sessions WHERE id = ?")
                    .bind(session_id)
                    .execute(&pool)
                    .await;
            });
            return Err(ApiError::internal(e.to_string()));
        }
        let ns = k8s_namespace.unwrap_or_else(|| "default".into());
        let embed_path = format!(
            "/platform/vms/{id}/consolehub?session={session_id}&native=1&kubevirt=1&namespace={}&token={ws_token}",
            urlencoding::encode(&ns)
        );
        return Ok(Json(ConsoleSessionResponse {
            session_id: session_id.to_string(),
            vm_id: id.to_string(),
            protocol,
            backend: "native".into(),
            embed_path,
            emergency_url: None,
            audit_id: audit_id.to_string(),
            expires_at: expires_at.to_rfc3339(),
            spectator_token: None,
            recording_enabled: false,
        }));
    }
    let (vm_name, host_id) = vm_row(&state, id).await?;
    let agent_addr = host_agent_grpc(&state.pool, host_id).await?;
    let mut client = agent_client::connect(&agent_addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agent_plan = agent_client::get_console_access_plan(&mut client, &vm_name)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let protocol = body
        .protocol
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| agent_plan.recommended.clone());
    // Without this, a caller could request e.g. "rdp" for a Linux VM, or for a
    // Windows VM with RDP unreachable, and get a 200 with a usable-looking
    // session — there's no browser RDP renderer at all (Guacamole was removed),
    // so the caller has no other signal the session doesn't actually work.
    let available = build_protocol_list(&agent_plan);
    if !available.iter().any(|p| p == &protocol) {
        return Err(ApiError::bad_request(format!(
            "protocol '{protocol}' is not available for this VM (available: {})",
            available.join(", ")
        )));
    }

    check_console_rbac(&state, &user, &protocol).await?;
    if !body.break_glass {
        check_jit_approval(&state, id, &user, &protocol).await?;
    } else {
        crate::auth::require_operator(&user)?;
    }
    check_device_posture(
        headers
            .get("x-zeus-device-posture")
            .and_then(|v| v.to_str().ok()),
    )?;

    if body.break_glass {
        crate::db::query(
            "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
             VALUES (?,?,?,?,?,?)",
        )
        .bind(Uuid::new_v4())
        .bind(&user.username)
        .bind("consolehub.break_glass")
        .bind("vm")
        .bind(id)
        .bind(SqlxJson(serde_json::json!({ "protocol": protocol })))
        .execute(&state.pool)
        .await?;
    }

    let ws_token = state
        .ws_tokens
        .issue(id, console_permissions_for(&user).read_only)
        .await;
    let audit_id = Uuid::new_v4();
    let ttl = Duration::from_secs(state.config.consolehub_session_ttl_secs);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl.as_secs() as i64);

    let agent_console = host_agent_console(&state.pool, host_id).await?;
    let agent_proxy = format!(
        "http://{}",
        agent_client::normalize_agent_addr(&agent_console)
    );

    // Every console session is native.
    let (backend, guac_token, emergency_url): (String, Option<String>, Option<String>) =
        (String::from("native"), None, None);

    let session_id = state
        .console_sessions
        .insert(LiveConsoleSession {
            expires: Instant::now() + ttl,
            ws_token: ws_token.clone(),
        })
        .await;

    let recording = state.config.consolehub_recording_enabled || body.break_glass;
    crate::db::query(
        "INSERT INTO console_sessions (id, vm_id, host_id, actor, protocol, backend, guac_token, agent_proxy_base, emergency_url, expires_at, audit_id, recording_enabled)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(session_id)
    .bind(id)
    .bind(host_id)
    .bind(&user.username)
    .bind(&protocol)
    .bind(&backend)
    .bind(guac_token.as_deref())
    .bind(&agent_proxy)
    .bind(emergency_url.as_deref())
    .bind(expires_at)
    .bind(audit_id)
    .bind(recording)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        let sessions = state.console_sessions.clone();
        let ws_tokens = state.ws_tokens.clone();
        tokio::spawn(async move {
            if let Some(token) = sessions.remove(session_id).await {
                ws_tokens.revoke(&token).await;
            }
        });
        ApiError::internal(e.to_string())
    })?;

    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
         VALUES (?,?,?,?,?,?)",
    )
    .bind(audit_id)
    .bind(&user.username)
    .bind("consolehub.session.start")
    .bind("vm")
    .bind(id)
    .bind(SqlxJson(serde_json::json!({ "protocol": protocol, "backend": backend, "session_id": session_id.to_string() })))
    .execute(&state.pool)
    .await
    .map_err(|e| {
        // Roll back the just-created session if the audit insert fails. Without
        // this the DB-insert path was cleaned up but the audit path was not,
        // leaking a live, proxyable console session (in-memory entry + DB row +
        // the provisioned session + its ws-token) while returning an error to
        // the caller.
        let pool = state.pool.clone();
        let sessions = state.console_sessions.clone();
        let ws_tokens = state.ws_tokens.clone();
        tokio::spawn(async move {
            if let Some(token) = sessions.remove(session_id).await {
                ws_tokens.revoke(&token).await;
            }
            let _ = crate::db::query("DELETE FROM console_sessions WHERE id = ?")
                .bind(session_id)
                .execute(&pool)
                .await;
        });
        ApiError::internal(e.to_string())
    })?;

    let spectator_token = Uuid::new_v4().to_string();
    if recording {
        // Propagate a failure here instead of discarding it: the response below
        // claims `recording_enabled: true` and hands back this exact
        // spectator_token, but join_spectator_session (see below) authorizes
        // solely by matching `spectator_token` in the DB row. A silently
        // dropped UPDATE would hand the caller a token that can never
        // authenticate a spectate — a false success.
        crate::db::query("UPDATE console_sessions SET spectator_token = ? WHERE id = ?")
            .bind(&spectator_token)
            .bind(session_id)
            .execute(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    let embed_path =
        format!("/platform/vms/{id}/consolehub?session={session_id}&native=1&token={ws_token}");

    Ok(Json(ConsoleSessionResponse {
        session_id: session_id.to_string(),
        vm_id: id.to_string(),
        protocol,
        backend,
        embed_path,
        emergency_url,
        audit_id: audit_id.to_string(),
        expires_at: expires_at.to_rfc3339(),
        spectator_token: if recording {
            Some(spectator_token)
        } else {
            None
        },
        recording_enabled: recording,
    }))
}

fn session_replay_path(state: &AppState, session_id: Uuid) -> std::path::PathBuf {
    state
        .config
        .consolehub_recording_dir
        .join(format!("{session_id}.webm"))
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    require_operator(&actor)?;
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        bool,
        Option<String>,
    )> = crate::db::query_as(
        "SELECT id, actor, protocol, backend, started_at, ended_at, recording_enabled, recording_path FROM console_sessions WHERE vm_id = ? ORDER BY started_at DESC LIMIT 50",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(sid, actor, protocol, backend, started, ended, recording, recording_path)| {
                    let replay_available = recording_path
                        .as_ref()
                        .and_then(|p| {
                            let path = std::path::Path::new(p);
                            path.exists().then_some(path.to_string_lossy().to_string())
                        })
                        .is_some();
                    serde_json::json!({
                        "session_id": sid.to_string(),
                        "actor": actor,
                        "protocol": protocol,
                        "backend": backend,
                        "started_at": started.to_rfc3339(),
                        "ended_at": ended.map(|t| t.to_rfc3339()),
                        "recording_enabled": recording,
                        "recording_path": recording_path,
                        "replay_available": replay_available,
                    })
                },
            )
            .collect(),
    ))
}

/// Validate a spectator token for read-only console viewing (recorded sessions).
pub async fn validate_spectator(
    State(_state): State<AppState>,
    axum::extract::Query(q): Query<SpectatorValidateQuery>,
) -> Result<Json<SpectatorValidateResponse>, ApiError> {
    let row: Option<(Uuid, String, String)> = crate::db::query_as(
        "SELECT vm_id, actor, protocol FROM console_sessions WHERE id = ? AND spectator_token = ? AND ended_at IS NULL AND recording_enabled = TRUE",
    )
    .bind(q.session_id)
    .bind(q.token.trim())
    .fetch_optional(&_state.pool)
    .await?;

    let Some((vm_id, actor, protocol)) = row else {
        return Ok(Json(SpectatorValidateResponse {
            valid: false,
            vm_id: String::new(),
            actor: String::new(),
            protocol: String::new(),
            read_only: true,
        }));
    };

    Ok(Json(SpectatorValidateResponse {
        valid: true,
        vm_id: vm_id.to_string(),
        actor,
        protocol,
        read_only: true,
    }))
}

#[derive(Debug, Deserialize)]
pub struct BreakGlassBody {
    pub protocol: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct CollaborateBody {
    pub protocol: Option<String>,
    #[serde(default)]
    pub reason: String,
}

/// Issue a read-only spectator link for collaborative console viewing.
pub async fn collaborate_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<CollaborateBody>,
) -> Result<Json<ConsoleSessionResponse>, ApiError> {
    crate::auth::require_operator(&user)?;
    let (_, host_id, _, _) = vm_meta(&state, id).await?;
    let protocol = body
        .protocol
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "novnc".into());
    let session_id = Uuid::new_v4();
    let spectator_token = Uuid::new_v4().to_string();
    let audit_id = Uuid::new_v4();
    let ttl = Duration::from_secs(state.config.consolehub_session_ttl_secs);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl.as_secs() as i64);

    crate::db::query(
        "INSERT INTO console_sessions (id, vm_id, host_id, actor, protocol, backend, expires_at, audit_id, recording_enabled, spectator_token, metadata_json)
         VALUES (?,?,?,?,?,'native',?,?,TRUE,?,?)",
    )
    .bind(session_id)
    .bind(id)
    .bind(host_id)
    .bind(&user.username)
    .bind(&protocol)
    .bind(expires_at)
    .bind(audit_id)
    .bind(&spectator_token)
    .bind(SqlxJson(serde_json::json!({
        "collaborate": true,
        "reason": body.reason,
    })))
    .execute(&state.pool)
    .await?;

    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
         VALUES (?,?,?,?,?,?)",
    )
    .bind(audit_id)
    .bind(&user.username)
    .bind("consolehub.collaborate")
    .bind("vm")
    .bind(id)
    .bind(SqlxJson(serde_json::json!({
        "protocol": protocol,
        "session_id": session_id.to_string(),
        "reason": body.reason,
    })))
    .execute(&state.pool)
    .await?;

    let share_path = format!(
        "/platform/vms/{id}/consolehub?mode=cinema&session={session_id}&spectator={}",
        urlencoding::encode(&spectator_token)
    );

    Ok(Json(ConsoleSessionResponse {
        session_id: session_id.to_string(),
        vm_id: id.to_string(),
        protocol,
        backend: "native".into(),
        embed_path: share_path.clone(),
        emergency_url: None,
        audit_id: audit_id.to_string(),
        expires_at: expires_at.to_rfc3339(),
        spectator_token: Some(spectator_token),
        recording_enabled: true,
    }))
}

/// Break-glass console access with mandatory audit + recording (Phase 5).
pub async fn break_glass_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<BreakGlassBody>,
) -> Result<Json<ConsoleSessionResponse>, ApiError> {
    create_session(
        State(state),
        Extension(user),
        headers,
        Path(id),
        Json(CreateSessionBody {
            protocol: Some(body.protocol),
            break_glass: true,
        }),
    )
    .await
}

pub async fn end_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let replay_path = session_replay_path(&state, session_id)
        .to_string_lossy()
        .into_owned();
    // Admins may force-end ANY live session (e.g. the owning account is
    // disabled/compromised); reuse the same role check the rest of this
    // codebase uses instead of comparing `user.role` inline.
    let is_admin = crate::auth::require_admin(&user).is_ok();
    let res = if is_admin {
        crate::db::query(
            "UPDATE console_sessions SET ended_at = datetime('now'),
             recording_path = CASE WHEN recording_enabled THEN ? ELSE recording_path END
             WHERE id = ?",
        )
        .bind(&replay_path)
        .bind(session_id)
        .execute(&state.pool)
        .await?
    } else {
        crate::db::query(
            "UPDATE console_sessions SET ended_at = datetime('now'),
             recording_path = CASE WHEN recording_enabled THEN ? ELSE recording_path END
             WHERE id = ? AND actor = ?",
        )
        .bind(&replay_path)
        .bind(session_id)
        .bind(&user.username)
        .execute(&state.pool)
        .await?
    };
    // Also drop the in-memory session AND revoke its ws-token: the
    // vnc/serial/spice proxies (console.rs) authorize purely off
    // `state.ws_tokens`, not this session store, so removing only the
    // session left the token — embedded in this session's embed_path —
    // proxyable for the rest of its TTL even after "ending" the session.
    // Only when the caller owned the session (or was an admin) actually
    // ended it (rows_affected > 0), matching the DB guard.
    if res.rows_affected() > 0 {
        if let Some(token) = state.console_sessions.remove(session_id).await {
            state.ws_tokens.revoke(&token).await;
        }
        return Ok(Json(
            serde_json::json!({ "ended": true, "session_id": session_id.to_string() }),
        ));
    }
    // rows_affected == 0: don't silently no-op. Report a clean 404 when the
    // session_id genuinely doesn't exist, vs 403 when it exists but the
    // caller doesn't own it (non-admins only — the admin query above already
    // has no actor filter, so 0 rows there can only mean "not found").
    if is_admin {
        return Err(ApiError::not_found("console session not found"));
    }
    let exists: Option<i64> = crate::db::query_scalar("SELECT 1 FROM console_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_some() {
        Err(ApiError::forbidden(
            "cannot end a console session owned by another user",
        ))
    } else {
        Err(ApiError::not_found("console session not found"))
    }
}

pub async fn upload_session_replay(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(session_id): Path<Uuid>,
    body: Body,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row: Option<(bool, String)> =
        crate::db::query_as("SELECT recording_enabled, actor FROM console_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((recording_enabled, actor)) = row else {
        return Err(ApiError::not_found("session not found"));
    };
    if !recording_enabled {
        return Err(ApiError::bad_request("session was not recorded"));
    }
    if actor != user.username {
        crate::auth::require_operator(&user)?;
    }

    let bytes = axum::body::to_bytes(body, 256 * 1024 * 1024)
        .await
        .map_err(|_| ApiError::bad_request("replay payload too large"))?;
    if bytes.is_empty() {
        return Err(ApiError::bad_request("empty replay payload"));
    }

    let path = session_replay_path(&state, session_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::internal(format!("create recording dir: {e}")))?;
    }
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| ApiError::internal(format!("write replay: {e}")))?;

    let path_str = path.to_string_lossy().into_owned();
    crate::db::query("UPDATE console_sessions SET recording_path = ? WHERE id = ?")
        .bind(&path_str)
        .bind(session_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({
        "uploaded": true,
        "session_id": session_id.to_string(),
        "recording_path": path_str,
        "bytes": bytes.len(),
    })))
}

pub async fn get_session_replay(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(session_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    require_operator(&actor)?;
    let recording_path: Option<String> =
        crate::db::query_scalar("SELECT recording_path FROM console_sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&state.pool)
            .await?
            .flatten();
    let Some(path_str) = recording_path else {
        return Err(ApiError::not_found("replay not available"));
    };
    let path = std::path::Path::new(&path_str);
    if !path.is_file() {
        return Err(ApiError::not_found("replay file missing"));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ApiError::internal(format!("read replay: {e}")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("video/webm"),
        )
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("inline; filename=\"console-{session_id}.webm\""))
                .unwrap_or_else(|_| HeaderValue::from_static("inline")),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal(format!("build response: {e}")))
}

#[derive(Debug, Deserialize)]
pub struct AccessRequestBody {
    pub protocol: String,
    #[serde(default)]
    pub reason: String,
}

pub async fn create_access_request(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AccessRequestBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let request_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO console_access_requests (id, vm_id, requester, protocol, reason, status, expires_at)
         VALUES (?,?,?,?,?,'pending', datetime('now', '+24 hours'))",
    )
    .bind(request_id)
    .bind(id)
    .bind(&user.username)
    .bind(&body.protocol)
    .bind(body.reason.trim())
    .execute(&state.pool)
    .await?;
    Ok(Json(serde_json::json!({
        "request_id": request_id.to_string(),
        "status": "pending",
        "message": "JIT console access request submitted — await approval in Zeus → Approvals."
    })))
}

pub async fn approve_access_request(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(request_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::auth::require_operator(&user)?;
    let updated = crate::db::query(
        "UPDATE console_access_requests SET status = 'approved', approved_by = ?, approved_at = datetime('now'), expires_at = datetime('now', '+4 hours')
         WHERE id = ? AND status = 'pending'",
    )
    .bind(&user.username)
    .bind(request_id)
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiError::bad_request(
            "request not found or already processed",
        ));
    }
    Ok(Json(
        serde_json::json!({ "approved": true, "request_id": request_id.to_string() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ConsoleExplainBody {
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub lens: String,
    #[serde(default)]
    pub guest_ip: Option<String>,
    #[serde(default)]
    pub vm_state: Option<String>,
    #[serde(default)]
    pub screen_snapshot: Option<String>,
}

pub async fn consolehub_explain(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<ConsoleExplainBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    let (vm_name, _host_id) = vm_row(&state, id).await?;
    let intent = if body.intent.is_empty() {
        "explain_screen"
    } else {
        body.intent.as_str()
    };
    let mut lines = vec![format!("**{}** — ConsoleHub lens: {}", vm_name, body.lens)];
    if let Some(ip) = &body.guest_ip {
        if !ip.is_empty() {
            lines.push(format!(
                "Guest IP visible: `{ip}` — try SSH from the host when sshd is up."
            ));
        }
    }
    if let Some(st) = &body.vm_state {
        lines.push(format!("VM state: {st}"));
    }
    match intent {
        "diagnose_boot" => {
            lines.push("If the display is black, open **Serial** for boot output.".into());
            lines.push(
                "Common causes: missing virtio drivers, wrong root device, cloud-init failure."
                    .into(),
            );
        }
        "fix_network" => {
            lines.push("Check guest NIC, cloud-init network config, and host/CNI routes.".into());
            lines.push(
                "Use **Network** lens or a native eBPF capture for packet-level detail.".into(),
            );
        }
        _ => {
            lines.push(
                "Analyze the visible console for login prompts, installers, or error screens."
                    .into(),
            );
            if body.guest_ip.as_deref().unwrap_or("").is_empty() {
                lines.push("No guest IP reported — network may still be initializing.".into());
            } else {
                lines.push(
                    "Guest appears to have network — console and SSH should be reachable.".into(),
                );
            }
        }
    }
    let object_ref = serde_json::json!({ "vm_id": id.to_string(), "intent": intent });
    if let Ok(text) =
        crate::engine::ai::explain_screen(&state.pool, "console_hub", &object_ref).await
    {
        lines.push(text);
    }
    Ok(Json(
        serde_json::json!({ "explanation": lines.join("\n\n") }),
    ))
}

pub fn console_info_from_plan(plan: &ConsoleHubPlan) -> serde_json::Value {
    serde_json::json!({
        "vm_id": plan.vm_id,
        "vm_name": plan.vm_name,
        "console_type": plan.native.console_type,
        "ws_path": plan.native.ws_path,
        "consolehub": plan,
    })
}
