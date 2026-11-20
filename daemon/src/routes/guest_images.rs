// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! OS catalog, install-media detection, and RHEL image URL (Cockpit-machines-style helpers).

use axum::extract::{Extension, Json};
use axum::routing::{get, post};
use axum::Router;
use machina_core::{LibvirtError, LibvirtManager};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::warn;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;

/// Applies to both the OS install-tree probe (`osinfo-detect`) and the RHEL
/// image-URL lookup (`curl`): both are short request/response calls against a
/// caller-influenced (or fixed, but network-dependent) endpoint. Without a
/// timeout, a target host that accepts a connection but never responds ties up
/// this call indefinitely; matches the general-purpose external-command
/// timeout (`KUBECTL_TIMEOUT_SECS`) used in `routes/k8s.rs` for the same class
/// of "single external command, bounded wait" call.
const EXTERNAL_FETCH_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
struct OsDetectBody {
    url: String,
}

#[derive(Debug, Deserialize)]
struct RhelUrlBody {
    access_token: String,
    #[serde(default = "default_rhel_ver")]
    rhel_version: String,
    #[serde(default = "default_arch")]
    arch: String,
}

fn default_rhel_ver() -> String {
    "9".to_string()
}

fn default_arch() -> String {
    "x86_64".to_string()
}

/// `osinfo-query os -f short-id,name,version` when available.
async fn os_list_handler() -> Result<Json<serde_json::Value>, AppError> {
    let result = Command::new("osinfo-query")
        .args(["os", "-f", "short-id,name,version"])
        .output()
        .await;

    // Degrade gracefully when osinfo-query can't even be spawned (libosinfo not
    // installed → io::ErrorKind::NotFound). Previously this mapped to a 500, which made
    // the Create VM page's OS dropdown fail to load. Return the same empty-list + hint
    // fallback used below for a non-zero exit, so the page still works.
    let out = match result {
        Ok(o) => o,
        Err(e) => {
            warn!("osinfo-query unavailable ({e}); returning empty OS list");
            let empty: Vec<serde_json::Value> = Vec::new();
            return Ok(Json(json!({
                "oses": empty,
                "hint": "Install libosinfo (`osinfo-db` / `libosinfo`) for a full OS list."
            })));
        }
    };
    if !out.status.success() {
        warn!(
            "osinfo-query failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let empty: Vec<serde_json::Value> = Vec::new();
        return Ok(Json(json!({
            "oses": empty,
            "hint": "Install libosinfo (`osinfo-db` / `libosinfo`) for a full OS list."
        })));
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cols.len() >= 3 {
            rows.push(json!({
                "short_id": cols[0],
                "name": cols[1],
                "version": cols[2],
            }));
        }
    }
    Ok(Json(json!({ "oses": rows })))
}

/// Run `osinfo-detect --type=tree <url>` (HTTP install tree).
///
/// This makes the daemon (running as root) fetch and parse an operator-supplied
/// URL, so it is gated like any other write/mutating operation and subject to
/// the same SSRF guard as the ISO-download feature (`assert_public_http_host`):
/// only plain `http(s)` URLs are accepted, and the resolved host must not be
/// loopback/private/link-local — otherwise this becomes an internal-network
/// probe or, via `file://`, a local-file-disclosure primitive.
async fn os_detect_handler(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<OsDetectBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_write(&actor, "vms:write")?;

    let url = body.url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::from(LibvirtError::Invalid(
            "URL must start with http:// or https://".into(),
        )));
    }
    machina_core::iso_upload::assert_public_http_host(&url).await?;

    let mut cmd = Command::new("osinfo-detect");
    cmd.args(["--type=tree", &url]);
    let out = timeout(
        Duration::from_secs(EXTERNAL_FETCH_TIMEOUT_SECS),
        cmd.output(),
    )
    .await
    .map_err(|_| AppError::from(LibvirtError::Operation("osinfo-detect timed out".into())))?
    .map_err(|e| AppError::from(LibvirtError::Operation(format!("osinfo-detect: {e}"))))?;

    Ok(Json(json!({
        "exit_code": out.status.code(),
        "stdout": String::from_utf8_lossy(&out.stdout).trim(),
        "stderr": String::from_utf8_lossy(&out.stderr).trim(),
    })))
}

/// Resolve RHEL KVM guest image URL via Red Hat API ( bearer token ).
async fn rhel_image_url_handler(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<RhelUrlBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Same class as os_detect_handler above: the daemon shells out to make an
    // outbound network call on the caller's behalf. Gate it the same way
    // instead of leaving it reachable by any authenticated (incl. read-only) user.
    require_write(&actor, "vms:write")?;
    let api = format!(
        "https://api.access.redhat.com/management/v1/images/rhel/{}/{}/",
        body.rhel_version, body.arch
    );
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-f",
        "-H",
        &format!("Authorization: Bearer {}", body.access_token),
        &api,
    ]);
    let out = timeout(
        Duration::from_secs(EXTERNAL_FETCH_TIMEOUT_SECS),
        cmd.output(),
    )
    .await
    .map_err(|_| AppError::from(LibvirtError::Operation("curl timed out".into())))?
    .map_err(|e| AppError::from(LibvirtError::Operation(format!("curl: {e}"))))?;

    if !out.status.success() {
        // The command line (and thus stderr, which curl can echo back) carries the
        // bearer token via `-H`; never return raw stderr/command output to the
        // caller on failure, only a generic message logged server-side.
        warn!(
            "rhel_image_url_handler: curl failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        return Err(AppError::from(LibvirtError::Operation(
            "failed to resolve RHEL image URL from the Red Hat API".into(),
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!({}));
    Ok(Json(json!({ "raw": v })))
}

pub fn guest_image_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/guest-images/os-list", get(os_list_handler))
        .route("/guest-images/os-detect", post(os_detect_handler))
        .route("/guest-images/rhel-url", post(rhel_image_url_handler))
}
