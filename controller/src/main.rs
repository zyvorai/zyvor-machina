// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

#![allow(clippy::result_large_err, clippy::type_complexity)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{header, HeaderValue};
use clap::Parser;
use machina_controller::api;
use machina_controller::config::ControllerConfig;
use machina_controller::db;
use machina_controller::engine::{drs, ha, reconcile, scheduler, webhook_worker};
use machina_controller::leader;
use machina_controller::state::AppState;
use machina_controller::sync;
use machina_controller::tasks::bus::{FanoutTaskBus, InMemoryTaskBus, NatsTaskBus};
use machina_controller::tasks::{nats_subscriber, worker, TaskMessage};
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Parser)]
#[command(
    name = "machina-controller",
    about = "Machina central management controller"
)]
struct Cli {
    #[arg(long, env = "MACHINA_CONTROLLER_HOST")]
    host: Option<String>,
    #[arg(long, env = "MACHINA_CONTROLLER_PORT")]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // One process-wide TLS provider (tonic, the join listener and the HTTP clients all use rustls).
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();
    machina_controller::engine::ai::crypto::init();
    let cli = Cli::parse();
    let mut config = ControllerConfig::default();
    if let Some(host) = cli.host {
        config.host = host;
    }
    if let Some(port) = cli.port {
        config.port = port;
    }
    let config = Arc::new(config);

    // MACHINA_SKIP_AUTH=1 makes `auth_middleware` accept every request as a
    // hardcoded local admin with no credential check at all (see auth.rs). That
    // flag flips silently otherwise — nothing else in this process prints a
    // warning for it — so a dev `.env` accidentally carried into a production
    // deploy would disable authentication on every API/WS route with zero
    // signal in the logs. Make it impossible to miss.
    if std::env::var("MACHINA_SKIP_AUTH").ok().as_deref() == Some("1") {
        tracing::error!(
            "MACHINA_SKIP_AUTH=1 — AUTHENTICATION IS DISABLED. Every request is treated as \
             local admin 'dev' with no credential check. This must NEVER be set in production."
        );
    }

    // The built-in default is exactly 32 bytes, so a length-only check never fires on
    // it. A shipped default secret lets anyone forge an admin JWT, so REFUSE TO BOOT on
    // the known dev default unless an operator explicitly opts into dev mode. Same for
    // the default admin password.
    let dev_secrets_allowed = std::env::var("MACHINA_ALLOW_DEV_SECRETS").ok().as_deref()
        == Some("1")
        || std::env::var("MACHINA_SKIP_AUTH").ok().as_deref() == Some("1");
    if config.jwt_secret == "machina-dev-jwt-secret-change-me" {
        if dev_secrets_allowed {
            tracing::warn!(
                "MACHINA_JWT_SECRET is the built-in DEV DEFAULT — admin tokens are forgeable. Allowed only because MACHINA_ALLOW_DEV_SECRETS/MACHINA_SKIP_AUTH is set."
            );
        } else {
            tracing::error!(
                "Refusing to start: MACHINA_JWT_SECRET is unset (built-in DEV DEFAULT). Anyone could forge admin tokens. Set MACHINA_JWT_SECRET (>=32 random bytes) on the controller AND daemon, or set MACHINA_ALLOW_DEV_SECRETS=1 for local dev."
            );
            std::process::exit(1);
        }
    } else if config.jwt_secret.len() < 32 {
        tracing::warn!(
            "MACHINA_JWT_SECRET is shorter than 32 bytes ({} bytes) — set a strong secret in production",
            config.jwt_secret.len()
        );
    }
    if config.admin_password == "admin" && !dev_secrets_allowed {
        tracing::error!(
            "Refusing to start: default admin password 'admin' is in use. Set MACHINA_ADMIN_PASSWORD, or set MACHINA_ALLOW_DEV_SECRETS=1 for local dev."
        );
        std::process::exit(1);
    }

    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;
    db::ensure_bootstrap(&pool, &config.admin_user, &config.admin_password).await?;
    if let Err(e) = db::name_local_host(&pool).await {
        tracing::warn!("could not name the local host: {e}");
    }
    match machina_controller::agent_client::load_host_tokens(&pool).await {
        Ok(n) if n > 0 => tracing::info!("{n} host(s) use their own agent token"),
        Ok(_) => {}
        Err(e) => tracing::warn!("could not load per-host agent tokens: {e}"),
    }

    // Reap orphaned in-flight tasks left by a previous run — ONLY for the in-memory
    // task bus (no NATS). There, a restart drops the in-memory queue, so any task still
    // 'pending'/'running' in the DB will never be picked up; left alone these rows also
    // trip the per-host anti-backlog guard in sync.rs, wedging host.inventory so hosts
    // go stale → offline → VM placement fails "no online hosts available".
    //
    // With NATS configured (the multi-controller / HA topology) we must NOT do this: the
    // broker redelivers in-flight work, and a peer controller may own or be about to
    // claim these rows — blindly failing them would kill a peer's live task. In that
    // topology, ensure_bootstrap already performs a controller-scoped 'running' reap.
    if config.nats_url.is_none() {
        // Select (rather than blind-UPDATE) so each reaped row can be run through
        // finalize_terminal_task_failure: a 'running' row may already have taken
        // side effects (e.g. ha.recover writing the VM's new host_id) before this
        // restart, and a bare status write skips set_vm_error/ha.recover's host_id
        // revert/webhook dispatch — the same gap fixed for ensure_bootstrap's reap.
        match machina_controller::db::query_as::<_, (uuid::Uuid, String, serde_json::Value)>(
            "SELECT id, operation, payload FROM tasks WHERE status IN ('pending', 'running')",
        )
        .fetch_all(&pool)
        .await
        {
            Ok(rows) if !rows.is_empty() => {
                info!(
                    "reaping {} orphaned in-flight task(s) at startup",
                    rows.len()
                );
                for (task_id, operation, payload) in rows {
                    let msg = TaskMessage {
                        task_id,
                        operation,
                        payload,
                    };
                    worker::finalize_terminal_task_failure(
                        &pool,
                        &msg,
                        "orphaned by controller restart",
                    )
                    .await;
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("orphan task reap at startup failed: {e:#}"),
        }
    }

    let (local_bus, rx) = InMemoryTaskBus::new();
    let local_tx = local_bus.sender();
    let nats_bus = if let Some(url) = &config.nats_url {
        match NatsTaskBus::connect(url, config.controller_id.clone()).await {
            Ok(n) => {
                info!("NATS task fan-out enabled on {url}");
                nats_subscriber::spawn(url.clone(), local_tx, config.controller_id.clone());
                Some(n)
            }
            Err(e) => {
                tracing::warn!("NATS connect failed ({e:#}); using in-memory bus only");
                None
            }
        }
    } else {
        None
    };
    let task_bus = FanoutTaskBus::new(local_bus, nats_bus);

    let leader = leader::spawn(pool.clone(), config.controller_id.clone());
    let state = AppState::new(pool, config.clone(), task_bus, leader.clone());
    worker::spawn(state.clone(), rx);
    webhook_worker::spawn(state.pool.clone(), leader);
    sync::spawn_periodic(state.clone());
    reconcile::spawn(state.clone());
    machina_controller::engine::cloud::spawn(state.clone());
    machina_controller::engine::chaos::spawn(state.clone());
    machina_controller::engine::alarms::spawn(state.clone());
    machina_controller::engine::eip::spawn(state.clone());
    machina_controller::engine::natgw::spawn(state.clone());
    machina_controller::engine::lb_health::spawn(state.clone());
    machina_controller::engine::imds::spawn(state.clone());
    machina_controller::engine::preempt::spawn(state.clone());
    machina_controller::engine::vm_sleep::spawn(state.clone());
    machina_controller::engine::time_travel::spawn(state.clone());
    machina_controller::engine::stack_reconcile::spawn(state.clone());
    ha::spawn(state.clone());
    drs::spawn(state.clone());
    scheduler::spawn(state.clone());
    machina_controller::engine::ai::worker::spawn(state.clone());
    machina_controller::engine::zeus_firewall::worker::spawn(state.clone());
    machina_controller::engine::operations_scheduler::spawn(state.clone());
    machina_controller::engine::fleet_snapshot_scheduler::spawn(state.clone());
    machina_controller::engine::fleet_backup_scheduler::spawn(state.clone());
    machina_controller::engine::alert_evaluator::spawn(state.clone());
    machina_controller::engine::scheduled_jobs_runner::spawn(state.clone());
    machina_controller::engine::health_watchdog::spawn(state.clone());
    machina_controller::engine::host_revalidate::spawn(state.clone());
    machina_controller::engine::baremetal_profiles::spawn(state.clone());
    machina_controller::engine::baremetal::spawn_power_poll(state.clone());
    machina_controller::engine::channel_worker::spawn(state.pool.clone(), state.leader.clone());
    machina_controller::engine::observability::spawn_trace_writer(state.pool.clone());
    machina_controller::engine::cert_monitor::spawn(state.clone());
    machina_controller::engine::vm_schedule_runner::spawn(state.clone());
    machina_controller::engine::vm_netpol::spawn(state.clone());
    machina_controller::engine::soc::worker::spawn(state.clone());

    let app = api::router(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        // Prevent MIME-sniffing on API responses.
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ));

    if let Some(tls_addr) = machina_controller::enrollment_tls::configured_addr() {
        let tls_addr: SocketAddr = tls_addr.parse()?;
        let (st, public_url) = (state.clone(), config.public_base_url.clone());
        tokio::spawn(async move {
            if let Err(e) = machina_controller::enrollment_tls::serve(st, tls_addr, public_url).await {
                tracing::error!("join listener stopped: {e:#}");
            }
        });
    }

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    info!(
        "machina-controller listening on http://{addr} (id={})",
        config.controller_id
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
