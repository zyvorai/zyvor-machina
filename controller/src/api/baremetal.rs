// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;

use crate::api::ApiError;
use crate::auth::{require_admin, require_operator, AuthUser};
use crate::engine::{
    baremetal, baremetal_alloc, baremetal_boot, baremetal_deploy, baremetal_lifecycle,
};
use crate::state::AppState;

pub async fn list_servers(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<baremetal::BaremetalServer>>, ApiError> {
    require_operator(&actor)?;
    baremetal::list_servers(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}

pub async fn register_server(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<baremetal::RegisterBaremetalBody>,
) -> Result<Json<baremetal::BaremetalServer>, ApiError> {
    require_admin(&actor)?;
    if body.hostname.trim().is_empty() {
        return Err(ApiError::bad_request("hostname required"));
    }
    baremetal::register(&state.pool, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct CapacityPlanBody {
    pub query: String,
}

pub async fn capacity_plan(
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CapacityPlanBody>,
) -> Result<Json<baremetal::BaremetalCapacityPlan>, ApiError> {
    require_operator(&actor)?;
    Ok(Json(baremetal::plan_capacity(&body.query)))
}

pub async fn server_power(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<baremetal::BmcPowerBody>,
) -> Result<Json<baremetal::BmcPowerResult>, ApiError> {
    require_admin(&actor)?;
    baremetal::set_power(&state.pool, id, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

pub async fn server_provision(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<baremetal::BaremetalProvisionPlan>, ApiError> {
    require_admin(&actor)?;
    baremetal::provision_preview(&state.pool, id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub struct LifecycleBody {
    pub to: String,
}

/// Generic lifecycle transition. Commissioning goes through `commission` (it issues a token).
pub async fn server_lifecycle(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<LifecycleBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let to = baremetal_lifecycle::Lifecycle::parse(&body.to)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    if to == baremetal_lifecycle::Lifecycle::Commissioning {
        return Err(ApiError::bad_request(
            "use POST .../commission to start commissioning",
        ));
    }
    let to = baremetal_lifecycle::transition(&state.pool, id, to)
        .await
        .map_err(|e| ApiError::conflict(e.to_string(), "check the server's current lifecycle"))?;
    Ok(Json(
        serde_json::json!({ "id": id, "lifecycle": to.as_str() }),
    ))
}

/// Start commissioning and return the one-time token the machine must present.
pub async fn server_commission(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let token = baremetal_lifecycle::begin_commissioning(&state.pool, id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string(), "check the server's current lifecycle"))?;
    Ok(Json(serde_json::json!({
        "id": id,
        "lifecycle": "commissioning",
        "token": token,
        "report_path": format!("/api/v1/baremetal/commissioning/{id}/report"),
    })))
}

pub async fn server_inventory(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    match baremetal_lifecycle::inventory(&state.pool, id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Some(v) => Ok(Json(v)),
        None => Err(ApiError::not_found(
            "no inventory yet; commission the server",
        )),
    }
}

/// Unauthenticated by design: the commissioning image has no login. The bearer token
/// is the one issued by `server_commission`, single-use and bound to this server.
pub async fn commissioning_report(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    const MAX_REPORT: usize = 256 * 1024;
    if body.len() > MAX_REPORT {
        return Err(ApiError::bad_request("report too large"));
    }
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let report: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::bad_request("report must be a JSON object"))?;
    if !report.is_object() {
        return Err(ApiError::bad_request("report must be a JSON object"));
    }
    let to = baremetal_lifecycle::accept_report(&state.pool, id, token, &report)
        .await
        .map_err(|e| ApiError::forbidden(e.to_string()))?;
    Ok(Json(serde_json::json!({ "lifecycle": to.as_str() })))
}

#[derive(Debug, Deserialize)]
pub struct BootQuery {
    #[serde(default)]
    pub mac: String,
}

fn text(body: String) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// iPXE fetches this after chain-loading. A registered machine in `commissioning`
/// boots the commissioning image; everything else is told to exit to its own disk.
pub async fn boot_ipxe(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<BootQuery>,
) -> Result<axum::response::Response, ApiError> {
    let base = state
        .config
        .public_base_url
        .trim_end_matches('/')
        .to_string();
    match baremetal_lifecycle::boot_target(&state.pool, &q.mac)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Some((id, token)) => {
            // Pin the controller CA when there is one (the ramdisk verifies it by this).
            let ca = crate::pki::ca_info().ok().map(|(_, fp)| fp);
            Ok(text(baremetal_boot::ipxe_commission_script(
                &base,
                id,
                &token,
                &format!("{base}/dist"),
                ca.as_deref(),
            )))
        }
        None => {
            if let Some((id, os, token)) = baremetal_deploy::deploy_boot_target(&state.pool, &q.mac)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
            {
                let kind = baremetal_deploy::install_kind(&os)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                let repo = std::env::var("MACHINA_METAL_RHEL_REPO").unwrap_or_default();
                return Ok(text(baremetal_deploy::ipxe_deploy_script(
                    &base, id, &token, &os, kind, &repo,
                )));
            }
            Ok(text(baremetal_boot::ipxe_exit_script().to_string()))
        }
    }
}

/// Deploy an allocated server: allocated -> deploying, renders the installer document.
pub async fn server_deploy(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<baremetal_deploy::DeployRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let base = state
        .config
        .public_base_url
        .trim_end_matches('/')
        .to_string();
    baremetal_deploy::begin_deploy(&state.pool, id, &body, &base)
        .await
        .map_err(|e| {
            ApiError::conflict(
                e.to_string(),
                "the server must be allocated, have a mac_address, and the request must be valid",
            )
        })?;
    Ok(Json(serde_json::json!({
        "id": id,
        "lifecycle": "deploying",
        "os": body.os,
        "next": "PXE-boot the machine; it installs, reboots and joins as a host",
    })))
}

/// Installer documents. The token in the path is minted when the machine PXE-boots.
pub async fn install_document(
    State(state): State<AppState>,
    Path((id, token, name)): Path<(uuid::Uuid, String, String)>,
) -> Result<axum::response::Response, ApiError> {
    let body = match name.as_str() {
        "meta-data" => format!("instance-id: {id}\n"),
        "vendor-data" => String::new(),
        "user-data" | "ks.cfg" => baremetal_deploy::document(&state.pool, id, &token)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::not_found("no installer document"))?,
        _ => return Err(ApiError::not_found("no installer document")),
    };
    // meta-data / vendor-data carry nothing secret, but are still gated by the same token.
    if matches!(name.as_str(), "meta-data" | "vendor-data")
        && baremetal_deploy::document(&state.pool, id, &token)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_none()
    {
        return Err(ApiError::not_found("no installer document"));
    }
    Ok(text(body))
}

pub async fn boot_commission_script() -> axum::response::Response {
    text(baremetal_boot::COMMISSION_SCRIPT.to_string())
}

#[derive(Debug, Deserialize)]
pub struct DnsmasqQuery {
    pub subnet: String,
    #[serde(default = "default_tftp_root")]
    pub tftp_root: String,
    #[serde(default)]
    pub interface: String,
}

fn default_tftp_root() -> String {
    "/srv/tftp".into()
}

/// The dnsmasq configuration for a site's boot host.
pub async fn boot_dnsmasq(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    axum::extract::Query(q): axum::extract::Query<DnsmasqQuery>,
) -> Result<axum::response::Response, ApiError> {
    require_admin(&actor)?;
    let conf = baremetal_boot::dnsmasq_conf(&baremetal_boot::BootConfig {
        subnet: &q.subnet,
        tftp_root: &q.tftp_root,
        controller_url: &state.config.public_base_url,
        interface: &q.interface,
    })
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(text(conf))
}

pub async fn allocate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<baremetal_alloc::AllocateRequest>,
) -> Result<Json<baremetal_alloc::Allocation>, ApiError> {
    require_operator(&actor)?;
    baremetal_alloc::allocate(&state.pool, &body, &actor.username)
        .await
        .map_err(|e| ApiError::conflict(e.to_string(), "register or commission more servers"))
        .map(Json)
}

pub async fn release(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    baremetal_alloc::release(&state.pool, id)
        .await
        .map_err(|e| {
            ApiError::conflict(
                e.to_string(),
                "only allocated or deployed servers can be released",
            )
        })?;
    Ok(Json(serde_json::json!({ "id": id, "lifecycle": "ready" })))
}

#[derive(Debug, Deserialize)]
pub struct ZoneBody {
    pub zone: String,
}

pub async fn set_zone(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<ZoneBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    baremetal_alloc::set_zone(&state.pool, id, &body.zone)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "id": id, "zone": body.zone.trim() }),
    ))
}

pub async fn zones(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<baremetal_alloc::ZoneSummary>>, ApiError> {
    require_operator(&actor)?;
    baremetal_alloc::zones(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
        .map(Json)
}
