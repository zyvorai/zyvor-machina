// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::Deserialize;
use crate::db::DbPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub enabled: bool,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    email: Option<String>,
    preferred_username: Option<String>,
    name: Option<String>,
}

pub async fn load_config(pool: &DbPool, fallback_redirect: &str) -> anyhow::Result<OidcConfig> {
    let row: (bool, String, String, String, String) = crate::db::query_as(
        "SELECT oidc_enabled, oidc_issuer, oidc_client_id, oidc_client_secret,
                COALESCE(NULLIF(oidc_redirect_uri, ''), ?)
         FROM clusters ORDER BY created_at LIMIT 1",
    )
    .bind(fallback_redirect)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no cluster configured — run machina-controller bootstrap"))?;
    Ok(OidcConfig {
        enabled: row.0,
        issuer: row.1.trim_end_matches('/').to_string(),
        client_id: row.2,
        client_secret: row.3,
        redirect_uri: row.4,
    })
}

pub async fn begin_login(pool: &DbPool, cfg: &OidcConfig) -> anyhow::Result<(String, String)> {
    let discovery = fetch_discovery(&cfg.issuer).await?;
    let state = Uuid::new_v4().to_string();
    let nonce = Uuid::new_v4().to_string();
    crate::db::query("INSERT INTO oidc_states (state, nonce) VALUES (?, ?)")
        .bind(&state)
        .bind(&nonce)
        .execute(pool)
        .await?;
    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=openid%20profile%20email&state={}&nonce={}",
        discovery.authorization_endpoint,
        urlencoding::encode(&cfg.client_id),
        urlencoding::encode(&cfg.redirect_uri),
        urlencoding::encode(&state),
        urlencoding::encode(&nonce),
    );
    Ok((state, url))
}

pub async fn complete_login(
    pool: &DbPool,
    cfg: &OidcConfig,
    jwt_secret: &str,
    code: &str,
    state: &str,
) -> anyhow::Result<(String, String, String)> {
    // Fetch nonce alongside state validation; None means state not found or expired.
    // 5-minute window — converged with the daemon's OIDC_STATE_TTL_SECS (this was
    // 10 minutes before unifying the two OIDC implementations' state/nonce TTLs).
    let stored_nonce: Option<Option<String>> = crate::db::query_scalar(
        "SELECT nonce FROM oidc_states WHERE state = ? AND created_at > datetime('now', '-5 minutes')",
    )
    .bind(state)
    .fetch_optional(pool)
    .await?;
    let Some(nonce) = stored_nonce else {
        anyhow::bail!("invalid or expired OIDC state");
    };
    crate::db::query("DELETE FROM oidc_states WHERE state = ?")
        .bind(state)
        .execute(pool)
        .await?;

    let discovery = fetch_discovery(&cfg.issuer).await?;
    let client = reqwest::Client::new();
    let token: TokenResponse = client
        .post(&discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", cfg.redirect_uri.as_str()),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let username = if let Some(ref userinfo_url) = discovery.userinfo_endpoint {
        let info: UserInfo = client
            .get(userinfo_url)
            .bearer_auth(&token.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        info.email
            .or(info.preferred_username)
            .or(info.name)
            .unwrap_or(info.sub)
    } else if let Some(ref id_token) = token.id_token {
        let Some(jwks_uri) = discovery.jwks_uri.as_deref() else {
            return Err(anyhow::anyhow!(
                "OIDC provider has no jwks_uri — cannot verify id_token signature"
            ));
        };
        let jwks = machina_core::oidc::fetch_jwks(&client, jwks_uri)
            .await
            .map_err(|e| anyhow::anyhow!("fetch OIDC JWKS: {e}"))?;
        let claims = match machina_core::oidc::validate_id_token(
            id_token,
            &jwks,
            &discovery.issuer,
            &cfg.client_id,
            nonce.as_deref(),
        ) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("id_token JWKS validation failed: {e:#}");
                return Err(anyhow::anyhow!("id_token signature verification failed"));
            }
        };
        machina_core::oidc::claim_string(claims.extra.get("email"))
            .or_else(|| machina_core::oidc::claim_string(claims.extra.get("preferred_username")))
            .or_else(|| machina_core::oidc::claim_string(claims.extra.get("name")))
            .unwrap_or(claims.sub)
    } else {
        format!(
            "oidc-{}",
            token.access_token.chars().take(8).collect::<String>()
        )
    };

    let role: String = match crate::db::query_scalar("SELECT role FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(pool)
        .await?
    {
        Some(r) => r,
        None => {
            let role = "viewer".to_string();
            crate::db::query(
                "INSERT INTO users (id, username, password_hash, role) VALUES (?, ?, ?, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(&username)
            .bind("oidc")
            .bind(&role)
            .execute(pool)
            .await?;
            role
        }
    };

    let token = crate::jwt::issue_token(jwt_secret, &username, &role, 86400, Some("oidc"))?;
    Ok((username, role, token))
}

// require_https: true unless MACHINA_OIDC_ALLOW_HTTP=1 — the controller has always required
// an HTTPS issuer (the daemon's own discovery fetch does not). The flag lets a lab or air-gapped
// site use a plain-HTTP local IdP (Keycloak, Dex) on a trusted network.
fn oidc_require_https() -> bool {
    !matches!(
        std::env::var("MACHINA_OIDC_ALLOW_HTTP").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

async fn fetch_discovery(
    issuer: &str,
) -> anyhow::Result<machina_core::oidc::OidcDiscoveryDocument> {
    let client =
        machina_core::oidc::oidc_http_client().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    machina_core::oidc::fetch_discovery(&client, issuer, oidc_require_https())
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
