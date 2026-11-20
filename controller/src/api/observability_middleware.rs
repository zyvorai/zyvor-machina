// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::engine::observability;
use crate::state::AppState;

pub async fn trace_middleware(
    State(_state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let start = Instant::now();
    let response = next.run(request).await;
    if path.starts_with("/api/v1/") {
        let status = response.status().as_u16() as i32;
        let duration_ms = start.elapsed().as_millis().min(i32::MAX as u128) as i32;
        observability::record_trace(&method, &path, status, duration_ms);
    }
    response
}
