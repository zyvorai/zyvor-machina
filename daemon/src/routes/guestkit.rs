// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// GuestKit worker health proxy for Machina Migration Radar.

use axum::{
    extract::{Extension, Path, Query},
    routing::get,
    Json, Router,
};
use machina_core::{GuestkitConfig, LibvirtError, LibvirtManager, MachinaConfig};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::{require_write, RequestActor};
use crate::error::AppError;

fn guestkit_cfg() -> GuestkitConfig {
    MachinaConfig::load().guestkit
}

fn client(cfg: &GuestkitConfig) -> Result<reqwest::Client, AppError> {
    let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120));
    if cfg.insecure_tls {
        b = b.danger_accept_invalid_certs(true);
    }
    b.build()
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("guestkit http client: {e}"))))
}

fn base_url(cfg: &GuestkitConfig) -> Result<String, AppError> {
    if !cfg.enabled {
        return Err(AppError::from(LibvirtError::Forbidden(
            "guestkit.enabled is false in machina config".into(),
        )));
    }
    let u = cfg.base_url.trim().trim_end_matches('/');
    if u.is_empty() {
        return Err(AppError::from(LibvirtError::Invalid(
            "guestkit.base_url is empty".into(),
        )));
    }
    Ok(u.to_string())
}

async fn proxy_get(cfg: &GuestkitConfig, path: &str) -> Result<Value, AppError> {
    let client = client(cfg)?;
    let url = format!("{}{}", base_url(cfg)?, path);
    let resp = client.get(&url).send().await.map_err(|e| {
        AppError::from(LibvirtError::Operation(format!("guestkit GET {path}: {e}")))
    })?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({ "raw": "non-json response" }));
    if !status.is_success() {
        return Err(AppError::from(LibvirtError::Operation(format!(
            "guestkit GET {path} HTTP {status}: {body}"
        ))));
    }
    Ok(body)
}

async fn proxy_post(cfg: &GuestkitConfig, path: &str, body: Value) -> Result<Value, AppError> {
    let client = client(cfg)?;
    let url = format!("{}{}", base_url(cfg)?, path);
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        AppError::from(LibvirtError::Operation(format!(
            "guestkit POST {path}: {e}"
        )))
    })?;
    let status = resp.status();
    let out: Value = resp
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({ "raw": "non-json response" }));
    if !status.is_success() {
        return Err(AppError::from(LibvirtError::Operation(format!(
            "guestkit POST {path} HTTP {status}: {out}"
        ))));
    }
    Ok(out)
}

async fn guestkit_status() -> Result<Json<Value>, AppError> {
    let cfg = guestkit_cfg();
    let mut out = serde_json::json!({
        "enabled": cfg.enabled,
        "base_url": cfg.base_url,
        "insecure_tls": cfg.insecure_tls,
        "reachable": false,
        "library": "guestkit",
    });
    if !cfg.enabled {
        return Ok(Json(out));
    }
    match proxy_get(&cfg, "/api/v1/health").await {
        Ok(_) => {
            out["reachable"] = true.into();
        }
        Err(_) => {
            out["last_error"] = "guestkit-worker health check failed".into();
        }
    }
    Ok(Json(out))
}

async fn guestkit_capabilities() -> Result<Json<Value>, AppError> {
    let cfg = guestkit_cfg();
    Ok(Json(proxy_get(&cfg, "/api/v1/capabilities").await?))
}

async fn guestkit_list_jobs() -> Result<Json<Value>, AppError> {
    let cfg = guestkit_cfg();
    Ok(Json(proxy_get(&cfg, "/api/v1/jobs").await?))
}

async fn guestkit_get_job(Path(id): Path<String>) -> Result<Json<Value>, AppError> {
    let cfg = guestkit_cfg();
    Ok(Json(proxy_get(&cfg, &format!("/api/v1/jobs/{id}")).await?))
}

async fn guestkit_submit_job(
    Extension(actor): Extension<RequestActor>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    // Submits a (potentially long-running) migration-radar job on the
    // guestkit-worker backend; gate like every other mutating handler.
    require_write(&actor, "vms:write")?;
    let cfg = guestkit_cfg();
    Ok(Json(proxy_post(&cfg, "/api/v1/jobs", body).await?))
}

#[derive(Debug, Deserialize)]
struct GuestkitProxyQuery {
    path: String,
}

async fn guestkit_proxy_get(Query(q): Query<GuestkitProxyQuery>) -> Result<Json<Value>, AppError> {
    let cfg = guestkit_cfg();
    let path = if q.path.starts_with('/') {
        q.path
    } else {
        format!("/{}", q.path)
    };
    Ok(Json(proxy_get(&cfg, &path).await?))
}

async fn guestkit_proxy_post(
    Extension(actor): Extension<RequestActor>,
    Query(q): Query<GuestkitProxyQuery>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    // This is an open POST proxy onto the guestkit-worker backend (arbitrary
    // path + body); require write role like every other mutating handler.
    require_write(&actor, "vms:write")?;
    let cfg = guestkit_cfg();
    let path = if q.path.starts_with('/') {
        q.path
    } else {
        format!("/{}", q.path)
    };
    Ok(Json(proxy_post(&cfg, &path, body).await?))
}

pub fn guestkit_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/guestkit/status", get(guestkit_status))
        .route("/guestkit/capabilities", get(guestkit_capabilities))
        .route(
            "/guestkit/jobs",
            get(guestkit_list_jobs).post(guestkit_submit_job),
        )
        .route("/guestkit/jobs/{id}", get(guestkit_get_job))
        .route(
            "/guestkit/proxy",
            get(guestkit_proxy_get).post(guestkit_proxy_post),
        )
}
