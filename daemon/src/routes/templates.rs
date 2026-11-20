// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::routing::get;
use axum::{Json, Router};

use machina_core::{LibvirtManager, VmTemplate};

async fn list_templates() -> Json<Vec<VmTemplate>> {
    Json(VmTemplate::all())
}

async fn list_saved_templates() -> Json<Vec<VmTemplate>> {
    Json(machina_core::libvirt::extras::list_saved_templates())
}

pub fn template_routes() -> Router<LibvirtManager> {
    Router::new()
        .route("/templates", get(list_templates))
        .route("/templates/saved", get(list_saved_templates))
}
