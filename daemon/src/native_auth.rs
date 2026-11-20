// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Native-app SSO support (issue #224).
//!
//! A native client (the Mac app) starts OIDC login with `native=1`, a fixed
//! `redirect_uri` (`machina://auth`) and a PKCE S256 `code_challenge`. After the IdP
//! callback validates, the daemon redirects to `machina://auth?code=...` with a
//! single-use, 60-second exchange code instead of setting a cookie. The client then
//! calls `POST /auth/native/exchange {code, code_verifier}` to receive the session.
//!
//! This module holds only the in-memory stores and the pure validation helpers; the
//! HTTP handlers live in `auth.rs`. Codes, verifiers and challenges are never logged.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The only redirect target a native flow may use (exact match, no other schemes).
pub const NATIVE_REDIRECT_URI: &str = "machina://auth";

/// Lifetime of an exchange code.
pub const NATIVE_CODE_TTL: Duration = Duration::from_secs(60);

/// Lifetime of a pending native login (start route to IdP callback).
pub const NATIVE_FLOW_TTL: Duration = Duration::from_secs(600);

/// Cap on entries per map, so an anonymous caller cannot grow memory without bound.
const MAX_ENTRIES: usize = 10_000;

pub fn redirect_uri_allowed(uri: &str) -> bool {
    uri == NATIVE_REDIRECT_URI
}

fn is_b64url(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A S256 challenge is the unpadded base64url of a SHA-256 digest: 43 characters.
pub fn challenge_valid(challenge: &str) -> bool {
    challenge.len() == 43 && is_b64url(challenge)
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// RFC 7636 verifier: 43..=128 unreserved characters.
pub fn verifier_valid(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len()) && verifier.bytes().all(is_unreserved)
}

/// Optional client `state`: short and URL-safe, so it can be echoed without escaping.
pub fn client_state_valid(state: &str) -> bool {
    !state.is_empty() && state.len() <= 128 && state.bytes().all(is_unreserved)
}

pub fn challenge_for_verifier(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Build `machina://auth?code=...[&state=...]`. `code` is hex and `state` is validated
/// URL-safe, so no percent-encoding is needed.
pub fn success_redirect(code: &str, client_state: Option<&str>) -> String {
    let mut url = format!("{NATIVE_REDIRECT_URI}?code={code}");
    if let Some(s) = client_state {
        url.push_str("&state=");
        url.push_str(s);
    }
    url
}

/// `machina://auth?error=...[&state=...]` for a failed IdP round trip.
pub fn error_redirect(error: &str, client_state: Option<&str>) -> String {
    let mut url = format!("{NATIVE_REDIRECT_URI}?error={error}");
    if let Some(s) = client_state {
        url.push_str("&state=");
        url.push_str(s);
    }
    url
}

/// Native parameters remembered next to the OIDC state/nonce.
#[derive(Clone, Debug)]
pub struct NativeFlow {
    pub code_challenge: String,
    pub client_state: Option<String>,
    created_at: Instant,
}

struct CodeEntry<T> {
    payload: T,
    code_challenge: String,
    created_at: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RedeemError {
    /// Unknown, expired or already used code.
    InvalidCode,
    /// Verifier malformed or not matching the stored challenge (the code is consumed).
    InvalidVerifier,
}

/// In-memory store: pending native logins keyed by OIDC state, and exchange codes.
#[derive(Clone)]
pub struct NativeAuthStore<T: Clone> {
    flows: Arc<Mutex<HashMap<String, NativeFlow>>>,
    codes: Arc<Mutex<HashMap<String, CodeEntry<T>>>>,
}

impl<T: Clone> NativeAuthStore<T> {
    pub fn new() -> Self {
        Self {
            flows: Arc::new(Mutex::new(HashMap::new())),
            codes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Remember a native login under the OIDC `state` it was started with.
    pub fn begin(&self, oidc_state: &str, code_challenge: &str, client_state: Option<String>) {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.retain(|_, f| f.created_at.elapsed() < NATIVE_FLOW_TTL);
        if flows.len() >= MAX_ENTRIES {
            flows.clear();
        }
        flows.insert(
            oidc_state.to_string(),
            NativeFlow {
                code_challenge: code_challenge.to_string(),
                client_state,
                created_at: Instant::now(),
            },
        );
    }

    /// Take (and remove) the native flow for an OIDC state, if it is one and unexpired.
    pub fn take_flow(&self, oidc_state: &str) -> Option<NativeFlow> {
        let mut flows = self.flows.lock().unwrap_or_else(|e| e.into_inner());
        flows.retain(|_, f| f.created_at.elapsed() < NATIVE_FLOW_TTL);
        flows.remove(oidc_state)
    }

    /// Mint a single-use exchange code bound to `payload` and the PKCE challenge.
    pub fn mint(&self, payload: T, code_challenge: &str) -> String {
        let mut rng = rand::thread_rng();
        let bytes: [u8; 32] = rng.gen();
        let code = hex::encode(bytes);
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes.retain(|_, c| c.created_at.elapsed() < NATIVE_CODE_TTL);
        if codes.len() >= MAX_ENTRIES {
            codes.clear();
        }
        codes.insert(
            code.clone(),
            CodeEntry {
                payload,
                code_challenge: code_challenge.to_string(),
                created_at: Instant::now(),
            },
        );
        code
    }

    /// Redeem a code. The code is removed on any attempt that finds it, so a wrong
    /// verifier cannot be retried.
    pub fn redeem(&self, code: &str, verifier: &str) -> Result<T, RedeemError> {
        let entry = {
            let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
            codes.retain(|_, c| c.created_at.elapsed() < NATIVE_CODE_TTL);
            codes.remove(code)
        };
        let entry = entry.ok_or(RedeemError::InvalidCode)?;
        if !verifier_valid(verifier) {
            return Err(RedeemError::InvalidVerifier);
        }
        let computed = challenge_for_verifier(verifier);
        if !ct_eq(computed.as_bytes(), entry.code_challenge.as_bytes()) {
            return Err(RedeemError::InvalidVerifier);
        }
        Ok(entry.payload)
    }

    #[cfg(test)]
    fn backdate_codes(&self, by: Duration) {
        let mut codes = self.codes.lock().unwrap();
        for c in codes.values_mut() {
            c.created_at = c.created_at.checked_sub(by).unwrap();
        }
    }
}

impl<T: Clone> Default for NativeAuthStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

    #[test]
    fn rfc7636_vector() {
        assert_eq!(
            challenge_for_verifier(VERIFIER),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn redirect_uri_allowlist_is_exact() {
        assert!(redirect_uri_allowed("machina://auth"));
        for bad in [
            "",
            "machina://auth/",
            "machina://auth?x=1",
            "machina://other",
            "Machina://auth",
            "https://example.com",
            "http://machina://auth",
            "machina://auth@evil.com",
            " machina://auth",
        ] {
            assert!(!redirect_uri_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn challenge_and_state_shapes() {
        assert!(challenge_valid(&challenge_for_verifier(VERIFIER)));
        assert!(!challenge_valid("short"));
        assert!(!challenge_valid(&"a".repeat(44)));
        assert!(!challenge_valid(&format!("{}=", "a".repeat(42))));
        assert!(verifier_valid(VERIFIER));
        assert!(!verifier_valid("tooshort"));
        assert!(!verifier_valid(&"a".repeat(129)));
        assert!(client_state_valid("abc-123._~"));
        assert!(!client_state_valid(""));
        assert!(!client_state_valid("a&b=c"));
        assert!(!client_state_valid(&"a".repeat(129)));
    }

    #[test]
    fn redirects_are_built() {
        assert_eq!(success_redirect("ab", None), "machina://auth?code=ab");
        assert_eq!(
            success_redirect("ab", Some("s1")),
            "machina://auth?code=ab&state=s1"
        );
        assert_eq!(
            error_redirect("oidc", Some("s1")),
            "machina://auth?error=oidc&state=s1"
        );
    }

    #[test]
    fn redeem_succeeds_once() {
        let store = NativeAuthStore::<String>::new();
        let code = store.mint("alice".to_string(), &challenge_for_verifier(VERIFIER));
        assert_eq!(store.redeem(&code, VERIFIER), Ok("alice".to_string()));
        assert_eq!(store.redeem(&code, VERIFIER), Err(RedeemError::InvalidCode));
    }

    #[test]
    fn wrong_verifier_consumes_code() {
        let store = NativeAuthStore::<String>::new();
        let code = store.mint("alice".to_string(), &challenge_for_verifier(VERIFIER));
        let other = "A".repeat(43);
        assert_eq!(
            store.redeem(&code, &other),
            Err(RedeemError::InvalidVerifier)
        );
        assert_eq!(store.redeem(&code, VERIFIER), Err(RedeemError::InvalidCode));
    }

    #[test]
    fn wrong_challenge_never_redeems() {
        let store = NativeAuthStore::<String>::new();
        let code = store.mint(
            "alice".to_string(),
            &challenge_for_verifier(&"B".repeat(43)),
        );
        assert_eq!(
            store.redeem(&code, VERIFIER),
            Err(RedeemError::InvalidVerifier)
        );
    }

    #[test]
    fn expired_code_is_rejected() {
        let store = NativeAuthStore::<String>::new();
        let code = store.mint("alice".to_string(), &challenge_for_verifier(VERIFIER));
        store.backdate_codes(Duration::from_secs(61));
        assert_eq!(store.redeem(&code, VERIFIER), Err(RedeemError::InvalidCode));
    }

    #[test]
    fn unknown_code_and_malformed_verifier() {
        let store = NativeAuthStore::<String>::new();
        assert_eq!(
            store.redeem("nope", VERIFIER),
            Err(RedeemError::InvalidCode)
        );
        let code = store.mint("alice".to_string(), &challenge_for_verifier(VERIFIER));
        assert_eq!(store.redeem(&code, ""), Err(RedeemError::InvalidVerifier));
    }

    #[test]
    fn flow_is_taken_once() {
        let store = NativeAuthStore::<String>::new();
        store.begin("st", "chal", Some("cs".to_string()));
        let flow = store.take_flow("st").expect("flow");
        assert_eq!(flow.code_challenge, "chal");
        assert_eq!(flow.client_state.as_deref(), Some("cs"));
        assert!(store.take_flow("st").is_none());
        assert!(store.take_flow("other").is_none());
    }

    #[test]
    fn codes_are_distinct_and_long() {
        let store = NativeAuthStore::<u8>::new();
        let a = store.mint(1, "x");
        let b = store.mint(1, "x");
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }
}
