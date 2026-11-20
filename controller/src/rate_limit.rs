// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use std::net::SocketAddr;

/// Cap on distinct tracked keys before we sweep expired buckets. The key is
/// derived from the (unverified) Authorization header, so an unauthenticated
/// caller can present a unique key per request — without a sweep the map grows
/// unbounded (OOM). One window's worth of keys stays well under this.
const MAX_TRACKED_KEYS: usize = 50_000;

struct Bucket {
    window_start: Instant,
    count: u32,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<String, Bucket>>>,
    limit: u32,
    window: Duration,
    enabled: bool,
    e2e_bypass_secret: Option<String>,
    jwt_secret: String,
}

impl RateLimiter {
    pub fn from_env(jwt_secret: String) -> Arc<Self> {
        let limit = std::env::var("MACHINA_RATE_LIMIT_PER_MIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let enabled = std::env::var("MACHINA_RATE_LIMIT_ENABLED")
            .ok()
            .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        let e2e_bypass_secret = std::env::var("MACHINA_E2E_BYPASS_SECRET")
            .ok()
            .filter(|s| !s.is_empty());
        Arc::new(Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            limit,
            window: Duration::from_secs(60),
            enabled,
            e2e_bypass_secret,
            jwt_secret,
        })
    }

    fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut map = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Evict fully-expired buckets before the map can grow without bound.
        if map.len() > MAX_TRACKED_KEYS {
            map.retain(|_, b| now.duration_since(b.window_start) < self.window);
        }
        let bucket = map.entry(key.to_string()).or_insert(Bucket {
            window_start: now,
            count: 0,
        });
        if now.duration_since(bucket.window_start) >= self.window {
            bucket.window_start = now;
            bucket.count = 0;
        }
        if bucket.count >= self.limit {
            return false;
        }
        bucket.count += 1;
        true
    }
}

/// Constant-time byte comparison so the e2e bypass secret can't be recovered
/// via response-timing (constant-time compare).
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

fn parse_basic_username(value: &str) -> Option<String> {
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, _) = decoded.split_once(':')?;
    if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    }
}

fn rate_limit_key(limiter: &RateLimiter, auth_value: Option<&str>) -> String {
    let Some(value) = auth_value else {
        return String::new();
    };
    if let Some(user) = parse_basic_username(value) {
        return format!("user:{user}");
    }
    if let Some(token) = value.strip_prefix("Bearer ") {
        let token = token.trim();
        if let Ok(claims) = crate::jwt::verify_token(&limiter.jwt_secret, token) {
            return format!("user:{}", claims.sub);
        }
        let prefix: String = token.chars().take(12).collect();
        return format!("bearer:{prefix}");
    }
    String::new()
}

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !limiter.enabled {
        return Ok(next.run(request).await);
    }

    if let Some(secret) = &limiter.e2e_bypass_secret {
        if let Some(hdr) = request.headers().get("x-machina-e2e") {
            // Constant-time compare: a `==` here leaks the secret one byte at a
            // time via response-time differences, letting an attacker brute-force
            // the bypass token (which lifts all rate-limiting, including on the
            // password-login route) without ever needing to see it.
            if let Ok(hdr) = hdr.to_str() {
                if ct_eq(hdr.as_bytes(), secret.as_bytes()) {
                    return Ok(next.run(request).await);
                }
            }
        }
    }

    // Always bucket by the real TCP peer address — never by X-Forwarded-For, which is
    // user-controlled and would allow any client to bypass the limit by spoofing that header.
    let ip_key = format!("ip:{}", addr.ip());
    let auth_header = request.headers().get(header::AUTHORIZATION);
    let key = rate_limit_key(&limiter, auth_header.and_then(|v| v.to_str().ok()));
    let key = if key.is_empty() { ip_key.clone() } else { key };

    if !limiter.check(&key) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(request).await)
}
