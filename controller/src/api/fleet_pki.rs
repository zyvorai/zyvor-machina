// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Public: the fleet CA certificate and its fingerprint, so a joining node can pin the controller.

use axum::Json;

use crate::api::ApiError;

pub async fn ca() -> Result<Json<serde_json::Value>, ApiError> {
    let (ca_pem, sha256) = crate::pki::ca_info().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "ca_pem": ca_pem, "sha256": sha256 }),
    ))
}

/// The CA certificate as plain PEM (for `curl` in the install script).
pub async fn ca_pem(
) -> Result<([(axum::http::header::HeaderName, &'static str); 1], String), ApiError> {
    let (pem, _) = crate::pki::ca_info().map_err(|e| ApiError::internal(e.to_string()))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/x-pem-file")],
        pem,
    ))
}
