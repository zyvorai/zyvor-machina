// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! "A new version is available" for the UI banner. Off unless the operator sets `MACHINA_UPDATE_CHECK_URL` to a
//! URL that returns `{"version": "x.y.z", "url": "release notes"}` (their own mirror, or a public feed): the
//! controller never contacts anything by itself, so an air-gapped site simply sees no banner.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::auth::{require_operator, AuthUser};

#[derive(Debug, Serialize)]
pub struct UpdateCheck {
    /// False when no check URL is configured: the UI shows nothing.
    pub enabled: bool,
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub notes_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Feed {
    version: String,
    #[serde(default)]
    url: Option<String>,
}

static CACHE: Mutex<Option<(Instant, Option<Feed>)>> = Mutex::new(None);
const CACHE_FOR: Duration = Duration::from_secs(6 * 3600);

/// Numeric components of a version (`v1.2.3-rc1+x` is 1, 2, 3); None when there is no number at all.
fn parts(v: &str) -> Option<Vec<u64>> {
    let core = v.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or("");
    let nums: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    nums.filter(|n| !n.is_empty())
}

pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parts(current), parts(latest)) {
        (Some(mut c), Some(mut l)) => {
            let n = c.len().max(l.len());
            c.resize(n, 0);
            l.resize(n, 0);
            l > c
        }
        _ => false,
    }
}

async fn fetch(url: &str) -> Option<Feed> {
    if let Some((at, feed)) = CACHE.lock().ok()?.as_ref() {
        if at.elapsed() < CACHE_FOR {
            return feed.clone();
        }
    }
    let feed = async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().ok()?;
        client.get(url).send().await.ok()?.error_for_status().ok()?.json::<Feed>().await.ok()
    }
    .await;
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), feed.clone()));
    }
    feed
}

pub async fn update_check(Extension(actor): Extension<AuthUser>) -> Result<Json<UpdateCheck>, ApiError> {
    require_operator(&actor)?;
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = std::env::var("MACHINA_UPDATE_CHECK_URL").unwrap_or_default();
    if url.trim().is_empty() {
        return Ok(Json(UpdateCheck { enabled: false, current, latest: None, available: false, notes_url: None }));
    }
    let feed = fetch(url.trim()).await;
    let available = feed.as_ref().is_some_and(|f| is_newer(&current, &f.version));
    Ok(Json(UpdateCheck {
        enabled: true,
        current,
        latest: feed.as_ref().map(|f| f.version.clone()),
        available,
        notes_url: feed.and_then(|f| f.url),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number_not_text() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("1.2", "v1.2.1"));
        assert!(!is_newer("1.2.0", "1.2"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.0", "garbage"));
        assert!(is_newer("0.1.0-rc1", "0.1.1+build5"));
    }
}
