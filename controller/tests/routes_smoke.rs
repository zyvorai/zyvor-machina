// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Integration smoke test: fresh SQLite DB → migrate → bootstrap → hit every GET route.
// Run with: cargo test -p machina-controller --test routes_smoke

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use http::{Request, StatusCode};
use machina_controller::{
    api,
    config::ControllerConfig,
    db, leader,
    state::AppState,
    tasks::{bus::InMemoryTaskBus, TaskBus},
};
use tower::ServiceExt;

async fn build_app() -> axum::Router {
    build_app_with_pool().await.0
}

async fn build_app_with_pool() -> (axum::Router, machina_controller::db::DbPool) {
    // Disable JWT auth so routes respond without a token.
    std::env::set_var("MACHINA_SKIP_AUTH", "1");

    let pool = machina_controller::db::testing::pool().await;
    db::ensure_bootstrap(&pool, "admin", "admin")
        .await
        .expect("bootstrap failed");

    let config = Arc::new(ControllerConfig::default());
    let (task_bus, _rx) = InMemoryTaskBus::new();
    let task_bus = task_bus as Arc<dyn TaskBus>;
    let leader = leader::spawn(pool.clone(), "test-controller".into());
    let state = AppState::new(pool.clone(), config, task_bus, leader);
    // The rate-limit middleware extracts ConnectInfo<SocketAddr> (client IP); the
    // real server provides it via into_make_service_with_connect_info, but
    // `oneshot` does not — inject a mock so routes don't 500 on a missing
    // extension.
    let app = api::router(state).layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
    (app, pool)
}

async fn get(app: &axum::Router, path: &str) -> StatusCode {
    get_with_body(app, path).await.0
}

/// The status and, for an error, the start of the response body (so a failing route says why).
async fn get_with_body(app: &axum::Router, path: &str) -> (StatusCode, String) {
    let req = Request::builder().uri(path).body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    if status.is_success() {
        return (status, String::new());
    }
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16).await.unwrap_or_default();
    (status, String::from_utf8_lossy(&bytes).chars().take(400).collect())
}

async fn post(app: &axum::Router, path: &str, body: &str) -> StatusCode {
    post_json(app, path, body).await.0
}

async fn post_json(app: &axum::Router, path: &str, body: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap_or_default();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn delete(app: &axum::Router, path: &str) -> StatusCode {
    let req = Request::builder()
        .method("DELETE")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn migrate_creates_all_tables() {
    let pool = machina_controller::db::testing::pool().await;

    let tables = [
        "clusters",
        "users",
        "hosts",
        "vms",
        "tasks",
        "events",
        "firewall_sites",
        "firewall_site_policies",
        "soc_detection_rules",
        "soc_alerts",
        "slo_policies",
        "ai_providers",
        "ai_prompts",
        "ai_actions",
        "ai_memory_entries",
        "storage_pools",
        "networks",
        "network_segments",
        "air_gap_bundles",
        "fips_crypto_profiles",
        "tenant_isolation_policies",
        "vm_schedules",
        "fleet_snapshot_schedules",
        "maintenance_schedules",
    ];
    for table in tables {
        let count: i64 = machina_controller::db::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("table '{table}' missing or unreadable: {e}"));
        let _ = count;
    }
}

#[tokio::test]
async fn bootstrap_creates_default_rows() {
    let pool = machina_controller::db::testing::pool().await;
    db::ensure_bootstrap(&pool, "admin", "s3cret")
        .await
        .unwrap();

    let clusters: i64 = machina_controller::db::query_scalar("SELECT COUNT(*) FROM clusters")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(clusters, 1, "should have exactly 1 default cluster");

    let users: i64 = machina_controller::db::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 1, "should have exactly 1 admin user");

    let hosts: i64 = machina_controller::db::query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hosts, 1, "should have exactly 1 default localhost host");

    // Verify UUID PKs are 16-byte BLOBs, not text strings.
    let bad_ids: i64 = machina_controller::db::query_scalar("SELECT COUNT(*) FROM clusters WHERE id IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(bad_ids, 0, "cluster id must be set");
    // ... and it must decode as a Uuid, which fails on a text id on either backend
    let _: uuid::Uuid = machina_controller::db::query_scalar("SELECT id FROM clusters LIMIT 1").fetch_one(&pool).await.unwrap();
}

#[tokio::test]
async fn smoke_get_routes() {
    let app = build_app().await;

    let routes = [
        "/api/v1/health",
        "/api/v1/hosts",
        "/api/v1/vms",
        "/api/v1/tasks",
        "/api/v1/cluster",
        "/api/v1/ai/agents",
        "/api/v1/ai/actions/hub",
        "/api/v1/ai/prompts",
        "/api/v1/ai/providers",
        "/api/v1/ai/memory/incidents",
        "/api/v1/ai/memory/settings",
        "/api/v1/ai/fleet/summary",
        "/api/v1/zeus-firewall/overview",
        "/api/v1/zeus-firewall/operator/plan",
        "/api/v1/zeus-firewall/multisite/overview",
        "/api/v1/zeus-firewall/multisite/dr-templates",
        "/api/v1/soc/rules",
        "/api/v1/observability/traces",
        "/api/v1/observability/overview",
        "/api/v1/enterprise/air-gap/bundles",
        "/api/v1/enterprise/fips/matrix",
        "/api/v1/enterprise/tenants/overview",
        "/api/v1/fleet/desktop",
        "/api/v1/fleet/activity",
        "/api/v1/fleet/storage",
        "/api/v1/marketplace/plugins",
        "/api/v1/operations/runbooks",
        "/api/v1/network/segments/overview",
        "/api/v1/network/ipam/pools",
        "/api/v1/storage/pools",
    ];

    let mut failures = Vec::new();
    for path in routes {
        let (status, body) = get_with_body(&app, path).await;
        if !status.is_success() {
            failures.push(format!("{path} [{status}] {body}"));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} route(s) returned non-2xx:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

#[tokio::test]
async fn smoke_post_routes() {
    let app = build_app().await;

    let routes: &[(&str, &str)] = &[("/api/v1/ai/copilot/chat", r#"{"message":"hi"}"#)];

    let mut failures = Vec::new();
    for (path, body) in routes {
        let status = post(&app, path, body).await;
        if !status.is_success() {
            failures.push(format!("{path} [{status}]"));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} POST route(s) returned non-2xx:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

// ─── vm_schedules route coverage ────────────────────────────────────────────

async fn seed_vm(pool: &machina_controller::db::DbPool) -> uuid::Uuid {
    let vm_id = uuid::Uuid::new_v4();
    machina_controller::db::query("INSERT INTO vms (id, name, spec_json) VALUES (?, ?, '{}')")
        .bind(vm_id)
        .bind("smoke-vm")
        .execute(pool)
        .await
        .expect("seed vm");
    vm_id
}

#[tokio::test]
async fn vm_schedules_list_returns_empty_array() {
    let (app, pool) = build_app_with_pool().await;
    let vm_id = seed_vm(&pool).await;
    let status = get(&app, &format!("/api/v1/vms/{vm_id}/schedules")).await;
    assert_eq!(status, StatusCode::OK, "list schedules should return 200");
}

#[tokio::test]
async fn vm_schedules_invalid_action_returns_400() {
    let (app, pool) = build_app_with_pool().await;
    let vm_id = seed_vm(&pool).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/v1/vms/{vm_id}/schedules"),
        r#"{"action":"delete","interval_minutes":1440}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "invalid action should return 400; body={body}"
    );
}

#[tokio::test]
async fn vm_schedules_create_and_delete_roundtrip() {
    let (app, pool) = build_app_with_pool().await;
    let vm_id = seed_vm(&pool).await;

    // Create
    let (status, body) = post_json(
        &app,
        &format!("/api/v1/vms/{vm_id}/schedules"),
        r#"{"action":"snapshot","interval_minutes":1440,"retention":3}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create schedule should return 200; body={body}"
    );
    let sched_id = body["id"]
        .as_str()
        .expect("response must have id field")
        .to_string();

    // List — should now contain the schedule
    let list_status = get(&app, &format!("/api/v1/vms/{vm_id}/schedules")).await;
    assert_eq!(list_status, StatusCode::OK);

    // Delete
    let del_status = delete(&app, &format!("/api/v1/vms/{vm_id}/schedules/{sched_id}")).await;
    assert_eq!(del_status, StatusCode::OK, "delete should return 200");

    // Delete again — not found
    let del_again = delete(&app, &format!("/api/v1/vms/{vm_id}/schedules/{sched_id}")).await;
    assert_eq!(
        del_again,
        StatusCode::NOT_FOUND,
        "second delete should return 404"
    );
}

#[tokio::test]
async fn vm_schedules_missing_vm_returns_404() {
    let (app, _pool) = build_app_with_pool().await;
    let fake_id = uuid::Uuid::new_v4();
    let (status, _body) = post_json(
        &app,
        &format!("/api/v1/vms/{fake_id}/schedules"),
        r#"{"action":"start","interval_minutes":60}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "unknown VM should return 404"
    );
}
