// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use machina_core::LibvirtError;
use serde_json::json;

pub fn ok_json(status: &str, name: &str) -> Json<serde_json::Value> {
    Json(json!({ "status": status, "name": name }))
}

/// Response wrapper that sets Content-Type: text/xml for XML endpoints.
pub struct Xml(pub String);

impl IntoResponse for Xml {
    fn into_response(self) -> Response {
        ([(header::CONTENT_TYPE, "text/xml; charset=utf-8")], self.0).into_response()
    }
}

pub struct AppError(LibvirtError);

impl From<LibvirtError> for AppError {
    fn from(err: LibvirtError) -> Self {
        Self(err)
    }
}

/// Classify an `Operation`/`Internal` libvirt message that would otherwise be a
/// blanket 500. Many libvirt failures are really client/state problems: the domain
/// is in the wrong power state, the guest agent is not up, or an argument exceeds a
/// hard limit. libvirt embeds a stable `code=<name>` token (and recognizable phrasing)
/// in its error text, which lets us return an actionable status instead of 500.
///
/// Returns `None` for genuinely internal failures, which keep the 500 default.
fn classify_operation_error(msg: &str) -> Option<(StatusCode, &'static str)> {
    let m = msg.to_ascii_lowercase();

    if m.contains("[vessel_unavailable]") {
        return Some((StatusCode::SERVICE_UNAVAILABLE, "vessel_unavailable"));
    }
    if m.contains("[vessel_unsupported]") {
        return Some((StatusCode::BAD_REQUEST, "vessel_unsupported"));
    }

    // Guest agent not connected / not responding → the request is valid but the
    // guest side isn't ready yet. 503 signals "retry once the guest is up".
    if m.contains("code=agentunresponsive")
        || m.contains("guest agent is not")
        || m.contains("guest agent is not responding")
        || m.contains("qemu guest agent is not connected")
    {
        return Some((StatusCode::SERVICE_UNAVAILABLE, "guest_agent_unavailable"));
    }

    // Wrong power state (domain not running / already running / not active) →
    // conflict with current resource state, not a server error.
    if m.contains("code=operationinvalid")
        || m.contains("domain is not running")
        || m.contains("domain is already running")
        || m.contains("domain is not active")
        || m.contains("domain is already active")
    {
        return Some((StatusCode::CONFLICT, "invalid_state"));
    }

    // Argument exceeds a hard domain limit (e.g. vcpus > maxvcpus) → bad request.
    if m.contains("code=invalidarg") || m.contains("greater than max allowable") {
        return Some((StatusCode::BAD_REQUEST, "invalid_argument"));
    }

    // q35 PCI topology full — client can retry after we hot-add a root port, but if
    // that still fails this is a capacity problem, not an internal crash.
    if m.contains("no more available pci slots") || m.contains("no more available pci slot") {
        return Some((StatusCode::CONFLICT, "pci_slots_exhausted"));
    }

    // Volume delete refused because a domain (live or offline) still references it —
    // see storage::delete_volume — a client/state conflict, not a server error.
    if m.contains("is still attached to:") {
        return Some((StatusCode::CONFLICT, "volume_in_use"));
    }

    None
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self.0 {
            LibvirtError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            LibvirtError::Invalid(msg) => (StatusCode::BAD_REQUEST, "invalid_request", msg.clone()),
            LibvirtError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg.clone()),
            LibvirtError::Connection(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "libvirt_connection",
                msg.clone(),
            ),
            LibvirtError::Operation(msg) => match classify_operation_error(msg) {
                Some((status, code)) => (status, code, msg.clone()),
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "operation_failed",
                    msg.clone(),
                ),
            },
            LibvirtError::Internal(msg) => match classify_operation_error(msg) {
                Some((status, code)) => (status, code, msg.clone()),
                None => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    msg.clone(),
                ),
            },
        };

        let body = axum::Json(json!({
            "error": message,
            "error_code": code,
        }));
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_unresponsive_maps_to_503() {
        let msg = "Failed to get guest hostname: error: Guest agent is not responding: \
                   QEMU guest agent is not connected [code=agentunresponsive (86), domain=qemu (10)]";
        let (s, c) = classify_operation_error(msg).expect("classified");
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(c, "guest_agent_unavailable");
    }

    #[test]
    fn wrong_power_state_maps_to_409() {
        let msg = "Failed to balloon memory for 'vm': error: Requested operation is not valid: \
                   domain is not running [code=operationinvalid (55), domain=domain (20)]";
        let (s, c) = classify_operation_error(msg).expect("classified");
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(c, "invalid_state");
    }

    #[test]
    fn exceeds_max_maps_to_400() {
        let msg = "Failed to set vCPUs for 'vm': error: invalid argument: requested vcpus is \
                   greater than max allowable vcpus for the persistent domain: 2 > 1 \
                   [code=invalidarg (8), domain=qemu (10)]";
        let (s, c) = classify_operation_error(msg).expect("classified");
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(c, "invalid_argument");
    }

    #[test]
    fn pci_slots_exhausted_maps_to_409() {
        let msg = "Failed to attach network interface: error: internal error: \
                   No more available PCI slots [code=internalerror (1), domain=qemu (10)]";
        let (s, c) = classify_operation_error(msg).expect("classified");
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(c, "pci_slots_exhausted");
    }

    #[test]
    fn unknown_operation_error_stays_500() {
        assert!(classify_operation_error("some unexpected libvirt failure").is_none());
    }
}
