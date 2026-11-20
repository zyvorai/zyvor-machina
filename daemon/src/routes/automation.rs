// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use axum::extract::{Extension, Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use machina_core::libvirt::automation;
use machina_core::LibvirtManager;
use serde::Deserialize;

use crate::auth::RequestActor;
use crate::error::AppError;

// ── RBAC ───────────────────────────────────────────────────────────

async fn list_roles(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Enumerates every username on the host with its assigned role — the same
    // "who is an admin" recon value as `list_tokens` below, which already
    // requires admin. This had no check at all, letting any authenticated
    // caller (including a read-only or API-token session) list it.
    if !actor.role.is_admin() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Listing user roles requires the admin role.".into(),
        )
        .into());
    }
    let roles = automation::load_roles();
    let list: Vec<_> = roles
        .into_iter()
        .map(|(u, r)| serde_json::json!({"username": u, "role": r}))
        .collect();
    Ok(Json(serde_json::json!(list)))
}

#[derive(Deserialize)]
struct SetRoleRequest {
    username: String,
    role: automation::Role,
}

async fn set_role(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<SetRoleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.is_admin() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Assigning user roles requires the admin role.".into(),
        )
        .into());
    }
    automation::set_user_role(&req.username, req.role.clone())?;
    Ok(Json(
        serde_json::json!({"status": "ok", "username": req.username, "role": req.role}),
    ))
}

// ── API Tokens ─────────────────────────────────────────────────────

async fn list_tokens(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.is_admin() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Listing API tokens requires the admin role.".into(),
        )
        .into());
    }
    let tokens = automation::list_api_tokens();
    Ok(Json(serde_json::json!(tokens)))
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    name: String,
    username: String,
    role: automation::Role,
    #[serde(default)]
    scopes: Vec<String>,
}

async fn create_token(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.is_admin() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Creating API tokens requires the admin role.".into(),
        )
        .into());
    }
    let token =
        automation::create_api_token_scoped(&req.name, &req.username, req.role, req.scopes)?;
    Ok(Json(serde_json::json!(token)))
}

async fn delete_token(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.is_admin() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Deleting API tokens requires the admin role.".into(),
        )
        .into());
    }
    automation::delete_api_token(&token)?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

// ── Alerts ─────────────────────────────────────────────────────────

async fn list_alert_rules(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rules = automation::load_alert_rules();
    Ok(Json(serde_json::json!(rules)))
}

async fn save_alert_rules_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(rules): Json<Vec<automation::AlertRule>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Saving alert rules requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::save_alert_rules(&rules)?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn list_alerts(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let alerts = automation::load_alerts();
    Ok(Json(serde_json::json!(alerts)))
}

async fn acknowledge_alert_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Acknowledging alerts requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::acknowledge_alert(&id)?;
    Ok(Json(serde_json::json!({"status": "acknowledged"})))
}

// ── Webhooks ───────────────────────────────────────────────────────

async fn list_webhooks(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Webhook URLs (e.g. Slack incoming-webhook URLs) are bearer credentials in
    // themselves — anyone who has the URL can post as that integration. This
    // list was readable by any authenticated user regardless of role; gate it
    // like the save handler below.
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Viewing webhooks requires the operator or admin role.".into(),
        )
        .into());
    }
    let hooks = automation::load_webhooks();
    Ok(Json(serde_json::json!(hooks)))
}

async fn save_webhooks_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(hooks): Json<Vec<automation::WebhookConfig>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Saving webhooks requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::save_webhooks(&hooks)?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// ── Scheduled Actions ──────────────────────────────────────────────

async fn list_schedules(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schedules = automation::load_schedules();
    Ok(Json(serde_json::json!(schedules)))
}

async fn save_schedules_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(schedules): Json<Vec<automation::ScheduledAction>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Saving scheduled actions requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::save_schedules(&schedules)?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// ── Notification Channels ─────────────────────────────────────────

async fn list_notifications(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
) -> Result<Json<serde_json::Value>, AppError> {
    // NotificationChannel::config carries the raw secret (webhook URL, bot
    // token, ...). This was readable by any authenticated user regardless of
    // role; gate it like the save handler below so a read-only user can't
    // exfiltrate the credential.
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Viewing notification channels requires the operator or admin role.".into(),
        )
        .into());
    }
    let channels = automation::load_notification_channels();
    Ok(Json(serde_json::json!(channels)))
}

async fn save_notifications_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(channels): Json<Vec<automation::NotificationChannel>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Saving notification channels requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::save_notification_channels(&channels)?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[derive(Deserialize)]
struct TestNotificationRequest {
    channel: automation::NotificationChannel,
}

async fn test_notification(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(req): Json<TestNotificationRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Sending test notifications requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::send_notification(
        &req.channel,
        "machina test",
        "This is a test notification from machina.",
    )?;
    Ok(Json(serde_json::json!({"status": "sent"})))
}

// ── Snapshot Schedules ────────────────────────────────────────────

async fn list_snapshot_schedules(
    State(_m): State<LibvirtManager>,
) -> Result<Json<serde_json::Value>, AppError> {
    let schedules = automation::load_snapshot_schedules();
    Ok(Json(serde_json::json!(schedules)))
}

async fn save_snapshot_schedules_handler(
    State(_m): State<LibvirtManager>,
    Extension(actor): Extension<RequestActor>,
    Json(schedules): Json<Vec<automation::SnapshotSchedule>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !actor.role.can_write() {
        return Err(machina_core::LibvirtError::Forbidden(
            "Saving snapshot schedules requires the operator or admin role.".into(),
        )
        .into());
    }
    automation::save_snapshot_schedules(&schedules)?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn automation_routes() -> Router<LibvirtManager> {
    Router::new()
        // RBAC
        .route("/roles", get(list_roles).post(set_role))
        // API Tokens
        .route("/tokens", get(list_tokens).post(create_token))
        .route("/tokens/{token}", delete(delete_token))
        // Alerts
        .route(
            "/alert-rules",
            get(list_alert_rules).post(save_alert_rules_handler),
        )
        .route("/alerts", get(list_alerts))
        .route("/alerts/{id}/ack", post(acknowledge_alert_handler))
        // Webhooks
        .route("/webhooks", get(list_webhooks).post(save_webhooks_handler))
        // Schedules
        .route(
            "/schedules",
            get(list_schedules).post(save_schedules_handler),
        )
        // Notification Channels
        .route(
            "/notifications",
            get(list_notifications).post(save_notifications_handler),
        )
        .route("/notifications/test", post(test_notification))
        // Snapshot Schedules
        .route(
            "/snapshot-schedules",
            get(list_snapshot_schedules).post(save_snapshot_schedules_handler),
        )
}
