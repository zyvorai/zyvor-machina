// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Shared OIDC Authorization Code Flow protocol logic — discovery, JWKS, and
//! id_token validation — used by both `machina-daemon` (single-host, cookie
//! sessions, Unix-user mapping) and `machina-controller` (multi-host, JWT
//! sessions, no Unix-user concept). Everything here is deliberately protocol-level
//! only: session issuance, config storage, and state/nonce persistence stay in
//! each caller, since those differ for real architectural reasons rather than
//! being accidental duplication. See `/Users/ssahani/.claude/plans/lazy-munching-quilt.md`
//! for the unification rationale — this module previously existed as two
//! independently hand-written implementations that could (and did) drift apart.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("fetch OIDC discovery: {0}")]
    DiscoveryFetch(String),
    #[error("decode OIDC discovery document: {0}")]
    DiscoveryDecode(String),
    #[error("{0}")]
    DiscoveryInvalid(String),
    #[error("fetch OIDC JWKS: {0}")]
    JwksFetch(String),
    #[error("decode OIDC JWKS: {0}")]
    JwksDecode(String),
    #[error("{0}")]
    TokenInvalid(String),
}

/// Parsed `.well-known/openid-configuration` response. `jwks_uri` and
/// `userinfo_endpoint` are optional in the shared type since callers differ on
/// whether they're required — the daemon has always treated `jwks_uri` as
/// mandatory (it never calls `userinfo_endpoint`) and must keep erroring out
/// itself if it's absent, matching its pre-unification behavior exactly.
#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscoveryDocument {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub jwks_uri: Option<String>,
    pub issuer: String,
}

const OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn oidc_http_client() -> Result<reqwest::Client, OidcError> {
    reqwest::Client::builder()
        .timeout(OIDC_HTTP_TIMEOUT)
        .build()
        .map_err(|e| OidcError::DiscoveryFetch(format!("build HTTP client: {e}")))
}

fn origin_of(url: &str) -> String {
    url.trim_end_matches('/')
        .split('/')
        .take(3)
        .collect::<Vec<_>>()
        .join("/")
}

/// Validate an already-fetched discovery document body against the issuer it was
/// fetched from. Pure — no I/O — so this is the primary thing unit-tested.
///
/// - `require_https`: the controller has always required an `https://` issuer;
///   the daemon never enforced this (needed for plain-HTTP internal test IdPs).
///   Preserved per-caller rather than picked one way, since it's a real behavior
///   difference, not accidental duplication.
pub fn validate_discovery_document(
    body: &str,
    issuer_url: &str,
    require_https: bool,
) -> Result<OidcDiscoveryDocument, OidcError> {
    if require_https && !issuer_url.starts_with("https://") {
        return Err(OidcError::DiscoveryInvalid(format!(
            "OIDC issuer must use HTTPS — got: {issuer_url}"
        )));
    }
    let doc: OidcDiscoveryDocument =
        serde_json::from_str(body).map_err(|e| OidcError::DiscoveryDecode(e.to_string()))?;

    // OIDC Discovery 1.0 §4.3: the `issuer` in the discovery document MUST exactly
    // match the URL it was fetched from. `doc.issuer` is what later pins the
    // id_token's `iss` validation, so skipping this check would let a
    // compromised/misconfigured discovery response redirect trust elsewhere.
    let base = issuer_url.trim_end_matches('/');
    if doc.issuer.trim_end_matches('/') != base {
        return Err(OidcError::DiscoveryInvalid(format!(
            "OIDC discovery document issuer '{}' does not match configured issuer_url '{issuer_url}'",
            doc.issuer
        )));
    }

    // SSRF hardening: token/userinfo/jwks endpoints must share the issuer's own
    // origin — otherwise a compromised or misconfigured discovery response could
    // redirect the token exchange or JWKS fetch to an attacker-controlled host.
    let issuer_origin = origin_of(base);
    if !origin_of(&doc.token_endpoint).eq_ignore_ascii_case(&issuer_origin) {
        return Err(OidcError::DiscoveryInvalid(format!(
            "OIDC token_endpoint '{}' does not match issuer origin '{issuer_origin}'",
            doc.token_endpoint
        )));
    }
    if let Some(ref ui) = doc.userinfo_endpoint {
        if !origin_of(ui).eq_ignore_ascii_case(&issuer_origin) {
            return Err(OidcError::DiscoveryInvalid(format!(
                "OIDC userinfo_endpoint '{ui}' does not match issuer origin '{issuer_origin}'"
            )));
        }
    }
    if let Some(ref jwks) = doc.jwks_uri {
        if !origin_of(jwks).eq_ignore_ascii_case(&issuer_origin) {
            return Err(OidcError::DiscoveryInvalid(format!(
                "OIDC jwks_uri '{jwks}' does not match issuer origin '{issuer_origin}'"
            )));
        }
    }
    Ok(doc)
}

pub async fn fetch_discovery(
    client: &reqwest::Client,
    issuer_url: &str,
    require_https: bool,
) -> Result<OidcDiscoveryDocument, OidcError> {
    let base = issuer_url.trim_end_matches('/');
    let url = format!("{base}/.well-known/openid-configuration");
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| OidcError::DiscoveryFetch(e.to_string()))?;
    if !res.status().is_success() {
        return Err(OidcError::DiscoveryFetch(format!("HTTP {}", res.status())));
    }
    let body = res
        .text()
        .await
        .map_err(|e| OidcError::DiscoveryFetch(e.to_string()))?;
    validate_discovery_document(&body, issuer_url, require_https)
}

pub async fn fetch_jwks(client: &reqwest::Client, jwks_uri: &str) -> Result<JwkSet, OidcError> {
    let res = client
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| OidcError::JwksFetch(e.to_string()))?;
    if !res.status().is_success() {
        return Err(OidcError::JwksFetch(format!("HTTP {}", res.status())));
    }
    res.json::<JwkSet>()
        .await
        .map_err(|e| OidcError::JwksDecode(e.to_string()))
}

/// Signing algorithms it is safe to accept for a JWK of this key type, derived from
/// the JWK's own (server-published, trusted) key parameters — never from the
/// `alg` field of the token header, which an attacker fully controls. This closes
/// the classic alg-confusion class of attack (e.g. an attacker requesting `HS256`
/// and trying to use the RSA public key bytes as an HMAC secret): the returned set
/// only ever contains algorithms whose key family matches the JWK, so a header
/// claiming a mismatched algorithm is rejected outright.
pub fn allowed_algorithms_for_jwk(jwk: &jsonwebtoken::jwk::Jwk) -> Vec<Algorithm> {
    match &jwk.algorithm {
        AlgorithmParameters::RSA(_) => vec![
            Algorithm::RS256,
            Algorithm::RS384,
            Algorithm::RS512,
            Algorithm::PS256,
            Algorithm::PS384,
            Algorithm::PS512,
        ],
        AlgorithmParameters::EllipticCurve(params) => match params.curve {
            EllipticCurve::P256 => vec![Algorithm::ES256],
            EllipticCurve::P384 => vec![Algorithm::ES384],
            // P-521 and any future curve variant: no jsonwebtoken Algorithm maps to
            // it, so there is nothing safe to accept.
            _ => vec![],
        },
        AlgorithmParameters::OctetKeyPair(_) => vec![Algorithm::EdDSA],
        // A symmetric (HMAC) key published in a *public* JWKS would mean the
        // "secret" is public too — never usable for signature verification.
        AlgorithmParameters::OctetKey(_) => vec![],
        // Any key type a future jsonwebtoken adds (post-quantum, …) is refused until it is deliberately supported here.
        #[allow(unreachable_patterns)]
        _ => vec![],
    }
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    sub: String,
    exp: usize,
    #[serde(default)]
    nbf: Option<usize>,
    #[serde(default)]
    iss: Option<String>,
    #[serde(default)]
    aud: Option<serde_json::Value>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct OidcClaims {
    pub sub: String,
    pub exp: usize,
    pub nbf: Option<usize>,
    pub iss: Option<String>,
    pub aud: Option<serde_json::Value>,
    pub nonce: Option<String>,
    /// Every other claim, flattened — this is where `username_claim`/`groups_claim`
    /// lookups happen, since those are caller-configurable claim *names*.
    pub extra: HashMap<String, serde_json::Value>,
}

/// Verify an id_token's signature, issuer, audience, expiry, and (when supplied)
/// nonce. Pure — no I/O, no config type dependency — so both daemon's `Role` enum
/// and controller's plain role strings can build their own claim→role logic on
/// top of the returned claims without this module knowing about either shape.
pub fn validate_id_token(
    id_token: &str,
    jwks: &JwkSet,
    issuer: &str,
    audience: &str,
    expected_nonce: Option<&str>,
) -> Result<OidcClaims, OidcError> {
    let header = decode_header(id_token)
        .map_err(|e| OidcError::TokenInvalid(format!("decode header: {e}")))?;
    let kid = header
        .kid
        .ok_or_else(|| OidcError::TokenInvalid("id_token is missing key id".into()))?;
    let jwk = jwks
        .find(&kid)
        .ok_or_else(|| OidcError::TokenInvalid(format!("signing key '{kid}' not found in JWKS")))?;
    let key = DecodingKey::from_jwk(jwk)
        .map_err(|e| OidcError::TokenInvalid(format!("build decoding key from JWKS: {e}")))?;

    // Pin the accepted signing algorithm(s) to what this JWK's own key type
    // supports — never trust `header.alg` (attacker-controlled) on its own.
    let allowed_algs = allowed_algorithms_for_jwk(jwk);
    if allowed_algs.is_empty() {
        return Err(OidcError::TokenInvalid(format!(
            "signing key '{kid}' has an unsupported or unsafe key type for id_token verification"
        )));
    }
    if !allowed_algs.contains(&header.alg) {
        return Err(OidcError::TokenInvalid(format!(
            "id_token alg {:?} is not permitted for signing key '{kid}'",
            header.alg
        )));
    }

    let mut validation = Validation::new(header.alg);
    validation.algorithms = allowed_algs;
    validation.set_audience(&[audience.trim()]);
    validation.set_issuer(&[issuer]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.validate_nbf = true;

    let token = decode::<RawClaims>(id_token, &key, &validation)
        .map_err(|e| OidcError::TokenInvalid(format!("validate id_token: {e}")))?;
    let claims = token.claims;

    if let Some(expected) = expected_nonce {
        if claims.nonce.as_deref() != Some(expected) {
            return Err(OidcError::TokenInvalid("nonce mismatch".into()));
        }
    }

    Ok(OidcClaims {
        sub: claims.sub,
        exp: claims.exp,
        nbf: claims.nbf,
        iss: claims.iss,
        aud: claims.aud,
        nonce: claims.nonce,
        extra: claims.extra,
    })
}

pub fn claim_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        _ => None,
    }
}

pub fn claim_strings(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Which configured group tier a set of OIDC groups matched, if any. Deliberately
/// stops short of returning a caller's own role type — the daemon inserts a local
/// role-override lookup between "no group matched" and "fall back to default",
/// which only makes sense on the daemon side, so callers finish the mapping
/// themselves after calling this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleTier {
    Admin,
    Operator,
    NoMatch,
}

pub fn resolve_role_tier(
    groups: &[String],
    admin_groups: &[String],
    operator_groups: &[String],
) -> RoleTier {
    if groups
        .iter()
        .any(|g| admin_groups.iter().any(|want| want == g))
    {
        return RoleTier::Admin;
    }
    if groups
        .iter()
        .any(|g| operator_groups.iter().any(|want| want == g))
    {
        return RoleTier::Operator;
    }
    RoleTier::NoMatch
}

/// State/nonce pair for one in-flight login attempt. Only the value type and TTL
/// are shared — each caller keeps its own storage backend (daemon: in-memory
/// `Mutex<HashMap>`; controller: a SQLite table), since those differ by process
/// lifetime/scale, not by protocol.
#[derive(Debug, Clone)]
pub struct OidcStateEntry {
    pub nonce: String,
    pub created_at: Instant,
}

/// Converged TTL for OIDC login state/nonce entries — was 300s on the daemon,
/// 600s on the controller pre-unification; picked the tighter of the two rather
/// than silently keeping whichever a given call site happened to use.
pub const OIDC_STATE_TTL_SECS: u64 = 300;

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const ISSUER: &str = "https://idp.example.com/realms/machina";
    const AUDIENCE: &str = "machina-client";

    // A fixed, throwaway 2048-bit RSA test keypair — never used for anything but
    // signing/verifying fixture tokens in this test module. Generated once via
    // `openssl genrsa` for a deterministic, dependency-free test suite (avoids
    // pulling in an RSA-keygen crate + RNG just for tests).
    const TEST_RSA_PRIVATE_KEY_PEM: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAvirR0TQjXzjK5V3NCeeDnsh8gfJrfEmKE2zqCwevm+oq19SZ
/Qfk03+OkVQZ17sD3UdW5IwcY/sJXETs6MZiGes1+99jQyLRSwl+9sf5JXATbV5i
RghuFX5+CjEPsP91p6PwmpjGyywpK4vWtNZAzb9dFenQwnpoEK9ssItqT78S9/BQ
melaLSNqjgfwVC4UIkPrXWurlfYSn9fbXJxVC5R5ZeBxfoNXZY6roMcyOX4L5x+1
wKzXVfH1czhnjJDc5gCJbVaHx6a2uCvbqwa/OKmBUILR433KbduLJX7+e2VDnpNi
BhG/iiDwqB/MaxxS+aVMKuWpoSzNl7lBZV2UbQIDAQABAoIBAENRRiAADGt3X7+t
IlmYtmudfhHEHo+LOtEgk4MfD+eqD8uCa5Z6VmMWOwIwGsvW3InW6KgT/zLXWVtr
3M+T9oNFU8FbNTi9WQhujp7OcGBz2bS0Hia7cjiyo+x4rJzT+bLD4nbfkTO36MUN
Bg7S73LGBK4urGND0JXje57NY29hz8F2jJUysrAR3Miv7kTmcbgjGRIOEnn+CBZX
Y/ksYxVHZSAhdR7jZVIG6k+Tm1iftPIhrHdzVGfFsqTKfom5eudveuW4ezkh+kxi
2J60N1AnNUR9yCrggZddYkhPqi5zTi363TNMhzqpHnqZNxQY96j6nZJK9QXPODEP
9fy75YMCgYEA9SHw6WjXKMsnEUa7y9YHPtvgr4PPiPOhvEj9IEMbMbjyl+JBCXNw
hJgK+BzwOJ3PZmYooLhrkqD3WiRV/rj8nWZbkvv/gXQGiPE9F0Mv5MD39QgiGDss
dyYdO3gx6nL6sdFcSCx0vccpBldNGRfEIXgZKWPzPRbqyOqNTdM7AWcCgYEAxpkQ
65KPSg2uDxurVu3INu5rvUClINRHLAXEGcbd8VKYDjiV7iqduH8yzU2gh9ObCBFT
WQ9XbBPkl+Sc7mY8YIxsgskd8VZdmJg81+MJyLEq63R6QJaa430pqpnUtiuW8E+U
Nb/iIjIxzrLs0r++WwIauG5X5rxR2mNnJCHrMwsCgYB8vcbgoC9NXQQGcJ5EPif1
vuJ5rnO/12roa6QT9NIz3U/mJoa+DnalotGCLQe5Z+UQ0M+/6dkkBrGYt4DjXLOA
TYQwKfh9odNpgIl7+v62Q9RqZrci6YnZIBYkGygbjGMydb2mJKlLobuotGhRapyu
A3WacHhpD+5uS52YivMt5QKBgGVQBsK2dKTJj8cjTWg+S4pCXPIh/VtfD6PUmTKx
Md3/TZTLpyjl0qohMbBsbn18JLWb6RIg8m8vQsl+FdTEkP6MBHs/0Cei8IJ8/2T+
7KnWP9f4BrnWWtO8sTnX2hzI5epYHnrBFcJuKtyQiKIGsTxKOYlmuS77WSJ43VAg
gZT7AoGBANjksOexYN2aWFliK3+EbrcSgBxFUbN9BoM4hLYL5edFB4qRT+GNWH9Q
f/Eb80NkGviQvIKKh5VmVgMwdHPW8V56eKxT6pY4koRmevqrtrcZpthbWgVnjksC
Pd7Jjl0b0utJgQ+XgFYY4EvvJr0eFWGnDte2uFeeL8bV6A1nUsgo
-----END RSA PRIVATE KEY-----";
    const TEST_RSA_N: &str = "virR0TQjXzjK5V3NCeeDnsh8gfJrfEmKE2zqCwevm-oq19SZ_Qfk03-OkVQZ17sD3UdW5IwcY_sJXETs6MZiGes1-99jQyLRSwl-9sf5JXATbV5iRghuFX5-CjEPsP91p6PwmpjGyywpK4vWtNZAzb9dFenQwnpoEK9ssItqT78S9_BQmelaLSNqjgfwVC4UIkPrXWurlfYSn9fbXJxVC5R5ZeBxfoNXZY6roMcyOX4L5x-1wKzXVfH1czhnjJDc5gCJbVaHx6a2uCvbqwa_OKmBUILR433KbduLJX7-e2VDnpNiBhG_iiDwqB_MaxxS-aVMKuWpoSzNl7lBZV2UbQ";
    const TEST_RSA_E: &str = "AQAB";

    fn jwks_with_kid(kid: &str) -> JwkSet {
        let jwk_json = serde_json::json!({
            "kty": "RSA",
            "kid": kid,
            "n": TEST_RSA_N,
            "e": TEST_RSA_E,
        });
        serde_json::from_value(serde_json::json!({ "keys": [jwk_json] })).unwrap()
    }

    fn sign_token(kid: &str, claims: serde_json::Value) -> String {
        let encoding_key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY_PEM.as_bytes())
            .expect("parse test RSA private key");
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        encode(&header, &claims, &encoding_key).unwrap()
    }

    fn base_claims(overrides: serde_json::Value) -> serde_json::Value {
        // jsonwebtoken validates nbf/exp against the real system clock, so these
        // must be relative to actual "now" — not a fixed placeholder timestamp.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let mut claims = serde_json::json!({
            "sub": "user-123",
            "iss": ISSUER,
            "aud": AUDIENCE,
            "exp": now + 300,
            "nbf": now - 10,
            "nonce": "expected-nonce",
        });
        if let serde_json::Value::Object(o) = overrides {
            for (k, v) in o {
                claims[k] = v;
            }
        }
        claims
    }

    #[test]
    fn valid_token_round_trips() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token("kid-1", base_claims(serde_json::json!({})));
        let claims =
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("expected-nonce")).unwrap();
        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn wrong_audience_rejected() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token(
            "kid-1",
            base_claims(serde_json::json!({"aud": "someone-else"})),
        );
        assert!(
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("expected-nonce")).is_err()
        );
    }

    #[test]
    fn wrong_issuer_rejected() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token(
            "kid-1",
            base_claims(serde_json::json!({"iss": "https://evil.example.com"})),
        );
        assert!(
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("expected-nonce")).is_err()
        );
    }

    #[test]
    fn expired_token_rejected() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token("kid-1", base_claims(serde_json::json!({"exp": 1})));
        assert!(
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("expected-nonce")).is_err()
        );
    }

    #[test]
    fn nonce_mismatch_rejected() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token("kid-1", base_claims(serde_json::json!({})));
        assert!(
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("different-nonce")).is_err()
        );
    }

    #[test]
    fn missing_nonce_ok_when_not_required() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token("kid-1", base_claims(serde_json::json!({})));
        assert!(validate_id_token(&token, &jwks, ISSUER, AUDIENCE, None).is_ok());
    }

    #[test]
    fn unknown_kid_rejected() {
        let jwks = jwks_with_kid("kid-1");
        let token = sign_token("kid-other", base_claims(serde_json::json!({})));
        assert!(
            validate_id_token(&token, &jwks, ISSUER, AUDIENCE, Some("expected-nonce")).is_err()
        );
    }

    #[test]
    fn hs256_header_against_rsa_jwk_rejected() {
        // The classic alg-confusion attack: claim HS256 in the header (and sign
        // with, say, the RSA public modulus as an HMAC secret) against a JWKS
        // that only publishes an RSA key. allowed_algorithms_for_jwk must reject
        // this before jsonwebtoken ever tries to verify the signature.
        let jwks = jwks_with_kid("kid-1");
        let jwk = jwks.keys.first().unwrap();
        let allowed = allowed_algorithms_for_jwk(jwk);
        assert!(!allowed.contains(&Algorithm::HS256));
        assert!(allowed.contains(&Algorithm::RS256));
    }

    #[test]
    fn ec_p521_jwk_has_no_allowed_algorithms() {
        let jwk_json = serde_json::json!({
            "kty": "EC",
            "kid": "ec-1",
            "crv": "P-521",
            "x": "AA",
            "y": "AA",
        });
        let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(jwk_json).unwrap();
        assert!(allowed_algorithms_for_jwk(&jwk).is_empty());
    }

    #[test]
    fn octet_key_jwk_has_no_allowed_algorithms() {
        let jwk_json = serde_json::json!({
            "kty": "oct",
            "kid": "oct-1",
            "k": "c2VjcmV0",
        });
        let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(jwk_json).unwrap();
        assert!(allowed_algorithms_for_jwk(&jwk).is_empty());
    }

    #[test]
    fn discovery_document_matching_issuer_passes() {
        let body = serde_json::json!({
            "issuer": ISSUER,
            "authorization_endpoint": format!("{ISSUER}/protocol/openid-connect/auth"),
            "token_endpoint": format!("{ISSUER}/protocol/openid-connect/token"),
            "jwks_uri": format!("{ISSUER}/protocol/openid-connect/certs"),
        })
        .to_string();
        assert!(validate_discovery_document(&body, ISSUER, true).is_ok());
    }

    #[test]
    fn discovery_document_mismatched_issuer_rejected() {
        let body = serde_json::json!({
            "issuer": "https://different.example.com",
            "authorization_endpoint": format!("{ISSUER}/auth"),
            "token_endpoint": format!("{ISSUER}/token"),
        })
        .to_string();
        assert!(validate_discovery_document(&body, ISSUER, true).is_err());
    }

    #[test]
    fn discovery_document_cross_origin_token_endpoint_rejected() {
        let body = serde_json::json!({
            "issuer": ISSUER,
            "authorization_endpoint": format!("{ISSUER}/auth"),
            "token_endpoint": "https://attacker.example.com/token",
        })
        .to_string();
        assert!(validate_discovery_document(&body, ISSUER, true).is_err());
    }

    #[test]
    fn discovery_document_requires_https_when_asked() {
        let http_issuer = "http://idp.internal.test:8080/realms/machina";
        let body = serde_json::json!({
            "issuer": http_issuer,
            "authorization_endpoint": format!("{http_issuer}/auth"),
            "token_endpoint": format!("{http_issuer}/token"),
        })
        .to_string();
        assert!(validate_discovery_document(&body, http_issuer, true).is_err());
        assert!(validate_discovery_document(&body, http_issuer, false).is_ok());
    }

    #[test]
    fn resolve_role_tier_admin_wins_over_operator() {
        let groups = vec!["machina-admins".to_string(), "machina-ops".to_string()];
        let tier = resolve_role_tier(
            &groups,
            &["machina-admins".to_string()],
            &["machina-ops".to_string()],
        );
        assert_eq!(tier, RoleTier::Admin);
    }

    #[test]
    fn resolve_role_tier_operator_when_no_admin_match() {
        let groups = vec!["machina-ops".to_string()];
        let tier = resolve_role_tier(
            &groups,
            &["machina-admins".to_string()],
            &["machina-ops".to_string()],
        );
        assert_eq!(tier, RoleTier::Operator);
    }

    #[test]
    fn resolve_role_tier_no_match() {
        let groups = vec!["some-other-group".to_string()];
        let tier = resolve_role_tier(
            &groups,
            &["machina-admins".to_string()],
            &["machina-ops".to_string()],
        );
        assert_eq!(tier, RoleTier::NoMatch);
    }

    #[test]
    fn claim_string_and_strings() {
        let v = serde_json::json!("hello");
        assert_eq!(claim_string(Some(&v)), Some("hello".to_string()));
        let arr = serde_json::json!(["a", "b", 1]);
        assert_eq!(
            claim_strings(Some(&arr)),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(claim_string(None), None);
        assert!(claim_strings(None).is_empty());
    }

    // --- thin I/O wrapper coverage — a real HTTP round-trip via wiremock, not
    // exhaustive negative-path testing (that lives in the pure-function tests
    // above). Just proving fetch_discovery/fetch_jwks actually parse a real response.

    #[tokio::test]
    async fn fetch_discovery_parses_real_http_response() {
        let server = wiremock::MockServer::start().await;
        let issuer = server.uri();
        let body = serde_json::json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/auth"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/certs"),
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/.well-known/openid-configuration",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = oidc_http_client().unwrap();
        let doc = fetch_discovery(&client, &issuer, false).await.unwrap();
        assert_eq!(doc.token_endpoint, format!("{issuer}/token"));
    }

    #[tokio::test]
    async fn fetch_discovery_propagates_http_error_status() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/.well-known/openid-configuration",
            ))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = oidc_http_client().unwrap();
        assert!(fetch_discovery(&client, &server.uri(), false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn fetch_jwks_parses_real_http_response() {
        let server = wiremock::MockServer::start().await;
        let jwks_body = serde_json::json!({
            "keys": [{ "kty": "RSA", "kid": "kid-1", "n": TEST_RSA_N, "e": TEST_RSA_E }],
        });
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/certs"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(&jwks_body))
            .mount(&server)
            .await;

        let client = oidc_http_client().unwrap();
        let jwks = fetch_jwks(&client, &format!("{}/certs", server.uri()))
            .await
            .unwrap();
        assert_eq!(jwks.keys.len(), 1);
    }
}
