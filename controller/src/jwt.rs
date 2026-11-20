// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

const ISSUER: &str = "machina-controller";

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
    #[serde(default = "default_iss")]
    pub iss: String,
    /// `oidc`, `saml`, or `local` when issued from federated login.
    #[serde(default)]
    pub auth: Option<String>,
}

fn default_iss() -> String {
    ISSUER.to_string()
}

pub fn issue_token(
    secret: &str,
    username: &str,
    role: &str,
    ttl_secs: i64,
    auth: Option<&str>,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: username.to_string(),
        role: role.to_string(),
        iat: now,
        exp: now + ttl_secs.max(60) as usize,
        iss: ISSUER.to_string(),
        auth: auth.map(str::to_string),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

pub fn verify_token(secret: &str, token: &str) -> anyhow::Result<Claims> {
    let previous = std::env::var("MACHINA_JWT_PREVIOUS_SECRETS").unwrap_or_default();
    let mut last = None;
    let mut claims = None;
    for key in std::iter::once(secret).chain(previous.split(',').map(str::trim).filter(|s| !s.is_empty())) {
        match decode::<Claims>(token, &DecodingKey::from_secret(key.as_bytes()), &Validation::default()) {
            Ok(data) => { claims = Some(data.claims); break; }
            Err(e) => last = Some(e),
        }
    }
    let claims = claims.ok_or_else(|| anyhow::anyhow!(
        "JWT verification failed with current and previous signing keys: {}",
        last.map(|e| e.to_string()).unwrap_or_else(|| "no key".into())
    ))?;
    // Tokens without iss use the serde default "machina-controller" (backwards-compatible).
    // Tokens explicitly issued by another service (wrong iss) are rejected.
    if claims.iss != ISSUER {
        anyhow::bail!("JWT issuer mismatch: expected {ISSUER}, got {}", claims.iss);
    }
    Ok(claims)
}
