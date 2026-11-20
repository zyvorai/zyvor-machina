// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use serde::{Deserialize, Serialize};
use crate::db::DbPool;
use uuid::Uuid;

use crate::api::ApiError;
use crate::auth::AuthUser;
use crate::state::AppState;
use crate::tasks::enqueue::enqueue_task;

#[derive(Debug, Serialize)]
pub struct RebalanceMove {
    pub vm_id: String,
    pub vm_name: String,
    pub from_host: String,
    pub to_host: String,
    pub reason: String,
    pub score: f32,
}

#[derive(Debug, Serialize)]
pub struct RebalanceProposal {
    pub moves: Vec<RebalanceMove>,
    pub estimated_savings_pct: f32,
    pub summary: String,
}

pub async fn propose(pool: &DbPool, max_moves: usize) -> anyhow::Result<RebalanceProposal> {
    let recs = crate::engine::placement::compute_recommendations(pool).await?;
    let cap = max_moves.clamp(1, 20);
    let moves: Vec<RebalanceMove> = recs
        .into_iter()
        .take(cap)
        .map(|r| RebalanceMove {
            vm_id: r.vm_id,
            vm_name: r.vm_name,
            from_host: r.from_host_name,
            to_host: r.to_host_name,
            reason: r.reason,
            score: r.score,
        })
        .collect();

    let estimated_savings_pct = if moves.is_empty() {
        0.0
    } else {
        (moves.len() as f32 * 4.5).min(25.0)
    };

    let summary = if moves.is_empty() {
        "Cluster is balanced — no autonomous rebalancing recommended.".into()
    } else {
        format!(
            "AI rebalancer proposes {} live migration(s) to relieve hotspots (~{:.0}% efficiency gain).",
            moves.len(),
            estimated_savings_pct
        )
    };

    Ok(RebalanceProposal {
        moves,
        estimated_savings_pct,
        summary,
    })
}

#[derive(Debug, Deserialize)]
pub struct RebalanceExecuteBody {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_rebalance_execute_max")]
    pub max_moves: usize,
}

fn default_rebalance_execute_max() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub struct RebalanceExecuteResult {
    pub dry_run: bool,
    pub task_ids: Vec<String>,
    pub moves: Vec<RebalanceMove>,
    pub summary: String,
}

pub async fn execute(
    state: &AppState,
    actor: &AuthUser,
    body: &RebalanceExecuteBody,
) -> Result<RebalanceExecuteResult, ApiError> {
    let proposal = propose(&state.pool, body.max_moves)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if body.dry_run || proposal.moves.is_empty() {
        let empty = proposal.moves.is_empty();
        let move_count = proposal.moves.len();
        return Ok(RebalanceExecuteResult {
            dry_run: true,
            task_ids: vec![],
            moves: proposal.moves,
            summary: if empty {
                proposal.summary
            } else {
                format!(
                    "Preview: {} migration(s) would be enqueued — POST with dry_run=false to execute.",
                    move_count
                )
            },
        });
    }

    crate::auth::require_admin(actor)?;

    let mut task_ids = Vec::new();
    for mv in &proposal.moves {
        let vm_id = Uuid::parse_str(&mv.vm_id)
            .map_err(|_| ApiError::bad_request("invalid vm_id in proposal"))?;
        let dest_id = crate::db::query_scalar::<_, Uuid>("SELECT id FROM hosts WHERE hostname = ?")
            .bind(&mv.to_host)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::bad_request(format!("host not found: {}", mv.to_host)))?;

        let source_host: Option<Uuid> = crate::db::query_scalar("SELECT host_id FROM vms WHERE id = ?")
            .bind(vm_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let task_id = enqueue_task(
            state,
            "vm.migrate",
            serde_json::json!({
                "vm_id": mv.vm_id,
                "dest_host_id": dest_id.to_string(),
                "live": true,
                "ai_rebalance": true,
            }),
            Some("vm"),
            Some(vm_id),
            source_host,
        )
        .await?;
        task_ids.push(task_id.to_string());
    }

    state.emit_event(
        "ai.rebalance",
        format!(
            "AI rebalancer enqueued {} migration(s) by {}",
            task_ids.len(),
            actor.username
        ),
    );

    let enqueued = task_ids.len();
    Ok(RebalanceExecuteResult {
        dry_run: false,
        task_ids,
        moves: proposal.moves,
        summary: format!("Enqueued {enqueued} live migration task(s)."),
    })
}
