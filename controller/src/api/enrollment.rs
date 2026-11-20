// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_admin, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct EnrollmentTokenResponse {
    pub token: String,
    pub expires_at: String,
    pub install_command: String,
    /// Present when the controller's TLS join listener is on: the command that pins the
    /// controller by its CA fingerprint and sets the node up for mutual TLS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_command: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEnrollmentRequest {
    #[serde(default)]
    pub ttl_hours: i64,
    /// Join this cluster instead of the first one.
    #[serde(default)]
    pub cluster_id: Option<Uuid>,
    /// Placement set on the host when it joins.
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub rack: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Existing networks / storage pools (by name) to set up on the host once it validates.
    #[serde(default)]
    pub networks: Vec<String>,
    #[serde(default)]
    pub storage_pools: Vec<String>,
}

pub async fn create_enrollment_token(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(req): Json<CreateEnrollmentRequest>,
) -> Result<Json<EnrollmentTokenResponse>, ApiError> {
    require_admin(&actor)?;
    const MAX_TTL_HOURS: i64 = 720; // 30 days
    let ttl = if req.ttl_hours <= 0 {
        24
    } else if req.ttl_hours > MAX_TTL_HOURS {
        return Err(ApiError::bad_request("ttl_hours must be <= 720 (30 days)"));
    } else {
        req.ttl_hours
    };
    let token = format!("join-{}", Uuid::new_v4());
    let expires = Utc::now() + Duration::hours(ttl);
    let cluster_id: Uuid = match req.cluster_id {
        Some(c) => crate::db::query_scalar("SELECT id FROM clusters WHERE id = ?")
            .bind(c)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("cluster_id does not exist"))?,
        None => crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::internal("no cluster configured"))?,
    };
    let tags = crate::engine::host_post_join::clean_names(&req.tags).map_err(ApiError::bad_request)?;
    let networks =
        crate::engine::host_post_join::clean_names(&req.networks).map_err(ApiError::bad_request)?;
    let pools = crate::engine::host_post_join::clean_names(&req.storage_pools)
        .map_err(ApiError::bad_request)?;
    for (what, v) in [("site", &req.site), ("rack", &req.rack)] {
        if v.len() > 64 || v.chars().any(|c| c.is_control()) {
            return Err(ApiError::bad_request(format!("invalid {what}")));
        }
    }

    crate::db::query(
        "INSERT INTO enrollment_tokens
         (token, cluster_id, expires_at, site, rack, tags, networks, storage_pools)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(cluster_id)
    .bind(expires)
    .bind(req.site.trim())
    .bind(req.rack.trim())
    .bind(serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&networks).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&pools).unwrap_or_else(|_| "[]".into()))
    .execute(&state.pool)
    .await?;

    // Use the configured public URL (MACHINA_PUBLIC_URL) for the join command —
    // `state.config.host` is the bind address, which is typically 0.0.0.0 and thus
    // not a usable URL for an agent to reach the controller.
    let controller_base = state
        .config
        .public_base_url
        .trim_end_matches('/')
        .to_string();
    let (install_command, join_command) = join_commands(&controller_base, &token);

    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, detail)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(&actor.username)
    .bind("enrollment.create")
    .bind("enrollment")
    .bind(serde_json::json!({ "token_prefix": &token[..12.min(token.len())] }))
    .execute(&state.pool)
    .await?;

    super::join_events::prune(&state.pool).await;
    super::join_events::record(
        &state.pool,
        &token,
        None,
        "info",
        "token",
        &format!("enrollment token issued by {}, valid {ttl} h", actor.username),
    )
    .await;
    Ok(Json(EnrollmentTokenResponse {
        token,
        expires_at: expires.to_rfc3339(),
        install_command,
        join_command,
    }))
}

/// The commands a machine runs to join: the plain one, and (with the network TLS listener on)
/// the one that pins the controller by its CA fingerprint and checks the script's checksum.
pub fn join_commands(controller_base: &str, token: &str) -> (String, Option<String>) {
    let install_command = format!(
        "curl -fsSL {controller_base}/install.sh | sudo bash -s -- --controller {controller_base} --token {token}"
    );
    // With the network (TLS) listener on, the node pins the controller by its CA fingerprint.
    let join_command = crate::enrollment_tls::configured_addr().and_then(|tls_addr| {
        let url = crate::enrollment_tls::public_https_url(controller_base, &tls_addr)?;
        let (_, fp) = crate::pki::ca_info().ok()?;
        let sum = install_script_sha256();
        // The script is fetched without trusting the certificate, so its checksum is in the
        // command; the script then verifies the controller by the pinned CA fingerprint.
        Some(format!(
            "curl -fsSk {url}/install.sh -o /tmp/machina-install.sh && echo \"{sum}  /tmp/machina-install.sh\" | sha256sum -c - && sudo bash /tmp/machina-install.sh --controller {url} --ca-sha256 {fp} --token {token}"
        ))
    });
    (install_command, join_command)
}

/// The bootstrap a bare machine runs (see `install.sh`); served as text, unauthenticated, and
/// carries no secrets: the token and CA fingerprint come from the command line.
pub const INSTALL_SH: &str = include_str!("install.sh");

pub async fn install_script(
) -> Result<([(axum::http::header::HeaderName, &'static str); 1], String), ApiError> {
    Ok((
        [(axum::http::header::CONTENT_TYPE, "text/x-shellscript")],
        INSTALL_SH.to_string(),
    ))
}

/// SHA-256 of the served script: the Add Host command checks it before running anything.
pub fn install_script_sha256() -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(INSTALL_SH.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EnrollmentTokenRow {
    pub token: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_enrollment_tokens(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<EnrollmentTokenRow>>, ApiError> {
    require_admin(&actor)?;
    let rows = crate::db::query_as::<_, EnrollmentTokenRow>(
        "SELECT token, expires_at, used_at, created_at FROM enrollment_tokens
         ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn revoke_enrollment_token(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(&actor)?;
    let revoked = crate::db::query("DELETE FROM enrollment_tokens WHERE token = ? AND used_at IS NULL")
        .bind(&token)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if revoked == 0 {
        return Err(ApiError::not_found(
            "enrollment token not found or already used",
        ));
    }
    Ok(Json(serde_json::json!({ "revoked": true })))
}

#[cfg(test)]
mod install_script_tests {
    use super::*;

    #[test]
    fn the_script_verifies_before_it_installs_and_hides_the_token() {
        assert!(INSTALL_SH.starts_with("#!/usr/bin/env bash\n"));
        for must in ["set -euo pipefail", "SHA256SUMS", "--ca-sha256", "sha256sum", "<hidden>", "[ \"$have\" = \"$want\" ]"] {
            assert!(INSTALL_SH.contains(must), "install.sh must contain {must}");
        }
        assert_eq!(install_script_sha256().len(), 64);
        assert_eq!(install_script_sha256(), install_script_sha256());
    }
}
