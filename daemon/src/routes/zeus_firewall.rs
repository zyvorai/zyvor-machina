// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Local Zeus Firewall inventory for single-node daemon.

use axum::{routing::get, Json, Router};
use machina_core::gather_firewall_inventory;
use serde_json::Value;

use crate::error::AppError;

async fn status() -> Json<Value> {
    Json(serde_json::json!({
        "zeus_firewall": {
            "feature": "zeus-firewall",
            "phase": 1,
            "ai_id": "AI-142",
            "ready": true
        }
    }))
}

async fn local_inventory() -> Result<Json<Value>, AppError> {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".into());
    let inv = gather_firewall_inventory(&hostname)
        .map_err(|e| AppError::from(machina_core::LibvirtError::Operation(e.to_string())))?;
    Ok(Json(serde_json::to_value(inv).unwrap_or_default()))
}

async fn local_overview() -> Result<Json<Value>, AppError> {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".into());
    let inv = gather_firewall_inventory(&hostname)
        .map_err(|e| AppError::from(machina_core::LibvirtError::Operation(e.to_string())))?;
    Ok(Json(serde_json::json!({
        "targets": [{
            "id": "local",
            "kind": "host",
            "name": hostname,
            "hostname": hostname,
            "enabled": inv.posture.enabled,
            "backend": inv.posture.backend.as_str(),
            "profile": inv.posture.profile,
            "risk": if inv.score.score < 70 { "warning" } else { "low" },
            "score": inv.score.score,
            "open_ports": inv.open_ports.len(),
            "blocked_today": 0,
            "agent_reachable": true
        }],
        "summary": format!("Local host · score {}/100", inv.score.score)
    })))
}

pub fn zeus_firewall_routes() -> Router<machina_core::LibvirtManager> {
    Router::new()
        .route("/zeus-firewall/status", get(status))
        .route("/zeus-firewall/overview", get(local_overview))
        .route("/zeus-firewall/local/inventory", get(local_inventory))
}
