// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Thin authenticated gateway from Machina to Transiva/h2kvm's existing web API.
//! This keeps migration/conversion ownership in Transiva while giving Machina a
//! single control-plane surface for provider discovery, preview and job submission.

use std::time::Duration;

use axum::extract::State;
use axum::{Extension, Json};
use reqwest::Method;
use serde_json::Value;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

fn endpoint() -> Result<String, ApiError> {
    let raw =
        std::env::var("MACHINA_TRANSIVA_URL").unwrap_or_else(|_| "http://127.0.0.1:5070".into());
    let raw = raw.trim().trim_end_matches('/');
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return Err(ApiError::bad_request(
            "MACHINA_TRANSIVA_URL must start with http:// or https://",
        ));
    }
    Ok(raw.to_string())
}

pub(crate) async fn call(method: Method, path: &str, body: Option<Value>) -> Result<Json<Value>, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(
            std::env::var("MACHINA_TRANSIVA_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        ))
        .build()
        .map_err(|e| ApiError::internal(format!("Transiva client: {e}")))?;
    let mut req = client.request(method, format!("{}{}", endpoint()?, path));
    if let Ok(key) = std::env::var("MACHINA_TRANSIVA_API_KEY") {
        if !key.trim().is_empty() {
            req = req.header("X-API-Key", key);
        }
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("Transiva request failed: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| ApiError::internal(format!("Transiva response: {e}")))?;
    if !status.is_success() {
        return Err(ApiError::internal(format!(
            "Transiva returned {}: {}",
            status,
            text.chars().take(2048).collect::<String>()
        ))
        .with_remediation(
            "Check MACHINA_TRANSIVA_URL, MACHINA_TRANSIVA_API_KEY and h2kweb health",
        ));
    }
    let value = if text.trim().is_empty() {
        serde_json::json!({"status":"ok"})
    } else {
        serde_json::from_str(&text)
            .map_err(|e| ApiError::internal(format!("Transiva returned invalid JSON: {e}")))?
    };
    Ok(Json(value))
}

pub async fn status(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    call(Method::GET, "/api/v1/health", None).await
}

pub async fn providers(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    call(Method::GET, "/api/v1/providers/", None).await
}

pub async fn preview(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    call(Method::POST, "/api/v1/migrations/preview", Some(body)).await
}

pub async fn submit(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    call(Method::POST, "/api/v1/migrations/submit", Some(body)).await
}

pub async fn batch(
    State(_state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_operator(&actor)?;
    call(Method::POST, "/api/v1/migrations/batch", Some(body)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_requires_http_scheme() {
        std::env::set_var("MACHINA_TRANSIVA_URL", "ftp://bad");
        assert!(endpoint().is_err());
        std::env::set_var("MACHINA_TRANSIVA_URL", "http://127.0.0.1:5070/");
        assert_eq!(endpoint().unwrap(), "http://127.0.0.1:5070");
        std::env::remove_var("MACHINA_TRANSIVA_URL");
    }
}
