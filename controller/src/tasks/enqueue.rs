// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use uuid::Uuid;

use crate::api::ApiError;
use crate::state::AppState;
use crate::tasks::TaskMessage;

pub async fn enqueue_task(
    state: &AppState,
    operation: &str,
    payload: serde_json::Value,
    resource_type: Option<&str>,
    resource_id: Option<Uuid>,
    host_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    let task_id = Uuid::new_v4();
    crate::db::query(
        "INSERT INTO tasks (id, operation, status, resource_type, resource_id, host_id, payload)
         VALUES (?, ?, 'pending', ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(operation)
    .bind(resource_type)
    .bind(resource_id)
    .bind(host_id)
    .bind(&payload)
    .execute(&state.pool)
    .await?;

    let msg = TaskMessage {
        task_id,
        operation: operation.to_string(),
        payload,
    };
    if let Err(e) = state.task_bus.publish("machina.tasks", &msg).await {
        // The task row was already committed as 'pending'. If the bus publish fails no worker
        // will ever pick it up, so mark it terminally failed instead of leaving it stuck. Route
        // through the same finalize helper every other terminal failure uses — a bare status
        // update here would skip set_vm_error/webhook dispatch for this failure mode.
        crate::tasks::worker::finalize_terminal_task_failure(
            &state.pool,
            &msg,
            &format!("task bus publish failed: {e}"),
        )
        .await;
        return Err(ApiError::internal(e.to_string()));
    }
    Ok(task_id)
}

pub async fn write_audit(
    state: &AppState,
    actor: &str,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    detail: serde_json::Value,
) -> Result<(), ApiError> {
    crate::db::query(
        "INSERT INTO audit_logs (id, actor, action, resource_type, resource_id, detail)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(detail)
    .execute(&state.pool)
    .await?;
    state.emit_event("audit", format!("{actor} {action}"));
    Ok(())
}
