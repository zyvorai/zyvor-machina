// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Mint short-lived platform JWTs for same-host calls into the co-located
//! `machina-daemon` (e.g. sprite inventory pull for fleet visibility).
//!
//! Mirrors `controller::jwt::issue_token` exactly, but with `iss:
//! "machina-agent"` — the daemon's `actor_from_platform_jwt` accepts both
//! issuers. This relies on the same `MACHINA_JWT_SECRET` operators must
//! already provision consistently across daemon/controller for the
//! existing KubeVirt inventory sync to work.

use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

const ISSUER: &str = "machina-agent";

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    role: String,
    exp: usize,
    iat: usize,
    iss: String,
}

/// Mint a short-lived, read-only ("viewer") token for this agent to call
/// its co-located daemon's HTTP API.
pub fn issue_local_token(secret: &str, ttl_secs: i64) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: "agent-internal-sync".to_string(),
        role: "viewer".to_string(),
        iat: now,
        exp: now + ttl_secs.max(60) as usize,
        iss: ISSUER.to_string(),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}
