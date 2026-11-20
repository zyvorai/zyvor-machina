// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native SSH keypair catalog — part of the external-cloud-client replacement ("Fleet
//! Cloud" native compute). Only the public key is ever stored (no private-key
//! generation here — Machina isn't in the business of handing out private keys
//! over an API); it's picked at instance-create time and injected via cloud-init
//! `ssh_authorized_keys`, the same mechanism `machina_spec::CloudInitSpec::ssh_pubkey`
//! already uses for a single key.

use axum::extract::{Path, State};
use axum::Extension;
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};
use crate::state::AppState;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KeypairRow {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    #[sqlx(skip)]
    pub ec2_id: String,
}

impl KeypairRow {
    fn with_id(mut self) -> Self {
        self.ec2_id = crate::resource_ids::ec2_id(crate::resource_ids::Kind::KeyPair, self.id);
        self
    }
}

const KEYPAIR_SELECT: &str = "SELECT id, project_id, name, public_key, fingerprint FROM keypairs";

pub async fn list_keypairs(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
) -> Result<Json<Vec<KeypairRow>>, ApiError> {
    require_operator(&actor)?;
    let rows = crate::db::query_as::<_, KeypairRow>(&format!("{KEYPAIR_SELECT} ORDER BY name"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows.into_iter().map(KeypairRow::with_id).collect()))
}

pub async fn get_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<KeypairRow>, ApiError> {
    require_operator(&actor)?;
    let row = crate::db::query_as::<_, KeypairRow>(&format!("{KEYPAIR_SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row.with_id()))
}

#[derive(Debug, Deserialize)]
pub struct CreateKeypairBody {
    pub name: String,
    pub public_key: String,
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

/// Standard `ssh-keygen -l -E sha256` format: SHA-256 of the raw (base64-decoded)
/// key blob, re-encoded as unpadded base64 — computed from the key bytes, not the
/// ASCII line, so two equivalent keys with different comments/whitespace fingerprint
/// identically.
fn ssh_fingerprint(public_key: &str) -> Result<String, ApiError> {
    let blob_b64 = public_key.split_whitespace().nth(1).ok_or_else(|| {
        ApiError::bad_request("public_key must be in 'type base64 [comment]' format")
    })?;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(blob_b64)
        .map_err(|e| ApiError::bad_request(format!("invalid base64 in public_key: {e}")))?;
    let digest = Sha256::digest(&blob);
    let encoded = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    Ok(format!("SHA256:{encoded}"))
}

pub async fn create_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Json(body): Json<CreateKeypairBody>,
) -> Result<Json<KeypairRow>, ApiError> {
    require_operator(&actor)?;
    machina_spec::validate_name(&body.name).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if body.public_key.trim().is_empty() {
        return Err(ApiError::bad_request("public_key is required"));
    }
    let fingerprint = ssh_fingerprint(&body.public_key)?;
    let id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO keypairs (id, project_id, name, public_key, fingerprint) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body.project_id)
    .bind(&body.name)
    .bind(&body.public_key)
    .bind(&fingerprint)
    .execute(&state.pool)
    .await?;
    Ok(Json(KeypairRow {
        id,
        project_id: body.project_id,
        name: body.name,
        public_key: body.public_key,
        fingerprint,
        ec2_id: crate::resource_ids::ec2_id(crate::resource_ids::Kind::KeyPair, id),
    }))
}

pub async fn delete_keypair(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_operator(&actor)?;
    crate::db::query("DELETE FROM keypairs WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
