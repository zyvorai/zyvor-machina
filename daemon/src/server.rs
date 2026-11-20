// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::Extension;
use axum::middleware;
use axum::Router;
use http::header;
use http::HeaderValue;
use machina_core::{LibvirtManager, MachinaConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tower::ServiceBuilder;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{self, SessionStore};
use crate::daemon_stats::DaemonStats;
use crate::http_metrics::{self, HttpMetrics};
use crate::job_registry::JobRegistry;
use crate::metrics_history::MetricsHistoryStore;
use crate::obs_workers::ObservabilityWorkers;
use crate::routes;
use crate::sprite_registry::{self, SpriteRegistry};
use crate::terminal::{self, TerminalSessionStore};
use crate::vessel_handle::VesselHandle;

pub async fn create_app(manager: LibvirtManager, config: MachinaConfig) -> Router {
    let web_dir = find_web_dist();
    let session_store = SessionStore::new(
        config.auth.max_sessions_global,
        config.auth.max_sessions_per_user,
    );
    let daemon_stats = Arc::new(DaemonStats::new({
        let s = session_store.clone();
        move || s.active_session_count()
    }));
    let http_metrics = Arc::new(HttpMetrics::new());
    let metrics_history_store = MetricsHistoryStore::new(config.metrics_history.max_points);
    let obs_workers = Arc::new(ObservabilityWorkers::new());
    obs_workers.start(
        &manager,
        &config,
        &metrics_history_store,
        &daemon_stats,
        &http_metrics,
    );
    let terminal_store = TerminalSessionStore::new();
    let console_session_store = routes::consolehub::ConsoleSessionStore::new();
    let vessel_handle = VesselHandle::from_config(&config.vessel).await;
    let ssh_terminal_cfg = config.ssh_terminal.clone();
    let mut auth_cfg = config.auth.clone();
    // Mirrors main.rs's bind_rustls-vs-plain-TCP decision so `machina_session`'s `Secure`
    // attribute (see auth.rs cookie-setting handlers) matches how this instance is actually
    // being served — never set on plain HTTP (browsers would just drop the cookie), always
    // set when TLS is really on.
    auth_cfg.tls_enabled = config.tls.is_effectively_enabled();
    let k8s_inventory_history_cfg = Arc::new(config.k8s_inventory_history.clone());

    let terminal_api = terminal::http_routes()
        .layer(Extension(terminal_store.clone()))
        .layer(Extension(ssh_terminal_cfg.clone()));

    let job_registry = std::sync::Arc::new(JobRegistry::new());
    let event_bus = std::sync::Arc::new(crate::routes::events::EventBus::new(256));
    let vib_build_slots = Arc::new(Semaphore::new(
        config.libvirt.virt_image_build_max_concurrent.max(1),
    ));
    let sprite_registry = std::sync::Arc::new(SpriteRegistry::new());
    sprite_registry::spawn_reaper(manager.clone(), (*sprite_registry).clone());

    // All routes under /api/v1 — auth routes skip middleware internally
    let api = routes::api_routes()
        .merge(terminal_api)
        .merge(auth::auth_routes(session_store.clone(), auth_cfg))
        .layer(Extension(console_session_store.clone()))
        .layer(Extension(job_registry))
        .layer(Extension(sprite_registry))
        .layer(Extension(event_bus))
        .layer(Extension(vessel_handle.clone()))
        .layer(Extension(vib_build_slots))
        .layer(Extension(k8s_inventory_history_cfg.clone()))
        .layer(Extension(daemon_stats.clone()))
        .layer(Extension(http_metrics.clone()))
        .layer(Extension(metrics_history_store.clone()))
        .layer(Extension(obs_workers.clone()))
        .route_layer(middleware::from_fn_with_state(
            session_store.clone(),
            auth::auth_middleware,
        ))
        .with_state(manager.clone());

    // WebSocket routes use single-use token auth via ?token= query parameter.
    // Clients first POST /api/v1/ws-token to get a short-lived token.
    let ws = routes::websocket_routes()
        .route_layer(middleware::from_fn_with_state(
            session_store.clone(),
            auth::ws_auth_middleware,
        ))
        .layer(Extension(terminal_store))
        .layer(Extension(ssh_terminal_cfg))
        .layer(Extension(vessel_handle))
        .with_state(manager);

    let mut router = Router::new().nest("/api/v1", api).nest("/ws/v1", ws);

    if let Some(novnc_dir) = find_novnc() {
        tracing::info!("Serving noVNC from {}", novnc_dir.display());
        router = router.nest_service("/novnc", ServeDir::new(&novnc_dir));
    }

    // Serve spice-html5 for SPICE console
    if let Some(spice_dir) = find_spice_html5() {
        tracing::info!("Serving spice-html5 from {}", spice_dir.display());
        router = router.nest_service("/spice-html5", ServeDir::new(&spice_dir));
    }

    if let Some(dir) = web_dir {
        tracing::info!("Serving web UI from {}", dir.display());
        let index = dir.join("index.html");
        // Avoid stale SPA shells after upgrades (hashed asset names change; old index.html must not linger in cache).
        let static_ui = ServiceBuilder::new()
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, must-revalidate"),
            ))
            .service(ServeDir::new(&dir).fallback(ServeFile::new(index)));
        router = router.fallback_service(static_ui);
    }

    // Outermost layer is listed last: Trace → Extension → record_request → routes.
    router
        .layer(middleware::from_fn(http_metrics::record_request))
        .layer(Extension(http_metrics))
        .layer(TraceLayer::new_for_http())
        // Prevent MIME-sniffing on API responses and served static assets.
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
}

fn find_web_dist() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let mut candidates = vec![
        PathBuf::from("/usr/local/share/machina/web"),
        PathBuf::from("/usr/share/machina/web"),
    ];
    if let Some(ref exe) = exe_dir {
        candidates.push(exe.join("web/dist"));
    }
    candidates.push(PathBuf::from("web/dist"));
    candidates.push(PathBuf::from("../web/dist"));
    candidates
        .into_iter()
        .find(|p| p.join("index.html").exists())
}

fn find_spice_html5() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/usr/share/spice-html5"),
        PathBuf::from("/usr/local/share/spice-html5"),
    ];
    candidates
        .into_iter()
        .find(|p| p.join("spice.html").exists() || p.join("spice_auto.html").exists())
}

fn find_novnc() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/usr/share/novnc"),
        PathBuf::from("/usr/local/share/novnc"),
        PathBuf::from("/usr/share/noVNC"),
    ];
    candidates.into_iter().find(|p| p.join("vnc.html").exists())
}
