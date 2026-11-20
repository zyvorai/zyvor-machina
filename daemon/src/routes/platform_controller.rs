// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Same-origin reverse proxy to machina-controller (:5093) for the web UI.

use axum::{
    body::Body,
    extract::{Path, Request},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    routing::any,
    Extension, Router,
};
use base64::Engine;
use machina_core::{LibvirtError, LibvirtManager};
use std::sync::LazyLock;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;

// Every platform dashboard load fans out to a dozen-plus controller-proxied API calls.
// A fresh reqwest::Client per request pays a new TCP connection (no keep-alive reuse)
// each time, which was the dominant contributor to multi-second dashboard load times.
// Building the client once lets reqwest pool and reuse connections across requests.
static CONTROLLER_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("build controller proxy http client")
});

pub(crate) fn controller_base() -> String {
    std::env::var("MACHINA_PLATFORM_CONTROLLER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:5093".into())
        .trim_end_matches('/')
        .to_string()
}

fn platform_service_basic_auth() -> Option<HeaderValue> {
    let creds = std::env::var("MACHINA_PLATFORM_AUTH")
        .ok()
        .filter(|s| !s.is_empty())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
    HeaderValue::from_str(&format!("Basic {encoded}")).ok()
}

fn forward_headers(src: &HeaderMap) -> HeaderMap {
    let mut h = HeaderMap::new();
    for name in ["content-type", "accept"] {
        if let Some(v) = src.get(name) {
            h.insert(name, v.clone());
        }
    }
    // When MACHINA_PLATFORM_AUTH is set, always use it — the browser's Bearer token
    // is a daemon JWT which the controller cannot verify (different JWT secret).
    // If MACHINA_PLATFORM_AUTH is not set, forward whatever the browser sent as a
    // fallback (allows shared-secret or API-key setups).
    if let Some(v) = platform_service_basic_auth() {
        h.insert(header::AUTHORIZATION, v);
    } else if let Some(v) = src.get(header::AUTHORIZATION) {
        h.insert(header::AUTHORIZATION, v.clone());
    }
    h
}

async fn platform_controller_proxy(
    Extension(actor): Extension<RequestActor>,
    Path(rest): Path<String>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    // This proxy replaces the caller's credential with the platform service account
    // (see forward_headers when MACHINA_PLATFORM_AUTH is set), so a low-privilege daemon
    // identity must not drive controller MUTATIONS as that service account. Reads stay
    // open to any authenticated user (platform dashboards); writes require write role +
    // the fleet:proxy scope.
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        require_write(&actor, "fleet:proxy")?;
    }
    let base = controller_base();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let path = if rest.is_empty() {
        String::new()
    } else if rest.starts_with('/') {
        rest
    } else {
        format!("/{rest}")
    };
    let url = format!("{base}{path}{query}");

    let method = req.method().clone();
    let headers = forward_headers(req.headers());
    let body_bytes = axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024)
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("read body: {e}"))))?;

    let mut rb = CONTROLLER_HTTP_CLIENT
        .request(method, &url)
        .headers(headers);
    if !body_bytes.is_empty() {
        rb = rb.body(body_bytes);
    }

    let resp = rb.send().await.map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "platform controller unreachable at {base}: {e}"
        )))
    })?;

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out_headers = HeaderMap::new();
    for name in [header::CONTENT_TYPE, header::CONTENT_DISPOSITION] {
        if let Some(v) = resp.headers().get(&name) {
            out_headers.insert(name, v.clone());
        }
    }
    let bytes = resp.bytes().await.map_err(|e| {
        AppError::from(LibvirtError::Internal(format!(
            "read controller response: {e}"
        )))
    })?;

    let mut out = Response::builder()
        .status(status)
        .body(Body::from(bytes))
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("response build: {e}"))))?;
    out.headers_mut().extend(out_headers);
    Ok(out)
}

pub fn platform_controller_routes() -> Router<LibvirtManager> {
    Router::new().route(
        "/platform/controller/{*rest}",
        any(platform_controller_proxy),
    )
}
