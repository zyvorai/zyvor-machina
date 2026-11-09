// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::extract::Query;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, DecodingKey, Validation};
use machina_core::libvirt::automation::{
    effective_token_scopes, get_user_role, load_roles, token_allows, Role,
};
use machina_core::{AuthConfig, LibvirtError, LibvirtManager, OidcConfig, OidcDefaultRole};
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::error::AppError;

/// Authenticated HTTP actor (cookie session or API bearer token).
#[derive(Clone, Debug)]
pub struct RequestActor {
    pub username: String,
    /// Local Linux account used for sudo/libvirt session policy when present.
    pub effective_linux_user: Option<String>,
    /// API tokens must not perform sensitive host administration (e.g. OS user creation).
    pub from_api_token: bool,
    /// Effective RBAC role (from `api-tokens.json` or `roles.json` for browser sessions).
    pub role: Role,
    /// Where the request identity came from.
    pub auth_source: AuthSource,
    /// API token scopes (empty for browser/OIDC sessions).
    pub token_scopes: Vec<String>,
}

/// Enforce scope for bearer-token requests (`vms:write`, `fleet:proxy`, `*`, etc.).
pub fn require_api_scope(actor: &RequestActor, scope: &str) -> Result<(), LibvirtError> {
    if !actor.from_api_token {
        return Ok(());
    }
    if token_allows(&actor.token_scopes, scope) {
        Ok(())
    } else {
        Err(LibvirtError::Forbidden(format!(
            "API token lacks required scope '{scope}'"
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    Pam,
    Ldap,
    Oidc,
    ApiToken,
}

/// Host insight that reads passwd-like data, runs package managers, or sleeps on `/proc/net/dev`.
/// API tokens are rejected so automation credentials cannot scrape the hypervisor.
pub fn require_browser_session_for_host_insight(actor: &RequestActor) -> Result<(), LibvirtError> {
    if actor.from_api_token {
        Err(LibvirtError::Forbidden(
            "This endpoint requires a browser session (cookie), not an API token.".into(),
        ))
    } else {
        Ok(())
    }
}

/// Session store: token -> (username, created_at)
#[derive(Clone)]
pub struct SessionStore {
    // RwLock so concurrent reads (validate_session on every request) don't serialize.
    // Only writes (create/revoke/remove) need exclusive access.
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
    ws_tokens: Arc<Mutex<HashMap<String, WsTokenData>>>,
    oidc_states: Arc<Mutex<HashMap<String, machina_core::oidc::OidcStateEntry>>>,
    /// Native-app SSO: pending logins and single-use exchange codes (issue #224).
    native: crate::native_auth::NativeAuthStore<RequestActor>,
    max_sessions_global: usize,
    /// `0` = unlimited concurrent sessions per username.
    max_sessions_per_user: usize,
}

struct SessionData {
    actor: RequestActor,
    created_at: Instant,
    /// Opaque id for admin revoke (never the secret cookie token).
    public_id: String,
}

/// Row for `GET /admin/sessions` (root only).
#[derive(Serialize)]
pub struct SessionListEntry {
    pub session_id: String,
    pub username: String,
    pub auth_source: AuthSource,
    pub age_secs: u64,
    pub expires_in_secs: u64,
    pub is_current: bool,
}

const SESSION_TTL_SECS: u64 = 86400; // 24 hours

struct WsTokenData {
    actor: RequestActor,
    created_at: Instant,
}

impl SessionStore {
    pub fn new(max_sessions_global: usize, max_sessions_per_user: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            ws_tokens: Arc::new(Mutex::new(HashMap::new())),
            oidc_states: Arc::new(Mutex::new(HashMap::new())),
            native: crate::native_auth::NativeAuthStore::new(),
            max_sessions_global: max_sessions_global.max(1),
            max_sessions_per_user,
        }
    }

    /// Active browser sessions for `username` (non-expired).
    pub fn active_sessions_for_user(&self, username: &str) -> usize {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        sessions
            .values()
            .filter(|d| {
                d.created_at.elapsed().as_secs() < SESSION_TTL_SECS && d.actor.username == username
            })
            .count()
    }

    /// Non-expired browser cookie sessions (API tokens are not counted).
    pub fn active_session_count(&self) -> usize {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        sessions
            .values()
            .filter(|d| d.created_at.elapsed().as_secs() < SESSION_TTL_SECS)
            .count()
    }

    fn create_session(&self, actor: RequestActor) -> String {
        let mut rng = rand::thread_rng();
        let token_bytes: [u8; 32] = rng.gen();
        let token = hex::encode(token_bytes);
        let public_id_bytes: [u8; 16] = rng.gen();
        let public_id = hex::encode(public_id_bytes);

        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());

        // Purge expired sessions
        sessions.retain(|_, data| data.created_at.elapsed().as_secs() < SESSION_TTL_SECS);

        // Global cap only (oldest session anywhere).
        if sessions.len() >= self.max_sessions_global {
            if let Some(oldest_token) = sessions
                .iter()
                .min_by_key(|(_, data)| data.created_at)
                .map(|(tok, _)| tok.clone())
            {
                sessions.remove(&oldest_token);
            }
        }

        // Per-user cap when configured (> 0). Default 0 = concurrent sessions allowed.
        if self.max_sessions_per_user > 0 {
            let user_sessions: Vec<String> = sessions
                .iter()
                .filter(|(_, data)| data.actor.username == actor.username)
                .map(|(tok, _)| tok.clone())
                .collect();
            if user_sessions.len() >= self.max_sessions_per_user {
                if let Some(oldest_token) = user_sessions
                    .iter()
                    .min_by_key(|tok| sessions.get(tok.as_str()).map(|d| d.created_at))
                    .cloned()
                {
                    sessions.remove(&oldest_token);
                }
            }
        }

        sessions.insert(
            token.clone(),
            SessionData {
                actor,
                created_at: Instant::now(),
                public_id,
            },
        );
        token
    }

    fn validate_session(&self, token: &str) -> Option<RequestActor> {
        // Use a read lock so concurrent requests don't serialize. Expired entries are
        // pruned lazily in the write paths (create_session, list_browser_sessions).
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        let data = sessions.get(token)?;
        if data.created_at.elapsed().as_secs() < SESSION_TTL_SECS {
            Some(data.actor.clone())
        } else {
            None
        }
    }

    /// Public id for the given session cookie token, if still valid.
    pub fn session_public_id(&self, token: &str) -> Option<String> {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        sessions.get(token).and_then(|data| {
            if data.created_at.elapsed().as_secs() < SESSION_TTL_SECS {
                Some(data.public_id.clone())
            } else {
                None
            }
        })
    }

    /// All non-expired browser sessions (in-memory).
    pub fn list_browser_sessions(&self, current_public_id: Option<&str>) -> Vec<SessionListEntry> {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, data| data.created_at.elapsed().as_secs() < SESSION_TTL_SECS);
        let mut out: Vec<SessionListEntry> = sessions
            .values()
            .map(|data| {
                let age = data.created_at.elapsed().as_secs();
                SessionListEntry {
                    session_id: data.public_id.clone(),
                    username: data.actor.username.clone(),
                    auth_source: data.actor.auth_source,
                    age_secs: age,
                    expires_in_secs: SESSION_TTL_SECS.saturating_sub(age),
                    is_current: current_public_id == Some(data.public_id.as_str()),
                }
            })
            .collect();
        out.sort_by_key(|a| a.age_secs);
        out
    }

    /// Revoke a session by its public id. Returns false if not found.
    pub fn revoke_session_by_public_id(&self, public_id: &str) -> bool {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let token = sessions
            .iter()
            .find(|(_, d)| d.public_id == public_id)
            .map(|(t, _)| t.clone());
        if let Some(t) = token {
            sessions.remove(&t);
            return true;
        }
        false
    }

    pub fn remove_session(&self, token: &str) {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.remove(token);
    }

    /// Short-lived WebSocket token (reusable until expiry — supports React remount / reconnect).
    pub fn create_ws_token(&self, actor: &RequestActor) -> String {
        const WS_TOKEN_TTL_SECS: u64 = 120;

        let mut rng = rand::thread_rng();
        let token_bytes: [u8; 32] = rng.gen();
        let token = hex::encode(token_bytes);

        let mut ws_tokens = self.ws_tokens.lock().unwrap_or_else(|e| e.into_inner());
        ws_tokens.retain(|_, data| data.created_at.elapsed().as_secs() < WS_TOKEN_TTL_SECS);
        ws_tokens.insert(
            token.clone(),
            WsTokenData {
                actor: actor.clone(),
                created_at: Instant::now(),
            },
        );
        token
    }

    /// Validate a WebSocket token without removing it (noVNC may open the socket more than once).
    pub fn validate_ws_token(&self, token: &str) -> Option<RequestActor> {
        const WS_TOKEN_TTL_SECS: u64 = 120;

        let mut ws_tokens = self.ws_tokens.lock().unwrap_or_else(|e| e.into_inner());
        ws_tokens.retain(|_, data| data.created_at.elapsed().as_secs() < WS_TOKEN_TTL_SECS);
        let data = ws_tokens.get(token)?;
        if data.created_at.elapsed().as_secs() < WS_TOKEN_TTL_SECS {
            Some(data.actor.clone())
        } else {
            None
        }
    }

    pub fn create_oidc_state(&self, nonce: String) -> String {
        let mut rng = rand::thread_rng();
        let state_bytes: [u8; 32] = rng.gen();
        let state = hex::encode(state_bytes);
        let mut states = self.oidc_states.lock().unwrap_or_else(|e| e.into_inner());
        states.retain(|_, data| {
            data.created_at.elapsed().as_secs() < machina_core::oidc::OIDC_STATE_TTL_SECS
        });
        states.insert(
            state.clone(),
            machina_core::oidc::OidcStateEntry {
                nonce,
                created_at: Instant::now(),
            },
        );
        state
    }

    pub fn take_oidc_state(&self, state: &str) -> Option<String> {
        let mut states = self.oidc_states.lock().unwrap_or_else(|e| e.into_inner());
        states.retain(|_, data| {
            data.created_at.elapsed().as_secs() < machina_core::oidc::OIDC_STATE_TTL_SECS
        });
        states.remove(state).and_then(|data| {
            if data.created_at.elapsed().as_secs() < machina_core::oidc::OIDC_STATE_TTL_SECS {
                Some(data.nonce)
            } else {
                None
            }
        })
    }
}

fn browser_actor(
    username: String,
    effective_linux_user: Option<String>,
    role: Role,
    auth_source: AuthSource,
) -> RequestActor {
    RequestActor {
        username,
        effective_linux_user,
        from_api_token: false,
        role,
        auth_source,
        token_scopes: Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PlatformJwtClaims {
    sub: String,
    role: String,
    exp: usize,
    iat: usize,
    #[serde(default)]
    auth: Option<String>,
}

const DEV_JWT_SECRET: &str = "machina-dev-jwt-secret-change-me";

/// The platform JWT secret used to verify controller-issued deep-link tokens. Never
/// falls back to `DEV_JWT_SECRET` — that value is public (it ships in this repo), so
/// accepting it would let anyone forge a token with `role="admin"` for unauthenticated
/// takeover. Instead: if `MACHINA_JWT_SECRET` is unset (or still the public default),
/// a random secret is generated once for this process's lifetime and a loud warning is
/// logged. This is a deliberate middle ground — not hard-failing daemon startup, and
/// not silently accepting a known-public secret — chosen so a host that never set the
/// env var still gets a working (if non-persistent) platform-JWT session: the random
/// secret means existing sessions are invalidated on every restart until an operator
/// sets MACHINA_JWT_SECRET, at which point sessions persist across restarts normally.
static PLATFORM_JWT_SECRET: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn platform_jwt_secret() -> &'static str {
    PLATFORM_JWT_SECRET.get_or_init(|| match std::env::var("MACHINA_JWT_SECRET") {
        Ok(s) if !s.is_empty() && s != DEV_JWT_SECRET => s,
        other => {
            if matches!(other, Ok(ref s) if s == DEV_JWT_SECRET) {
                tracing::error!(
                    "MACHINA_JWT_SECRET is set to the well-known public default value — \
                     ignoring it and generating a random secret instead. Set a real \
                     secret via MACHINA_JWT_SECRET to allow platform-JWT sessions to \
                     survive a daemon restart."
                );
            } else {
                tracing::warn!(
                    "MACHINA_JWT_SECRET is not set — generating a random platform-JWT \
                     signing secret for this process only. Platform (controller) login \
                     sessions will NOT survive a daemon restart until you set \
                     MACHINA_JWT_SECRET to a stable, private value."
                );
            }
            let mut rng = rand::thread_rng();
            let bytes: [u8; 32] = rng.gen();
            hex::encode(bytes)
        }
    })
}

fn skip_auth_enabled() -> bool {
    std::env::var("MACHINA_DAEMON_SKIP_AUTH").ok().as_deref() == Some("1")
}

fn dev_bypass_actor() -> RequestActor {
    browser_actor("dev".into(), None, Role::Admin, AuthSource::Pam)
}

fn role_from_platform_jwt(role: &str) -> Role {
    match role.to_lowercase().as_str() {
        "admin" => Role::Admin,
        "operator" => Role::Operator,
        _ => Role::ReadOnly,
    }
}

fn auth_source_from_platform_jwt(auth: Option<&str>) -> AuthSource {
    match auth {
        Some("oidc") | Some("saml") => AuthSource::Oidc,
        _ => AuthSource::Pam,
    }
}

/// Issuer stamped on every platform JWT by `controller/src/jwt.rs::issue_token`
/// (`ISSUER` there). Binding validation to it means a token minted by some other
/// service/HS256-secret-holder can't be replayed here even if it guesses the secret.
const PLATFORM_JWT_ISSUER: &str = "machina-controller";

/// Issuer stamped on tokens `agent/src/jwt.rs::issue_local_token` mints for
/// same-host calls into this daemon (e.g. sprite inventory pull for fleet
/// visibility) — always `role: "viewer"`, never anything privileged.
const AGENT_JWT_ISSUER: &str = "machina-agent";

fn actor_from_platform_jwt(token: &str) -> Option<RequestActor> {
    let secret = platform_jwt_secret();
    let mut validation = Validation::default();
    validation.set_issuer(&[PLATFORM_JWT_ISSUER, AGENT_JWT_ISSUER]);
    validation.set_required_spec_claims(&["exp", "iss"]);
    let data = decode::<PlatformJwtClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()?;
    let claims = data.claims;
    Some(browser_actor(
        claims.sub,
        None,
        role_from_platform_jwt(&claims.role),
        auth_source_from_platform_jwt(claims.auth.as_deref()),
    ))
}

fn actor_from_bearer_token(token: &str) -> Option<RequestActor> {
    if token.starts_with("mach_") || token.starts_with("vs_") {
        let api = machina_core::libvirt::automation::validate_api_token(token)?;
        let scopes = effective_token_scopes(&api);
        return Some(RequestActor {
            username: api.username,
            effective_linux_user: None,
            from_api_token: true,
            role: api.role,
            auth_source: AuthSource::ApiToken,
            token_scopes: scopes,
        });
    }
    actor_from_platform_jwt(token)
}

fn actor_from_cookie(sessions: &SessionStore, cookie_header: &str) -> Option<RequestActor> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("machina_session=") {
            let token = value.trim();
            if !token.is_empty() {
                return sessions.validate_session(token);
            }
        }
    }
    None
}

fn resolve_actor(sessions: &SessionStore, headers: &axum::http::HeaderMap) -> Option<RequestActor> {
    if skip_auth_enabled() {
        return Some(dev_bypass_actor());
    }

    if let Some(cookie_header) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        if let Some(actor) = actor_from_cookie(sessions, cookie_header) {
            return Some(actor);
        }
    }

    if let Some(auth_header) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                if let Some(actor) = actor_from_bearer_token(token) {
                    return Some(actor);
                }
                if token.starts_with("mach_") || token.starts_with("vs_") {
                    machina_core::obs_counters::inc_api_token_fail();
                }
            }
        }
    }

    None
}

pub fn effective_linux_user(actor: &RequestActor) -> Option<&str> {
    actor.effective_linux_user.as_deref()
}

fn resolve_oidc_role(username: &str, groups: &[String], cfg: &OidcConfig) -> Role {
    use machina_core::oidc::RoleTier;
    match machina_core::oidc::resolve_role_tier(groups, &cfg.admin_groups, &cfg.operator_groups) {
        RoleTier::Admin => Role::Admin,
        RoleTier::Operator => Role::Operator,
        RoleTier::NoMatch => {
            let roles = load_roles();
            if let Some(local_role) = roles.get(username) {
                return local_role.clone();
            }
            match cfg.default_role {
                OidcDefaultRole::Admin => Role::Admin,
                OidcDefaultRole::Operator => Role::Operator,
                OidcDefaultRole::ReadOnly => Role::ReadOnly,
            }
        }
    }
}

#[derive(Clone)]
pub struct OidcAuth(pub std::sync::Arc<AuthConfig>);

#[derive(Debug, Deserialize, Serialize)]
struct OidcProviderMetadata {
    enabled: bool,
    button_label: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SamlProviderMetadata {
    enabled: bool,
    button_label: String,
    /// SAML browser login is not implemented yet — metadata only.
    login_available: bool,
}

#[derive(Debug, Deserialize)]
struct OidcCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Query of `GET /auth/oidc/login`. All fields are only used by the native-app flow;
/// the web flow sends none of them.
#[derive(Debug, Deserialize)]
struct OidcLoginQuery {
    native: Option<String>,
    redirect_uri: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OidcTokenResponse {
    id_token: Option<String>,
}

/// Map a `machina_core::oidc::OidcError` onto the daemon's own error type, keeping
/// the pre-unification Forbidden/Operation split: an untrustworthy or invalid
/// discovery/token response is Forbidden (403), a network/decode failure talking
/// to the IdP is Operation (500) — matching the behavior this daemon had before
/// the OIDC protocol logic moved into `machina_core::oidc`.
fn oidc_error_to_app_error(e: machina_core::oidc::OidcError) -> AppError {
    use machina_core::oidc::OidcError as E;
    match e {
        E::DiscoveryInvalid(msg) | E::TokenInvalid(msg) => {
            AppError::from(LibvirtError::Forbidden(msg))
        }
        E::DiscoveryFetch(msg) => AppError::from(LibvirtError::Operation(format!(
            "Fetch OIDC discovery: {msg}"
        ))),
        E::DiscoveryDecode(msg) => AppError::from(LibvirtError::Operation(format!(
            "Decode OIDC discovery document: {msg}"
        ))),
        E::JwksFetch(msg) => {
            AppError::from(LibvirtError::Operation(format!("Fetch OIDC JWKS: {msg}")))
        }
        E::JwksDecode(msg) => {
            AppError::from(LibvirtError::Operation(format!("Decode OIDC JWKS: {msg}")))
        }
    }
}

fn oidc_http_client() -> Result<reqwest::Client, AppError> {
    machina_core::oidc::oidc_http_client().map_err(oidc_error_to_app_error)
}

/// `require_https: false` — this daemon has never enforced an HTTPS-only issuer
/// (needed for internal/test IdPs), unlike the controller's OIDC flow which does.
/// Preserved as-is rather than silently tightened as part of sharing this code.
async fn fetch_oidc_discovery(
    cfg: &OidcConfig,
) -> Result<machina_core::oidc::OidcDiscoveryDocument, AppError> {
    let client = oidc_http_client()?;
    machina_core::oidc::fetch_discovery(&client, &cfg.issuer_url, false)
        .await
        .map_err(oidc_error_to_app_error)
}

async fn fetch_oidc_jwks(url: &str) -> Result<JwkSet, AppError> {
    let client = oidc_http_client()?;
    machina_core::oidc::fetch_jwks(&client, url)
        .await
        .map_err(oidc_error_to_app_error)
}

fn resolve_effective_linux_user(
    username: &str,
    claims: &HashMap<String, serde_json::Value>,
    cfg: &OidcConfig,
) -> Option<String> {
    let claimed = machina_core::oidc::claim_string(claims.get(&cfg.linux_username_claim))
        .or_else(|| machina_core::oidc::claim_string(claims.get(&cfg.username_claim)))
        .or_else(|| Some(username.to_string()))?;
    if machina_core::system_accounts::unix_user_exists(&claimed) {
        Some(claimed)
    } else {
        None
    }
}

fn validate_oidc_id_token(
    id_token: &str,
    jwks: &JwkSet,
    discovery: &machina_core::oidc::OidcDiscoveryDocument,
    cfg: &OidcConfig,
    expected_nonce: &str,
) -> Result<(String, Option<String>, Role), AppError> {
    let claims = machina_core::oidc::validate_id_token(
        id_token,
        jwks,
        &discovery.issuer,
        cfg.client_id.trim(),
        Some(expected_nonce),
    )
    .map_err(oidc_error_to_app_error)?;

    let username = machina_core::oidc::claim_string(claims.extra.get(&cfg.username_claim))
        .or_else(|| machina_core::oidc::claim_string(claims.extra.get("email")))
        .unwrap_or(claims.sub);
    let effective_linux_user = resolve_effective_linux_user(&username, &claims.extra, cfg);
    let groups = machina_core::oidc::claim_strings(claims.extra.get(&cfg.groups_claim));
    let role = resolve_oidc_role(&username, &groups, cfg);
    Ok((username, effective_linux_user, role))
}

/// Extract session token from cookie header.
fn extract_token(req: &Request<Body>) -> Option<String> {
    let cookie_header = req.headers().get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("machina_session=") {
            let token = value.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

// ── Auth-adjacent rate limiting ────────────────────────────────────
//
// `auth_middleware` below explicitly exempts every `/auth/*` path from auth checks
// (they *are* the authentication), which means `/auth/login` (PAM/LDAP password
// verification) and `/auth/oidc/login` + `/auth/oidc/callback` (each triggering a
// real outbound call to the OIDC IdP) are unauthenticated by design and, until now,
// completely unthrottled — open to brute-force/credential-stuffing and to IdP-request
// amplification. This is a minimal, self-contained limiter scoped to just those three
// routes; it deliberately does not replicate `controller/src/rate_limit.rs` (no
// JWT-subject keying — these routes are pre-auth by definition — no env-var tuning,
// no bypass header). It follows the same sliding/fixed-window-bucket idiom for
// consistency with that module.

/// 10 attempts per (client IP, path) per 60s. This is a common baseline for a login
/// endpoint: generous enough that a user mistyping a password a handful of times, or
/// a dev/E2E suite logging in repeatedly in a loop, never trips it, while still
/// bounding brute-force/credential-stuffing traffic (which needs hundreds to
/// thousands of attempts to be useful) and capping how often an anonymous caller can
/// force an outbound request to the OIDC IdP via oidc/login or oidc/callback.
const AUTH_RATE_LIMIT: u32 = 10;
const AUTH_RATE_WINDOW: Duration = Duration::from_secs(60);

/// Cap on tracked (ip, path) keys before we sweep expired buckets — bounds memory
/// against a caller spraying requests from many source ports/spoofed-looking IPs.
/// One window's worth of real traffic to three routes stays well under this.
const AUTH_RATE_MAX_KEYS: usize = 10_000;

struct AuthRateBucket {
    window_start: Instant,
    count: u32,
}

/// In-memory fixed-window limiter for pre-auth endpoints, keyed by `"{ip}:{path}"` so
/// that hammering `/auth/login` cannot burn through the quota for `/auth/oidc/login`
/// (or vice versa). One instance lives for the daemon process lifetime.
#[derive(Clone)]
pub struct AuthRateLimiter {
    inner: Arc<Mutex<HashMap<String, AuthRateBucket>>>,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut map = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Evict fully-expired buckets before the map can grow without bound.
        if map.len() > AUTH_RATE_MAX_KEYS {
            map.retain(|_, b| now.duration_since(b.window_start) < AUTH_RATE_WINDOW);
        }
        let bucket = map.entry(key.to_string()).or_insert(AuthRateBucket {
            window_start: now,
            count: 0,
        });
        if now.duration_since(bucket.window_start) >= AUTH_RATE_WINDOW {
            bucket.window_start = now;
            bucket.count = 0;
        }
        if bucket.count >= AUTH_RATE_LIMIT {
            return false;
        }
        bucket.count += 1;
        true
    }
}

impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Throttles only `/auth/login`, `/auth/oidc/login`, `/auth/oidc/callback` — the three
/// pre-auth, unauthenticated routes carved out by `auth_middleware`. Every other route
/// registered in `auth_routes()` (providers, session, logout, ws-token, …) passes
/// through untouched, including ones the frontend polls frequently.
///
/// Requires `ConnectInfo<SocketAddr>` to be present on the request, which is wired up
/// in `main.rs` via `.into_make_service_with_connect_info::<SocketAddr>()` on both the
/// TLS and plaintext listener paths.
pub async fn auth_rate_limit_middleware(
    State(limiter): State<AuthRateLimiter>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/auth/login"
        || path == "/auth/oidc/login"
        || path == "/auth/oidc/callback"
        || path == "/auth/native/exchange"
    {
        let key = format!("{}:{}", addr.ip(), path);
        if !limiter.check(&key) {
            warn!("rate limit exceeded for {} from {}", path, addr.ip());
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "Too many attempts. Please wait a minute and try again.",
                    "error_code": "rate_limited"
                })),
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Auth middleware — checks for valid session cookie.
/// Skips health check. Applied via route_layer on API/WS routes.
pub async fn auth_middleware(
    State(store): State<SessionStore>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Public endpoints (paths after nest stripping of /api/v1 or /ws/v1)
    if path == "/health"
        || path == "/license"
        || path == "/openapi.json"
        || path.starts_with("/auth/")
    {
        return next.run(req).await;
    }

    // Check session cookie
    if skip_auth_enabled() {
        req.extensions_mut().insert(dev_bypass_actor());
        return next.run(req).await;
    }

    if let Some(token) = extract_token(&req) {
        if let Some(actor) = store.validate_session(&token) {
            req.extensions_mut().insert(actor);
            return next.run(req).await;
        }
    }

    // Check Authorization header for API tokens or platform JWT (Bearer …).
    if let Some(auth_header) = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let token = token.trim();
            if let Some(actor) = actor_from_bearer_token(token) {
                req.extensions_mut().insert(actor);
                return next.run(req).await;
            }
            if token.starts_with("mach_") || token.starts_with("vs_") {
                machina_core::obs_counters::inc_api_token_fail();
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(
            serde_json::json!({ "error": "Authentication required", "error_code": "unauthorized" }),
        ),
    )
        .into_response()
}

// ── WebSocket token handler ────────────────────────────────────────

/// Create a single-use WebSocket token. Must be called from an authenticated context.
pub async fn ws_token_handler(
    Extension(sessions): Extension<SessionStore>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    match resolve_actor(&sessions, &headers) {
        Some(actor) => {
            let token = sessions.create_ws_token(&actor);
            (StatusCode::OK, Json(serde_json::json!({ "token": token }))).into_response()
        }
        None => {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Authentication required", "error_code": "unauthorized" })),
            )
                .into_response()
        }
    }
}

/// WebSocket auth middleware — checks for `?token=` query parameter.
pub async fn ws_auth_middleware(
    State(store): State<SessionStore>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();
    // Platform VNC/serial use controller-issued tokens; machina-controller validates them.
    // The WS routes are nested at /ws/v1, so the full path is /ws/v1/platform/vnc/... or
    // /ws/v1/platform/serial/... — match either prefix to accommodate future re-nesting.
    // The bare-metal SOL proxy is the same case: the controller validates its single-use token.
    if path.contains("/platform/vnc/")
        || path.contains("/platform/serial/")
        || path.contains("/platform/spice/")
        || (path.starts_with("/ws/v1/baremetal/") && path.ends_with("/sol"))
    {
        return next.run(req).await;
    }

    // Extract token from query string
    let token = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|pair| pair.strip_prefix("token=").map(|v| v.to_string()))
    });

    if let Some(ref tok) = token {
        if let Some(actor) = store.validate_ws_token(tok) {
            req.extensions_mut().insert(actor);
            return next.run(req).await;
        }
        // The controller relays KubeVirt console WS connections here as a
        // trusted server-to-server hop (mirroring how it relays libvirt VNC
        // to the agent) after validating its own ws-token on the browser
        // side — see controller/src/console.rs's kubevirt branch. It proves
        // itself with the same short-lived platform JWT already used for
        // its other daemon-internal calls (kubevirt_inventory.rs), not a
        // token from this daemon's own (unrelated) ws-token store.
        if let Some(actor) = actor_from_platform_jwt(tok) {
            req.extensions_mut().insert(actor);
            return next.run(req).await;
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Valid WebSocket token required", "error_code": "unauthorized" })),
    )
        .into_response()
}

// ── Auth handlers (use Extension<SessionStore>) ────────────────────

/// Build the `Set-Cookie` value for a freshly issued `machina_session` token. `Secure` is only
/// appended when `tls_enabled` is true (the daemon is actually terminating TLS itself, per
/// `AuthConfig::tls_enabled` / `TlsConfig::is_effectively_enabled`) — setting `Secure`
/// unconditionally would make browsers silently drop the cookie on any instance still serving
/// plain HTTP, breaking login rather than protecting anything.
fn session_cookie(token: &str, tls_enabled: bool) -> String {
    if tls_enabled {
        format!("machina_session={token}; Path=/; HttpOnly; Secure; SameSite=Strict")
    } else {
        format!("machina_session={token}; Path=/; HttpOnly; SameSite=Strict")
    }
}

/// Build the `Set-Cookie` value that clears `machina_session` on logout. Must carry the same
/// `Secure` attribute the cookie was originally set with, or some browsers won't recognize it as
/// the same cookie and won't delete it.
fn clear_session_cookie(tls_enabled: bool) -> &'static str {
    if tls_enabled {
        "machina_session=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0"
    } else {
        "machina_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    Extension(stats): Extension<std::sync::Arc<crate::daemon_stats::DaemonStats>>,
    Json(req): Json<LoginRequest>,
) -> Response {
    if req.username.is_empty() || req.password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Username and password required", "error_code": "invalid_request" }))).into_response();
    }

    let cfg = &auth.0;
    if !req.username.chars().all(|c| {
        c.is_alphanumeric()
            || c == '_'
            || c == '-'
            || c == '.'
            || (c == '@' && cfg.ldap.is_enabled())
    }) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Invalid username characters", "error_code": "invalid_request" }))).into_response();
    }

    if cfg.ldap.is_enabled() {
        match crate::ldap_auth::ldap_authenticate_async(&cfg.ldap, &req.username, &req.password)
            .await
        {
            Ok(ldap) => {
                let role = ldap.role.clone();
                info!(
                    "LDAP login successful for user '{}' (role {:?})",
                    ldap.username, role
                );
                let token = store.create_session(browser_actor(
                    ldap.username.clone(),
                    Some(ldap.username.clone()),
                    role.clone(),
                    AuthSource::Ldap,
                ));
                let cookie = session_cookie(&token, cfg.tls_enabled);
                stats.inc_auth_attempt("ldap", "success");
                return (
                    StatusCode::OK,
                    [(header::SET_COOKIE, cookie)],
                    Json(serde_json::json!({
                        "status": "ok",
                        "username": ldap.username,
                        "role": role,
                        "auth_source": "ldap"
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                warn!("LDAP login failed for user '{}': {}", req.username, e);
                stats.inc_auth_attempt("ldap", "failure");
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Invalid username or password", "error_code": "unauthorized" }))).into_response();
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        if !cfg.ldap.is_enabled() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Password login via PAM is only available when machina-daemon runs on Linux",
                    "error_code": "pam_unavailable"
                })),
            )
                .into_response();
        }
    }

    match pam_authenticate(&req.username, &req.password, &cfg.pam_service) {
        Ok(()) => {
            info!("PAM login successful for user '{}'", req.username);
            let token = store.create_session(browser_actor(
                req.username.clone(),
                Some(req.username.clone()),
                get_user_role(&req.username),
                AuthSource::Pam,
            ));
            let cookie = session_cookie(&token, cfg.tls_enabled);
            stats.inc_auth_attempt("pam", "success");
            (
                StatusCode::OK,
                [(header::SET_COOKIE, cookie)],
                Json(serde_json::json!({
                    "status": "ok",
                    "username": req.username,
                    "auth_source": "pam"
                })),
            )
                .into_response()
        }
        Err(e) => {
            warn!("PAM login failed for user '{}': {}", req.username, e);
            stats.inc_auth_attempt("pam", "failure");
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Invalid username or password", "error_code": "unauthorized" }))).into_response()
        }
    }
}

async fn logout_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    req: Request<Body>,
) -> Response {
    if let Some(token) = extract_token(&req) {
        store.remove_session(&token);
    }
    let cookie = clear_session_cookie(auth.0.tls_enabled);
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "status": "logged_out" })),
    )
        .into_response()
}

async fn session_handler(
    Extension(store): Extension<SessionStore>,
    req: Request<Body>,
) -> Response {
    if let Some(token) = extract_token(&req) {
        if let Some(actor) = store.validate_session(&token) {
            let session_id = store.session_public_id(&token);
            let run_as = machina_core::MachinaConfig::load().auth.run_as_user;
            let mode = serde_json::to_value(&run_as.mode).unwrap_or(serde_json::json!("disabled"));
            let active_for_user = store.active_sessions_for_user(&actor.username);
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "authenticated": true,
                    "username": actor.username,
                    "effective_linux_user": actor.effective_linux_user,
                    "session_id": session_id,
                    "role": actor.role,
                    "auth_source": actor.auth_source,
                    "active_sessions_for_user": active_for_user,
                    "max_sessions_per_user": store.max_sessions_per_user,
                    "run_as_user": {
                        "enabled": run_as.wants_impersonation(),
                        "mode": mode,
                        "impersonation_active": run_as.impersonation_active(),
                        "prefer_session_libvirt_on_impersonation": run_as.prefer_session_libvirt_on_impersonation,
                    },
                })),
            )
                .into_response();
        }
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "authenticated": false })),
    )
        .into_response()
}

async fn auth_providers_handler(Extension(auth): Extension<OidcAuth>) -> Response {
    let cfg = &auth.0;
    let oidc = &cfg.oidc;
    let ldap_on = cfg.ldap.is_enabled();
    let saml = &cfg.saml;
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "native_sso": true,
            "pam": { "enabled": !ldap_on },
            "ldap": { "enabled": ldap_on },
            "oidc": OidcProviderMetadata {
                enabled: oidc.is_enabled(),
                button_label: oidc.button_label.clone(),
            },
            "saml": SamlProviderMetadata {
                enabled: saml.is_configured(),
                button_label: saml.button_label.clone(),
                login_available: false,
            }
        })),
    )
        .into_response()
}

fn native_bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg, "error_code": "invalid_request" })),
    )
        .into_response()
}

async fn oidc_login_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    Query(query): Query<OidcLoginQuery>,
) -> Response {
    let cfg = &auth.0.oidc;
    if !cfg.is_enabled() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "OIDC login is not enabled" })),
        )
            .into_response();
    }

    // Native-app flow is opt-in; without `native` the web flow below is untouched.
    // Everything is validated before any outbound request to the IdP.
    let mut native_params: Option<(String, Option<String>)> = None;
    if let Some(native) = query.native.as_deref() {
        if native != "1" {
            return native_bad_request("native must be 1");
        }
        let redirect_ok = query
            .redirect_uri
            .as_deref()
            .map(crate::native_auth::redirect_uri_allowed)
            .unwrap_or(false);
        if !redirect_ok {
            return native_bad_request("redirect_uri is not allowed");
        }
        if query.code_challenge_method.as_deref() != Some("S256") {
            return native_bad_request("code_challenge_method must be S256");
        }
        let challenge = match query.code_challenge.as_deref() {
            Some(c) if crate::native_auth::challenge_valid(c) => c.to_string(),
            _ => return native_bad_request("code_challenge is missing or malformed"),
        };
        let client_state = match query.state.as_deref() {
            None => None,
            Some(s) if crate::native_auth::client_state_valid(s) => Some(s.to_string()),
            Some(_) => return native_bad_request("state is malformed"),
        };
        native_params = Some((challenge, client_state));
    }

    let discovery = match fetch_oidc_discovery(cfg).await {
        Ok(doc) => doc,
        Err(e) => return e.into_response(),
    };

    let mut rng = rand::thread_rng();
    let nonce_bytes: [u8; 32] = rng.gen();
    let nonce = hex::encode(nonce_bytes);
    let state = store.create_oidc_state(nonce.clone());
    if let Some((challenge, client_state)) = native_params {
        store.native.begin(&state, &challenge, client_state);
    }
    let mut url = match reqwest::Url::parse(&discovery.authorization_endpoint) {
        Ok(url) => url,
        Err(e) => {
            return AppError::from(LibvirtError::Operation(format!(
                "Invalid OIDC authorization endpoint: {e}"
            )))
            .into_response();
        }
    };
    let scope = if cfg.scopes.is_empty() {
        "openid profile email".to_string()
    } else {
        cfg.scopes.join(" ")
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", cfg.client_id.trim())
        .append_pair("redirect_uri", cfg.redirect_url.trim())
        .append_pair("scope", &scope)
        .append_pair("state", &state)
        .append_pair("nonce", &nonce);

    axum::response::Redirect::temporary(url.as_ref()).into_response()
}

async fn oidc_callback_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    Extension(stats): Extension<std::sync::Arc<crate::daemon_stats::DaemonStats>>,
    Query(query): Query<OidcCallbackQuery>,
) -> Response {
    let cfg = &auth.0.oidc;
    if !cfg.is_enabled() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "OIDC login is not enabled" })),
        )
            .into_response();
    }
    if let Some(err) = query.error {
        let msg = query
            .error_description
            .unwrap_or_else(|| "OIDC provider rejected the login".to_string());
        warn!("OIDC callback error '{}': {}", err, msg);
        stats.inc_auth_attempt("oidc", "failure");
        if let Some(flow) = query.state.as_deref().and_then(|s| store.native.take_flow(s)) {
            let target =
                crate::native_auth::error_redirect("oidc", flow.client_state.as_deref());
            return axum::response::Redirect::temporary(&target).into_response();
        }
        return axum::response::Redirect::temporary("/login?error=oidc").into_response();
    }

    let state = match query.state {
        Some(state) if !state.is_empty() => state,
        _ => {
            stats.inc_auth_attempt("oidc", "failure");
            return AppError::from(LibvirtError::Forbidden(
                "OIDC callback missing state".into(),
            ))
            .into_response();
        }
    };
    let expected_nonce = match store.take_oidc_state(&state) {
        Some(nonce) => nonce,
        None => {
            stats.inc_auth_attempt("oidc", "failure");
            return AppError::from(LibvirtError::Forbidden(
                "OIDC state is invalid or expired".into(),
            ))
            .into_response();
        }
    };
    // Present only when this login was started by a native app.
    let native_flow = store.native.take_flow(&state);
    let code = match query.code {
        Some(code) if !code.is_empty() => code,
        _ => {
            stats.inc_auth_attempt("oidc", "failure");
            return AppError::from(LibvirtError::Forbidden("OIDC callback missing code".into()))
                .into_response();
        }
    };

    let discovery = match fetch_oidc_discovery(cfg).await {
        Ok(doc) => doc,
        Err(e) => return e.into_response(),
    };
    let client = match oidc_http_client() {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    let token_res = match client
        .post(&discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", cfg.redirect_url.trim()),
            ("client_id", cfg.client_id.trim()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return AppError::from(LibvirtError::Operation(format!(
                "Exchange OIDC authorization code: {e}"
            )))
            .into_response();
        }
    };
    if !token_res.status().is_success() {
        stats.inc_auth_attempt("oidc", "failure");
        return AppError::from(LibvirtError::Forbidden(format!(
            "OIDC token endpoint returned HTTP {}",
            token_res.status()
        )))
        .into_response();
    }
    let token_body = match token_res.json::<OidcTokenResponse>().await {
        Ok(body) => body,
        Err(e) => {
            return AppError::from(LibvirtError::Operation(format!(
                "Decode OIDC token response: {e}"
            )))
            .into_response();
        }
    };
    let id_token = match token_body.id_token {
        Some(token) if !token.is_empty() => token,
        _ => {
            stats.inc_auth_attempt("oidc", "failure");
            return AppError::from(LibvirtError::Forbidden(
                "OIDC token response did not include id_token".into(),
            ))
            .into_response();
        }
    };
    let jwks_uri = match discovery.jwks_uri.as_deref() {
        Some(uri) => uri,
        None => {
            stats.inc_auth_attempt("oidc", "failure");
            return AppError::from(LibvirtError::Operation(
                "OIDC provider has no jwks_uri — cannot verify id_token signature".into(),
            ))
            .into_response();
        }
    };
    let jwks = match fetch_oidc_jwks(jwks_uri).await {
        Ok(set) => set,
        Err(e) => return e.into_response(),
    };
    let (username, effective_linux_user, role) =
        match validate_oidc_id_token(&id_token, &jwks, &discovery, cfg, &expected_nonce) {
            Ok(actor) => actor,
            Err(e) => {
                stats.inc_auth_attempt("oidc", "failure");
                return e.into_response();
            }
        };

    if let Some(flow) = native_flow {
        // Native app: hand back a single-use exchange code instead of a cookie. The
        // session itself is created at exchange time, so an unredeemed code leaves
        // nothing behind.
        let actor = browser_actor(
            username.clone(),
            effective_linux_user,
            role,
            AuthSource::Oidc,
        );
        let exchange_code = store.native.mint(actor, &flow.code_challenge);
        info!(
            "OIDC login validated for user '{}' (native app, awaiting code exchange)",
            username
        );
        let target =
            crate::native_auth::success_redirect(&exchange_code, flow.client_state.as_deref());
        return axum::response::Redirect::temporary(&target).into_response();
    }

    let token = store.create_session(browser_actor(
        username.clone(),
        effective_linux_user,
        role,
        AuthSource::Oidc,
    ));
    let cookie = session_cookie(&token, auth.0.tls_enabled);
    info!("OIDC login successful for user '{}'", username);
    stats.inc_auth_attempt("oidc", "success");
    (
        StatusCode::TEMPORARY_REDIRECT,
        [
            (header::SET_COOKIE, cookie),
            (header::LOCATION, "/".to_string()),
        ],
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct NativeExchangeRequest {
    code: String,
    code_verifier: String,
}

/// `POST /auth/native/exchange`: trade a native-app exchange code plus its PKCE verifier
/// for the same session a cookie login would give. Single use; never logs the inputs.
async fn native_exchange_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    Extension(stats): Extension<std::sync::Arc<crate::daemon_stats::DaemonStats>>,
    Json(req): Json<NativeExchangeRequest>,
) -> Response {
    match store
        .native
        .redeem(req.code.trim(), req.code_verifier.trim())
    {
        Ok(actor) => {
            let session_token = store.create_session(actor.clone());
            let cookie = session_cookie(&session_token, auth.0.tls_enabled);
            stats.inc_auth_attempt("oidc", "success");
            info!(
                "OIDC login successful for user '{}' (native app)",
                actor.username
            );
            (
                StatusCode::OK,
                [(header::SET_COOKIE, cookie)],
                Json(serde_json::json!({
                    "status": "ok",
                    "username": actor.username,
                    "role": actor.role,
                    "auth_source": actor.auth_source,
                })),
            )
                .into_response()
        }
        Err(_) => {
            stats.inc_auth_attempt("oidc", "failure");
            warn!("native SSO code exchange rejected");
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Invalid or expired code",
                    "error_code": "invalid_grant"
                })),
            )
                .into_response()
        }
    }
}

pub fn require_destroy_vm(actor: &RequestActor) -> Result<(), AppError> {
    if actor.role.can_destroy_vm() {
        Ok(())
    } else {
        Err(AppError::from(LibvirtError::Forbidden(
            "Destroying or undefining VMs requires the admin role.".into(),
        )))
    }
}

/// Gate a mutating handler: the caller must have write role (operator/admin) AND, for
/// API-token callers, the required scope. Many storage/device/snapshot/migrate handlers
/// historically authenticated but never authorized, letting a read-only token perform
/// destructive host/VM operations — this is the single guard they were missing.
pub fn require_write(actor: &RequestActor, scope: &str) -> Result<(), AppError> {
    require_api_scope(actor, scope).map_err(AppError::from)?;
    if !actor.role.can_write() {
        return Err(AppError::from(LibvirtError::Forbidden(
            "This operation requires the operator or admin role.".into(),
        )));
    }
    Ok(())
}

pub fn require_usb_pci(actor: &RequestActor) -> Result<(), AppError> {
    if actor.role.can_usb_pci() {
        Ok(())
    } else {
        Err(AppError::from(LibvirtError::Forbidden(
            "USB and PCI passthrough require the operator or admin role.".into(),
        )))
    }
}

pub fn require_browse_host_paths(actor: &RequestActor) -> Result<(), AppError> {
    if actor.role.can_browse_host_paths() {
        Ok(())
    } else {
        Err(AppError::from(LibvirtError::Forbidden(
            "Browsing host paths requires the admin role.".into(),
        )))
    }
}

fn require_root_session(actor: &RequestActor) -> Result<(), LibvirtError> {
    if actor.from_api_token {
        return Err(LibvirtError::Forbidden(
            "Session administration requires a browser login as root".into(),
        ));
    }
    if actor.username != "root" {
        return Err(LibvirtError::Forbidden(
            "Only the root user may list or revoke web sessions".into(),
        ));
    }
    Ok(())
}

async fn admin_list_sessions(
    Extension(store): Extension<SessionStore>,
    Extension(actor): Extension<RequestActor>,
    req: Request<Body>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_root_session(&actor)?;
    let current_public_id = extract_token(&req).and_then(|t| store.session_public_id(&t));
    let list = store.list_browser_sessions(current_public_id.as_deref());
    let total_sessions = list.len();
    let by_user: std::collections::HashMap<String, usize> =
        list.iter()
            .fold(std::collections::HashMap::new(), |mut acc, s| {
                *acc.entry(s.username.clone()).or_insert(0) += 1;
                acc
            });
    Ok(Json(serde_json::json!({
        "sessions": list,
        "total_sessions": total_sessions,
        "users_logged_in": by_user.len(),
        "sessions_per_username": by_user,
    })))
}

async fn admin_revoke_session(
    Extension(store): Extension<SessionStore>,
    Extension(actor): Extension<RequestActor>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_root_session(&actor)?;
    if session_id.chars().count() != 32 || !session_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(LibvirtError::Invalid("Invalid session_id".into()).into());
    }
    if store.revoke_session_by_public_id(&session_id) {
        info!("Session {} revoked by root", session_id);
        Ok(Json(
            serde_json::json!({ "status": "revoked", "session_id": session_id }),
        ))
    } else {
        Err(LibvirtError::NotFound("Session not found or already expired".into()).into())
    }
}

#[cfg(target_os = "linux")]
fn pam_authenticate(username: &str, password: &str, pam_service: &str) -> Result<(), String> {
    use pam_client::conv_mock::Conversation;
    use pam_client::{Context, Flag};

    let mut ctx = Context::new(
        pam_service,
        None,
        Conversation::with_credentials(username, password),
    )
    .map_err(|e| format!("PAM init failed ({pam_service}): {e}"))?;
    ctx.authenticate(Flag::NONE)
        .map_err(|e| format!("PAM auth failed: {e}"))?;
    ctx.acct_mgmt(Flag::NONE)
        .map_err(|e| format!("PAM auth failed: {e}"))?;
    // Skip open_session() — pam_loginuid fails under systemd with NoNewPrivileges.
    // We only need credential verification, not a full login session.
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn pam_authenticate(_username: &str, _password: &str, _pam_service: &str) -> Result<(), String> {
    Err("PAM authentication is only compiled on Linux".into())
}

#[derive(Debug, Deserialize)]
struct TokenSessionRequest {
    token: String,
}

/// Exchange a platform-issued JWT (`?token=` deep link) for a browser session cookie.
async fn token_session_handler(
    Extension(store): Extension<SessionStore>,
    Extension(auth): Extension<OidcAuth>,
    Extension(stats): Extension<std::sync::Arc<crate::daemon_stats::DaemonStats>>,
    Json(req): Json<TokenSessionRequest>,
) -> Response {
    let token = req.token.trim();
    if token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "token required", "error_code": "invalid_request" })),
        )
            .into_response();
    }
    if token.starts_with("mach_") || token.starts_with("vs_") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "API automation tokens cannot be exchanged for browser sessions",
                "error_code": "invalid_request"
            })),
        )
            .into_response();
    }
    let Some(actor) = actor_from_platform_jwt(token) else {
        stats.inc_auth_attempt("oidc", "failure");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid or expired token", "error_code": "unauthorized" })),
        )
            .into_response();
    };
    let session_token = store.create_session(actor.clone());
    let cookie = session_cookie(&session_token, auth.0.tls_enabled);
    stats.inc_auth_attempt("oidc", "success");
    info!(
        "Platform JWT exchanged for browser session for user '{}'",
        actor.username
    );
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({
            "status": "ok",
            "username": actor.username,
            "role": actor.role,
            "auth_source": actor.auth_source,
        })),
    )
        .into_response()
}

async fn saml_metadata_handler(Extension(auth): Extension<OidcAuth>) -> Response {
    let saml = &auth.0.saml;
    if !saml.is_configured() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "SAML is not configured" })),
        )
            .into_response();
    }
    let entity = xml_escape(&saml.sp_entity_id);
    let acs = xml_escape(if saml.sp_acs_url.trim().is_empty() {
        "/api/v1/auth/saml/acs"
    } else {
        saml.sp_acs_url.as_str()
    });
    let xml = format!(
        r#"<?xml version="1.0"?>
<EntityDescriptor xmlns="urn:oasis:names:tc:SAML:2.0:metadata" entityID="{entity}">
  <SPSSODescriptor AuthnRequestsSigned="false" WantAssertionsSigned="true" protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <NameIDFormat>{nameid}</NameIDFormat>
    <AssertionConsumerService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="{acs}" index="1"/>
  </SPSSODescriptor>
</EntityDescriptor>"#,
        entity = entity,
        acs = acs,
        nameid = xml_escape(&saml.name_id_format),
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/samlmetadata+xml")],
        xml,
    )
        .into_response()
}

fn xml_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn run_as_user_status_handler(Extension(auth): Extension<OidcAuth>) -> Response {
    let cfg = &auth.0.run_as_user;
    let mode = serde_json::to_value(&cfg.mode).unwrap_or(serde_json::json!("disabled"));
    Json(serde_json::json!({
        "enabled": cfg.wants_impersonation(),
        "mode": mode,
        "impersonation_active": cfg.impersonation_active(),
        "setuid_helper_path": cfg.setuid_helper_path,
        "prefer_session_libvirt_on_impersonation": cfg.prefer_session_libvirt_on_impersonation,
        "supported_programs": ["useradd", "userdel", "usermod", "homectl", "chpasswd", "id", "getent"],
    }))
    .into_response()
}

/// Auth routes — these use Extension<SessionStore> so they can be merged
/// into Router<LibvirtManager> without state conflicts.
pub fn auth_routes(session_store: SessionStore, auth_cfg: AuthConfig) -> Router<LibvirtManager> {
    Router::new()
        .route("/auth/providers", get(auth_providers_handler))
        .route("/auth/run-as-user", get(run_as_user_status_handler))
        .route("/auth/login", post(login_handler))
        .route("/auth/oidc/login", get(oidc_login_handler))
        .route("/auth/oidc/callback", get(oidc_callback_handler))
        .route("/auth/native/exchange", post(native_exchange_handler))
        .route("/auth/token/session", post(token_session_handler))
        .route("/auth/saml/metadata", get(saml_metadata_handler))
        .route("/auth/logout", post(logout_handler))
        .route("/auth/session", get(session_handler))
        .route("/ws-token", post(ws_token_handler))
        .route("/admin/sessions", get(admin_list_sessions))
        .route("/admin/sessions/{session_id}", delete(admin_revoke_session))
        // Scoped to /auth/login, /auth/oidc/login, /auth/oidc/callback only — see
        // auth_rate_limit_middleware doc comment. Every other route above (providers,
        // session, logout, ws-token, admin/sessions) passes through untouched.
        .layer(middleware::from_fn_with_state(
            AuthRateLimiter::new(),
            auth_rate_limit_middleware,
        ))
        .layer(Extension(session_store))
        .layer(Extension(OidcAuth(std::sync::Arc::new(auth_cfg))))
}
