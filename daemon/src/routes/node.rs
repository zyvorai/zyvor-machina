// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use machina_core::libvirt::node;
use machina_core::{LibvirtError, LibvirtManager, NodeInfo};

use crate::error::AppError;

async fn get_node_info(State(manager): State<LibvirtManager>) -> Result<Json<NodeInfo>, AppError> {
    let result = tokio::task::spawn_blocking(move || manager.with_conn(node::get_node_info))
        .await
        .map_err(|e| AppError::from(LibvirtError::Internal(format!("Task failed: {e}"))))?;
    Ok(Json(result?))
}

pub fn node_routes() -> Router<LibvirtManager> {
    Router::new().route("/node", get(get_node_info))
}
