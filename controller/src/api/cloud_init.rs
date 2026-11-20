// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::ApiError;

#[derive(Debug, Deserialize)]
pub struct ValidateCloudInitBody {
    pub user_data: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateCloudInitResponse {
    pub valid: bool,
    pub issues: Vec<String>,
    pub preview_hostname: Option<String>,
}

pub async fn validate_cloud_init(
    Json(body): Json<ValidateCloudInitBody>,
) -> Result<Json<ValidateCloudInitResponse>, ApiError> {
    let text = body.user_data.trim();
    let mut issues = Vec::new();
    if text.is_empty() {
        issues.push("user_data is empty".into());
    }
    if !text.contains("#cloud-config") && !text.starts_with("#cloud-config") {
        issues.push("Missing #cloud-config header (first line)".into());
    }
    // Flag EITHER plaintext-password form (chpasswd's `passwd:` or a `password:`
    // key). The old `&&` required both substrings, so a normal config with just
    // one never triggered the warning.
    if text.contains("passwd:") || text.contains("password:") || text.contains("chpasswd") {
        issues.push("Avoid plain-text passwords in user_data — use ssh_authorized_keys".into());
    }
    let preview_hostname = text
        .lines()
        .find(|l| l.trim_start().starts_with("hostname:"))
        .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(Json(ValidateCloudInitResponse {
        valid: issues.is_empty(),
        issues,
        preview_hostname,
    }))
}
