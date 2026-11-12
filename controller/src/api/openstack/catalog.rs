// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

use super::auth::OsActor;
use super::OsError;

pub fn build_catalog(public_base: &str) -> Value {
    let base = public_base.trim_end_matches('/');
    json!([
        {
            "type": "identity",
            "id": "identity",
            "name": "keystone",
            "endpoints": [{
                "id": "identity-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v3")
            }]
        },
        {
            "type": "compute",
            "id": "compute",
            "name": "nova",
            "endpoints": [{
                "id": "compute-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v2.1")
            }]
        },
        {
            "type": "image",
            "id": "image",
            "name": "glance",
            "endpoints": [{
                "id": "image-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v2")
            }]
        },
        {
            "type": "volumev3",
            "id": "volumev3",
            "name": "cinderv3",
            "endpoints": [{
                "id": "volume-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v3")
            }]
        },
        {
            "type": "network",
            "id": "network",
            "name": "neutron",
            "endpoints": [{
                "id": "network-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v2.0")
            }]
        },
        {
            "type": "load-balancer",
            "id": "load-balancer",
            "name": "octavia",
            "endpoints": [{
                "id": "octavia-public",
                "interface": "public",
                "region": "RegionOne",
                "region_id": "RegionOne",
                "url": format!("{base}/v2.0")
            }]
        }
    ])
}

pub async fn auth_catalog(
    State(state): State<AppState>,
    Extension(_actor): Extension<OsActor>,
) -> Result<Json<Value>, OsError> {
    Ok(Json(json!({
        "catalog": build_catalog(&state.config.public_base_url)
    })))
}
