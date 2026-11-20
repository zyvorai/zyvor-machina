// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Keeps v2 stacks matching their templates: every minute each created stack
//! is compared with what exists. Stacks with `auto_heal` get missing VMs
//! recreated and their labels, policies and backup schedules re-applied;
//! the rest only record drift. Size changes are always just reported.

use std::time::Duration;

use uuid::Uuid;

use crate::state::AppState;

const TICK: Duration = Duration::from_secs(60);

pub async fn tick(state: &AppState) -> anyhow::Result<usize> {
    let rows: Vec<(Uuid, bool)> = crate::db::query_as(
        "SELECT id, auto_heal FROM stacks WHERE status = 'created' ORDER BY created_at",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut checked = 0;
    for (id, heal) in rows {
        match crate::api::stacks::reconcile_stack(state, id, heal).await {
            Ok(Some(_)) => checked += 1,
            Ok(None) => {}
            Err(e) => tracing::warn!(stack = %id, "stack reconcile: {}", e.message),
        }
    }
    Ok(checked)
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if !state.leader.is_leader() {
                continue;
            }
            if let Err(e) = tick(&state).await {
                tracing::warn!("stack reconcile: {e:#}");
            }
        }
    });
}
